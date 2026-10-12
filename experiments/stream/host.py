"""1080p60 SDR experiment. Native GStreamer media path with Python control, probes and PCM delivery."""
import argparse
import ipaddress
import json
import os
from pathlib import Path
import signal
import socket
import sys
import threading
import time
import queue
from audio_transport import PcmServer

ROOT = Path(__file__).resolve().parents[2]
BUNDLE = ROOT / "local/tools/gst-python"
if BUNDLE.exists():
    sys.path.insert(0, str(BUNDLE))
    import gstreamer_libs
    gstreamer_libs.setup_python_environment()
    _dll_handles = [os.add_dll_directory(p) for p in gstreamer_libs.gstreamer_env()[1].split(os.pathsep) if Path(p).is_dir()]
import gi
gi.require_version("Gst", "1.0")
gi.require_version("GstVideo", "1.0")
from gi.repository import Gst, GstVideo, GLib


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--device", required=True, type=ipaddress.IPv4Address)
    parser.add_argument("--port", type=int, default=5004)
    parser.add_argument("--bitrate", type=int, default=20000, help="kbit/s")
    parser.add_argument("--width", type=int, default=1920)
    parser.add_argument("--height", type=int, default=1080)
    parser.add_argument("--fps", type=int, default=60)
    parser.add_argument("--audio-rate", type=int, choices=(44100,48000), default=48000)
    parser.add_argument("--encoder-preset", choices=("p1", "p4"), default="p4",
                        help="P4 reduces static-detail flicker; P1 is retained for controlled comparisons")
    parser.add_argument("--encoder-profile", choices=("baseline", "high"), default="high")
    parser.add_argument("--gop-size", type=int, default=-1)
    parser.add_argument("--vbv-frames", type=int, default=1,
                        help="rate-control capacity in frame budgets; does not add a raw video queue")
    parser.add_argument("--test-pattern", action="store_true")
    parser.add_argument("--monitor-handle", type=lambda v: int(v, 0), default=0,
                        help="explicit HMONITOR selected by the Windows display adapter; no primary-screen fallback")
    parser.add_argument("--color-profile", choices=("sdr709", "goldfish-compat"), default="sdr709",
                        help="SDR BT.709 matrix; goldfish-compat is calibrated only for the TV emulator")
    parser.add_argument("--transport", choices=("tcp","udp"), default="tcp")
    parser.add_argument("--no-audio", action="store_true")
    parser.add_argument("--test-tone", action="store_true")
    parser.add_argument("--audio-exclude-pid", type=int, default=0)
    parser.add_argument("--audio-include-pid", type=int, default=0,
                        help="capture only this process tree, without changing the default audio endpoint")
    parser.add_argument("--audio-device", help="WASAPI render endpoint ID for device loopback (e.g. Steam Streaming Speakers)")
    parser.add_argument("--session-id", default="experiment")
    parser.add_argument("--control-stdin", action="store_true")
    parser.add_argument("--diagnostics", action="store_true", help="bounded per-frame diagnostic recording")
    parser.add_argument("--seconds", type=int, default=0, help="0: until Ctrl+C")
    parser.add_argument("--log", default=str(ROOT / "local/stream-host.jsonl"))
    args = parser.parse_args()
    if args.monitor_handle < 0 or (args.monitor_handle and args.test_pattern):
        parser.error("monitor-handle requires desktop capture and a positive native handle")
    if args.audio_include_pid and (args.audio_exclude_pid or args.audio_device or args.test_tone or args.no_audio):
        parser.error("audio-include-pid requires process loopback without other audio source overrides")
    if not 0 <= args.audio_include_pid <= 0xffffffff:
        parser.error("invalid audio-include-pid")
    if (args.gop_size != -1 and not 1 <= args.gop_size <= 600) or not 1 <= args.vbv_frames <= 8:
        parser.error("gop-size must be -1 (on-demand refresh) or 1..600; vbv-frames must be 1..8")
    if args.audio_device and (args.audio_exclude_pid or args.test_tone or args.no_audio):
        parser.error("--audio-device requires device loopback, without process exclusion/test tone/no audio")
    if not 1 <= args.port <= 65535 or not 1000 <= args.bitrate <= 100000 or args.seconds < 0 or not 0 <= args.audio_exclude_pid <= 0xffffffff:
        parser.error("invalid port, bitrate or duration")
    Gst.init(None)
    monitor = None
    if args.monitor_handle:
        from capture_target import monitor_bounds
        monitor = monitor_bounds(args.monitor_handle)
    if not 160 <= args.width <= 4096 or not 160 <= args.height <= 4096 or args.width%2 or args.height%2 or not 1 <= args.fps <= 240:
        parser.error("invalid video mode")
    for name in ("nvd3d11h264enc", "d3d11convert", "rtph264pay", "udpsink"):
        if not Gst.ElementFactory.find(name):
            raise RuntimeError("Required GStreamer plugin unavailable: " + name)
    source = (f"videotestsrc name=capture is-live=true pattern=ball ! video/x-raw,width={args.width},height={args.height},framerate={args.fps}/1 ! d3d11upload"
              if args.test_pattern else f"d3d11screencapturesrc name=capture show-cursor=true ! videorate drop-only=true ! video/x-raw(memory:D3D11Memory),framerate={args.fps}/1")
    # Capture, GPU conversion/scaling and NVENC stay in D3D11 memory.
    # Real TV uses standard limited BT.709 SDR metadata. Changing transfer alone
    # passed a fixed chart but did not fix normal desktop flicker; the current
    # encoder combination passed that observation, with root cause still open.
    # Goldfish was measured interpreting BT.709 YUV with BT.601 coefficients.
    # Change only its matrix; bt601 shorthand also changes transfer/primaries.
    colorimetry = "2:4:7:1" if args.color_profile == "goldfish-compat" else "2:3:5:1"
    description = (source + f" ! d3d11convert ! video/x-raw(memory:D3D11Memory),format=NV12,width={args.width},height={args.height},framerate={args.fps}/1,colorimetry={colorimetry} "
        "! queue max-size-buffers=1 max-size-time=0 max-size-bytes=0 leaky=downstream "
        f"! nvd3d11h264enc name=encoder preset={args.encoder_preset} tune=ultra-low-latency rc-mode=cbr "
        f"bitrate={args.bitrate} vbv-buffer-size={max(1,args.bitrate*args.vbv_frames//args.fps)} gop-size={args.gop_size} bframes=0 rc-lookahead=0 zerolatency=true "
        f"! video/x-h264,stream-format=byte-stream,alignment=au,profile={args.encoder_profile} "
        "! rtph264pay name=pay pt=96 mtu=1200 config-interval=-1 aggregate-mode=none timestamp-offset=0 "
        + (f"! udpsink host={args.device} port={args.port} sync=true async=false" if args.transport=="udp" else
           f"! rtpstreampay ! tcpserversink host=0.0.0.0 port={args.port} sync=true async=false"))
    if not args.no_audio:
        audio_source = f"audiotestsrc is-live=true samplesperbuffer={args.audio_rate//100} volume=0.02 wave=sine" if args.test_tone else "wasapi2src name=capture_audio loopback=true low-latency=true provide-clock=false slave-method=re-timestamp loopback-silence-on-device-mute=false"
        if not args.test_tone and args.audio_exclude_pid:
            audio_source += f" loopback-mode=exclude-process-tree loopback-target-pid={args.audio_exclude_pid}"
        if args.audio_include_pid:
            audio_source += f" loopback-mode=include-process-tree loopback-target-pid={args.audio_include_pid}"
        description += (f" {audio_source} ! audioconvert ! audioresample "
            f"! audio/x-raw,format=S16LE,rate={args.audio_rate},channels=2,layout=interleaved "
            "! appsink name=pcm emit-signals=true max-buffers=2 drop=true sync=false")
    pipeline = Gst.parse_launch(description)
    if args.monitor_handle:
        capture = pipeline.get_by_name("capture")
        if not capture.find_property("monitor-handle"):
            raise RuntimeError("Capture adapter does not support explicit monitor selection")
        capture.set_property("monitor-handle", args.monitor_handle)
    if args.audio_device:
        pipeline.get_by_name("capture_audio").set_property("device", args.audio_device)
    pipeline.use_clock(Gst.SystemClock.obtain())
    log_path = Path(args.log)
    log_path.parent.mkdir(parents=True, exist_ok=True)
    log = log_path.open("a", encoding="utf-8")
    lock = threading.Lock()
    def emit(event):
        event = {"host_monotonic_ns": time.perf_counter_ns(), "session_id": args.session_id, **event}
        line = json.dumps(event, ensure_ascii=False)
        with lock:
            print(line, flush=True)
            log.write(line + "\n")
            log.flush()
    loop = GLib.MainLoop()
    state = {"encoded": 0, "last": 0, "start": time.perf_counter(), "encode_ms": [], "submitted": {}, "pts_offset": None}
    captures = {}
    encoded_frames = {}
    trace_queue = queue.Queue(maxsize=240)
    trace_state = {"drops": 0, "last_pay_pts": None, "captures": 0, "encoder_caps": None}
    def capture_probe(pad, info):
        buffer = info.get_buffer()
        captures[buffer.pts] = time.perf_counter_ns()
        trace_state["captures"] += 1
        if len(captures) > 240: captures.pop(next(iter(captures)), None)
        return Gst.PadProbeReturn.OK
    if args.diagnostics:
        pipeline.get_by_name("capture").get_static_pad("src").add_probe(Gst.PadProbeType.BUFFER, capture_probe)
    encoder = pipeline.get_by_name("encoder")
    refresh = {"last_ns": 0, "sequence": 0, "count": 0}
    def request_keyframe(sequence):
        # Main-loop control event only; never grow a frame queue for recovery.
        event = GstVideo.video_event_new_downstream_force_key_unit(
            Gst.CLOCK_TIME_NONE, Gst.CLOCK_TIME_NONE, Gst.CLOCK_TIME_NONE, True, refresh["count"])
        accepted = encoder.get_static_pad("sink").send_event(event)
        emit({"type":"keyframe_requested", "sequence":sequence, "accepted":accepted})
        return False
    def input_probe(pad, info):
        buffer = info.get_buffer()
        state["submitted"][buffer.pts] = (time.perf_counter_ns(), captures.pop(buffer.pts, None))
        # Bound diagnostics even if the encoder stops producing output.
        if len(state["submitted"]) > 240:
            state["submitted"].pop(next(iter(state["submitted"])))
        return Gst.PadProbeReturn.OK
    def output_probe(pad, info):
        buffer = info.get_buffer()
        # NVENC may shift all output PTS by a constant to keep DTS non-negative.
        if state["pts_offset"] is None and state["submitted"]:
            state["pts_offset"] = buffer.pts - next(iter(state["submitted"]))
            emit({"type": "encoder_timestamp_offset", "ns": state["pts_offset"]})
        started = state["submitted"].pop(buffer.pts - (state["pts_offset"] or 0), None)
        if started is not None:
            output_ns = time.perf_counter_ns()
            state["encode_ms"].append((output_ns - started[0]) / 1e6)
            if args.diagnostics:
                encoded_frames[buffer.pts] = [started[1], started[0], output_ns]
                if len(encoded_frames) > 240: encoded_frames.pop(next(iter(encoded_frames)), None)
        state["encoded"] += 1
        if args.diagnostics:
            # Compressed-buffer metadata only: no raw-frame mapping or file I/O
            # on the media thread. Keep the existing timing schema unchanged.
            caps = pad.get_current_caps()
            caps_text = caps.to_string() if caps else None
            if caps_text != trace_state["encoder_caps"]:
                trace_state["encoder_caps"] = caps_text
                try: trace_queue.put_nowait(("encoder_format", caps_text))
                except queue.Full: trace_state["drops"] += 1
            row = [state["encoded"], buffer.pts, buffer.get_size(),
                   int(not buffer.has_flags(Gst.BufferFlags.DELTA_UNIT))]
            try: trace_queue.put_nowait(("encoder_frames", row))
            except queue.Full: trace_state["drops"] += 1
        return Gst.PadProbeReturn.OK
    encoder.get_static_pad("sink").add_probe(Gst.PadProbeType.BUFFER, input_probe)
    encoder.get_static_pad("src").add_probe(Gst.PadProbeType.BUFFER, output_probe)
    audio = PcmServer(args.device, diagnostics=args.diagnostics) if not args.no_audio else None
    media_offset = {"ns": 0, "known": False}
    def rtp_probe(pad, info):
        buffer = info.get_buffer()
        if args.diagnostics and buffer.pts != trace_state["last_pay_pts"]:
            trace_state["last_pay_pts"] = buffer.pts
            stages = encoded_frames.pop(buffer.pts, None)
            if stages:
                header = buffer.extract_dup(0, 12)
                timestamp = int.from_bytes(header[4:8], "big")
                try: trace_queue.put_nowait(("host_frames", [timestamp, *stages, time.perf_counter_ns()]))
                except queue.Full: trace_state["drops"] += 1
        if not media_offset["known"]:
            buffer = info.get_buffer()
            packet = buffer.extract_dup(0, min(buffer.get_size(), 12))
            if len(packet) >= 12:
                timestamp = int.from_bytes(packet[4:8], "big")
                segment = pad.get_sticky_event(Gst.EventType.SEGMENT, 0).parse_segment()
                running = segment.to_running_time(Gst.Format.TIME, buffer.pts)
                media_offset.update(ns=int(running)-timestamp*1000000000//90000, known=True)
                emit({"type":"media_clock", "video_rtp_offset_ns":media_offset["ns"]})
        return Gst.PadProbeReturn.OK
    pipeline.get_by_name("pay").get_static_pad("src").add_probe(Gst.PadProbeType.BUFFER, rtp_probe)
    audio_capture = {"buffers":0, "nonzero":0, "media_pts_ns":0, "running_ns":0}
    def pcm_sample(sink):
        sample = sink.emit("pull-sample")
        if sample:
            buffer = sample.get_buffer()
            pcm = buffer.extract_dup(0, buffer.get_size())
            audio_capture["buffers"] += 1
            if any(pcm): audio_capture["nonzero"] += 1
            if media_offset["known"]:
                segment = sample.get_segment()
                pts = segment.to_running_time(Gst.Format.TIME, buffer.pts)
                audio_capture["media_pts_ns"] = int(pts)
                if args.diagnostics:
                    audio_capture["running_ns"] = int(pipeline.get_clock().get_time() - pipeline.get_base_time())
                audio.offer(pcm, int(pts), media_offset["ns"])
        return Gst.FlowReturn.OK
    if audio: pipeline.get_by_name("pcm").connect("new-sample", pcm_sample)
    stopping = threading.Event()
    feedback = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    feedback.bind(("0.0.0.0", 5005))
    feedback.settimeout(0.5)
    def feedback_worker():
        while not stopping.is_set():
            try:
                data, remote = feedback.recvfrom(4096)
                received_ns = time.perf_counter_ns()
                if remote[0] == str(args.device):
                    metrics = json.loads(data)
                    if not isinstance(metrics, dict):
                        continue
                    if metrics.get("sessionId", "experiment") == args.session_id:
                        if metrics.get("type") == "clock_ping":
                            pong = {"type":"clock_pong", "sessionId":args.session_id, "sequence":metrics.get("sequence"),
                                    "clientNs":metrics.get("clientNs"), "hostReceiveNs":received_ns,"hostSendNs":time.perf_counter_ns()}
                            feedback.sendto(json.dumps(pong).encode(), remote)
                        elif metrics.get("type") == "keyframe_request":
                            sequence = metrics.get("sequence")
                            now = time.perf_counter_ns()
                            if (type(sequence) is int and 0 < sequence <= 9007199254740991
                                    and sequence > refresh["sequence"] and now-refresh["last_ns"] >= 500000000):
                                refresh.update(last_ns=now, sequence=sequence, count=refresh["count"]+1)
                                GLib.idle_add(request_keyframe, sequence)
                        elif metrics.get("type") == "client_frames":
                            emit({"type":"client_frames", "trace":metrics})
                        else:
                            emit({"type": "client", "metrics": metrics})
            except socket.timeout:
                pass
            except (ValueError, OSError):
                if stopping.is_set(): break
    thread = threading.Thread(target=feedback_worker, daemon=True)
    thread.start()
    trace_stop = threading.Event()
    def trace_writer():
        while not trace_stop.is_set() or not trace_queue.empty():
            items = []
            try: items.append(trace_queue.get(timeout=.1))
            except queue.Empty: continue
            while len(items) < 16:
                try: items.append(trace_queue.get_nowait())
                except queue.Empty: break
            for kind in ("host_frames", "encoder_frames"):
                rows = [value for item_kind, value in items if item_kind == kind]
                if rows: emit({"type":kind, "rows":rows})
            for kind, value in items:
                if kind == "encoder_format": emit({"type":kind, "caps":value})
    trace_thread = threading.Thread(target=trace_writer, daemon=True)
    if args.diagnostics: trace_thread.start()
    def stats():
        now = time.perf_counter()
        timings = sorted(state["encode_ms"])
        state["encode_ms"].clear()
        emit({"type": "host", "encoded_fps": round((state["encoded"] - state["last"]) / (now - state["start"]), 2),
              "encoder_ms_p50": round(timings[len(timings)//2], 2) if timings else None,
              "encoder_ms_p95": round(timings[min(len(timings)-1, int(len(timings)*.95))], 2) if timings else None,
              "audio_capture_buffers":audio_capture["buffers"], "audio_nonzero_buffers":audio_capture["nonzero"],
              "audio_media_pts_ns":audio_capture["media_pts_ns"], "audio_capture_running_ns":audio_capture["running_ns"],
              "audio_connected":audio.connected if audio else False, "audio_sent_bytes":audio.bytes if audio else 0,
              "audio_send_drops":audio.drops if audio else 0, "audio_transport_error":audio.error if audio else ""})
        if args.diagnostics:
            emit({"type":"host_diagnostics", "capture_frames":trace_state["captures"],"trace_drops":trace_state["drops"], **(audio.timing_stats() if audio else {})})
        state.update(last=state["encoded"], start=now)
        return True
    failed = []
    def bus_message(bus, message):
        if message.type == Gst.MessageType.ERROR:
            error, debug = message.parse_error()
            failed.append(str(error))
            emit({"type": "error", "message": str(error), "debug": debug})
            loop.quit()
        elif message.type == Gst.MessageType.EOS:
            loop.quit()
    bus = pipeline.get_bus()
    bus.add_signal_watch()
    bus.connect("message", bus_message)
    signal.signal(signal.SIGINT, lambda *_: loop.quit())
    if args.control_stdin:
        def control():
            for line in sys.stdin:
                if line.strip() == "stop": break
            GLib.idle_add(lambda: (loop.quit(), False)[1])
        threading.Thread(target=control, daemon=True).start()
    GLib.timeout_add_seconds(1, stats)
    if args.seconds: GLib.timeout_add_seconds(args.seconds, lambda: (loop.quit(), False)[1])
    emit({"type": "configuration", "pipeline": description, "width": args.width, "height": args.height, "fps": args.fps,"audioRate":args.audio_rate,"audio":bool(audio),
          "color_profile":args.color_profile,"colorimetry":colorimetry,"encoder_preset":args.encoder_preset,"audio_device":args.audio_device,
          "encoder_profile":args.encoder_profile,"gop_size":args.gop_size,"vbv_frames":args.vbv_frames,
          "monitor":monitor,"audio_include_pid":args.audio_include_pid,
          "encoder_frames_columns":["index", "encoded_pts_ns", "bytes", "keyframe"],
          "diagnostics":args.diagnostics,
          "latency_scope": "encoder input->encoded output; client first packet->Surface callback; neither is glass-to-glass"})
    try:
        if pipeline.set_state(Gst.State.PLAYING) == Gst.StateChangeReturn.FAILURE:
            raise RuntimeError("GStreamer could not enter PLAYING")
        result, current, pending = pipeline.get_state(5 * Gst.SECOND)
        if result == Gst.StateChangeReturn.FAILURE or current != Gst.State.PLAYING:
            raise RuntimeError("GStreamer PLAYING state was not confirmed")
        capture_caps = pipeline.get_by_name("capture").get_static_pad("src").get_current_caps()
        encoder_caps = pipeline.get_by_name("encoder").get_static_pad("sink").get_current_caps()
        emit({"type":"host_ready", "capture_caps":capture_caps.to_string() if capture_caps else None,
              "encoder_input_caps":encoder_caps.to_string() if encoder_caps else None})
        loop.run()
    finally:
        pipeline.set_state(Gst.State.NULL)
        stopping.set()
        thread.join(timeout=1)
        feedback.close()
        if audio: audio.close()
        trace_stop.set()
        if args.diagnostics: trace_thread.join(timeout=2)
        emit({"type":"host_stopped"})
        log.close()
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
