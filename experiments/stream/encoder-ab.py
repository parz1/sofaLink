"""Controlled native NVENC experiment and OFFLINE pixel analysis; never used by live core.

Replay the exact same frozen image through every profile/GOP/VBV combination.
Encoding remains in native GStreamer/D3D11. NumPy reads software-decoded frames
only after encoding has stopped, so its cost is excluded from encoder timings.
"""
import argparse
import hashlib
import itertools
import json
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "local/tools/encoder-analysis"))
import numpy as np
from PIL import Image
import host

Gst = host.Gst


def p(values, quantile):
    return float(np.percentile(values, quantile)) if values else None


def encode(image, output, profile, gop, vbv, frames, bitrate, multi_pass="default", clip=None):
    source=("filesrc name=source ! rawvideoparse width=1920 height=1080 format=rgb framerate=60/1 ! identity sync=true "
            if clip else f"filesrc name=source ! pngdec ! imagefreeze is-live=true num-buffers={frames} ! video/x-raw,framerate=60/1 ")
    pipeline = Gst.parse_launch(
        source+"! d3d11upload ! d3d11convert "
        "! video/x-raw(memory:D3D11Memory),format=NV12,width=1920,height=1080,framerate=60/1,colorimetry=2:3:5:1 "
        "! queue max-size-buffers=1 max-size-time=0 max-size-bytes=0 leaky=downstream "
        f"! nvd3d11h264enc name=encoder preset=p4 tune=ultra-low-latency rc-mode=cbr bitrate={bitrate} "
        f"vbv-buffer-size={bitrate*vbv//60} gop-size={gop} multi-pass={multi_pass} bframes=0 rc-lookahead=0 zerolatency=true "
        f"! video/x-h264,stream-format=byte-stream,alignment=au,profile={profile} ! filesink name=record sync=false")
    pipeline.get_by_name("source").set_property("location", str(clip or image))
    pipeline.get_by_name("record").set_property("location", str(output / "encoded.h264"))
    encoder = pipeline.get_by_name("encoder")
    submitted, rows, offset = {}, [], [None]
    def incoming(pad, info):
        buffer = info.get_buffer()
        submitted[buffer.pts] = time.perf_counter_ns()
        if len(submitted) > 240: submitted.pop(next(iter(submitted)))
        return Gst.PadProbeReturn.OK
    def outgoing(pad, info):
        buffer = info.get_buffer()
        if offset[0] is None and submitted:
            offset[0] = buffer.pts - next(iter(submitted))
        start = submitted.pop(buffer.pts - (offset[0] or 0), None)
        rows.append({"index": len(rows), "pts_ns": buffer.pts, "bytes": buffer.get_size(),
                     "keyframe": not buffer.has_flags(Gst.BufferFlags.DELTA_UNIT),
                     "encode_ms": (time.perf_counter_ns()-start)/1e6 if start else None})
        return Gst.PadProbeReturn.OK
    encoder.get_static_pad("sink").add_probe(Gst.PadProbeType.BUFFER, incoming)
    encoder.get_static_pad("src").add_probe(Gst.PadProbeType.BUFFER, outgoing)
    try:
        if pipeline.set_state(Gst.State.PLAYING) == Gst.StateChangeReturn.FAILURE:
            raise RuntimeError("Encoding pipeline failed to start")
        message = pipeline.get_bus().timed_pop_filtered(45*Gst.SECOND, Gst.MessageType.ERROR | Gst.MessageType.EOS)
        if message is None: raise RuntimeError("Encoding timed out")
        if message.type == Gst.MessageType.ERROR: raise RuntimeError(str(message.parse_error()))
        caps = encoder.get_static_pad("src").get_current_caps().to_string()
    finally:
        pipeline.set_state(Gst.State.NULL)
    if len(rows) != frames:
        raise RuntimeError(f"Controlled replay lost frames: {len(rows)} of {frames}")
    (output / "encoded-frames.json").write_text(json.dumps(rows), encoding="utf-8")
    return rows, caps


def decode(output, rows, flat_mask):
    pipeline = Gst.parse_launch("filesrc name=source ! h264parse ! avdec_h264 ! videoconvert "
        "! video/x-raw,format=RGB,width=1920,height=1080 ! appsink name=frames sync=false max-buffers=2 drop=false")
    pipeline.get_by_name("source").set_property("location", str(output / "encoded.h264"))
    sink = pipeline.get_by_name("frames")
    previous = minimum = maximum = None
    transitions, count = [], 0
    # Exclude the identical two-second warmup for every configuration.
    warmup = 120
    try:
        pipeline.set_state(Gst.State.PLAYING)
        while True:
            sample = sink.emit("try-pull-sample", Gst.SECOND)
            if sample is None:
                message = pipeline.get_bus().timed_pop_filtered(0, Gst.MessageType.ERROR | Gst.MessageType.EOS)
                if message is not None and message.type == Gst.MessageType.ERROR:
                    raise RuntimeError(str(message.parse_error()))
                if sink.get_property("eos"): break
                raise RuntimeError("Offline decode stopped making progress")
            buffer = sample.get_buffer()
            ok, mapped = buffer.map(Gst.MapFlags.READ)
            if not ok: raise RuntimeError("Offline RGB mapping failed")
            try:
                frame = np.frombuffer(mapped.data, dtype=np.uint8).reshape(1080,1920,3).copy()
            finally:
                buffer.unmap(mapped)
            if count >= warmup:
                if minimum is None:
                    minimum, maximum = frame.copy(), frame.copy()
                else:
                    np.minimum(minimum, frame, out=minimum)
                    np.maximum(maximum, frame, out=maximum)
                if previous is not None:
                    delta = np.abs(frame.astype(np.int16)-previous.astype(np.int16))
                    transitions.append({"index":count, "keyframe":rows[count]["keyframe"],
                        "mean_channel_change":float(delta.mean()),
                        "flat_mean_change":float(delta[flat_mask].mean()),
                        "flat_p99_change":float(np.percentile(delta[flat_mask],99)),
                        "maximum_change":int(delta.max())})
                previous = frame
            count += 1
    finally:
        pipeline.set_state(Gst.State.NULL)
    if count != len(rows) or minimum is None: raise RuntimeError("Offline decoded frame count mismatch")
    span = maximum.astype(np.int16)-minimum.astype(np.int16)
    (output / "pixel-transitions.json").write_text(json.dumps(transitions), encoding="utf-8")
    return {"decoded_frames":count, "warmup_frames":warmup,
        "flat_mask_pixels":int(flat_mask.sum()), "maximum_channel_span":int(span.max()),
        "mean_channel_span":float(span.mean()), "flat_mean_span":float(span[flat_mask].mean()),
        "flat_p99_span":float(np.percentile(span[flat_mask],99)),
        "flat_maximum_span":int(span[flat_mask].max()),
        "keyframe_transition_mean":p([t["mean_channel_change"] for t in transitions if t["keyframe"]],50),
        "nonkey_transition_mean":p([t["mean_channel_change"] for t in transitions if not t["keyframe"]],50),
        "flat_key_transition_mean":p([t["flat_mean_change"] for t in transitions if t["keyframe"]],50),
        "flat_nonkey_transition_mean":p([t["flat_mean_change"] for t in transitions if not t["keyframe"]],50),
        "flat_transition_p95":p([t["flat_mean_change"] for t in transitions],95)}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image",type=Path)
    parser.add_argument("--out",type=Path,required=True)
    parser.add_argument("--frames",type=int,default=360)
    parser.add_argument("--bitrate",type=int,default=20000)
    parser.add_argument("--extra-only",action="store_true",help="one-frame VBV: infinite GOP and quarter-resolution multipass candidates")
    parser.add_argument("--dynamic-clip",type=Path,help="identical RGB 1920x1080/60 raw clip; moving pane at 800,400,640,360 excluded from flat mask")
    args=parser.parse_args()
    if not 240 <= args.frames <= 1200 or not 1000 <= args.bitrate <= 100000:
        parser.error("frames must be 240..1200 and bitrate 1000..100000")
    Gst.init(None)
    reference=np.asarray(Image.open(args.image).convert("RGB").resize((1920,1080)),dtype=np.int16)
    flat=np.ones((1080,1920),dtype=bool)
    # Common mask from the SAME source, never selected from each decoded result.
    for distance in (1,2,4):
        for axis in (0,1):
            for sign in (-1,1):
                flat &= np.max(np.abs(reference-np.roll(reference,sign*distance,axis=axis)),axis=2)<=1
    flat[:4]=flat[-4:]=False
    flat[:,:4]=flat[:,-4:]=False
    if args.dynamic_clip:
        flat[396:764,796:1444]=False
        if args.dynamic_clip.stat().st_size!=args.frames*1920*1080*3:
            raise RuntimeError("Raw reference clip size does not match frame count")
    if flat.sum()<1000: raise RuntimeError("Source has insufficient flat regions")
    args.out.mkdir(parents=True,exist_ok=True)
    result={"source_sha256":hashlib.sha256(args.image.read_bytes()).hexdigest(),
            "frames":args.frames,"fps":60,"bitrate_kbit":args.bitrate,
            "scope":"frozen source; native GPU encode then offline software decode; excludes TV/network/audio",
            "runs":[]}
    if args.dynamic_clip:
        with args.dynamic_clip.open("rb") as source_file:
            result["clip_sha256"]=hashlib.file_digest(source_file,"sha256").hexdigest()
        result["scope"]="identical dynamic clip; flat mask excludes moving pane; whole-frame spans include intentional motion"
    configurations=([("baseline",15,1,"default"),("high",120,1,"default"),("high",-1,1,"default")] if args.dynamic_clip else
                    [("high",-1,1,"default"),("high",120,1,"two-pass-quarter")]
                    if args.extra_only else [(a,b,c,"default") for a,b,c in itertools.product(("baseline","high"),(15,120),(1,4))])
    for profile,gop,vbv,multi_pass in configurations:
        name=f"{profile}-g{gop}-v{vbv}-m{multi_pass}" if args.extra_only else f"{profile}-g{gop}-v{vbv}"
        output=args.out/name
        output.mkdir(exist_ok=False)
        rows,caps=encode(args.image,output,profile,gop,vbv,args.frames,args.bitrate,multi_pass,args.dynamic_clip)
        pixels=decode(output,rows,flat)
        key_indices=[r["index"] for r in rows if r["keyframe"]]
        run={"name":name,"profile":profile,"gop":gop,"vbv_frames":vbv,"multi_pass":multi_pass,"caps":caps,
             "keyframe_intervals":sorted(set(b-a for a,b in zip(key_indices,key_indices[1:]))),
             "encode_p50_ms":p([r["encode_ms"] for r in rows[120:] if r["encode_ms"] is not None],50),
             "encode_p95_ms":p([r["encode_ms"] for r in rows[120:] if r["encode_ms"] is not None],95),
             "keyframe_bytes_p95":p([r["bytes"] for r in rows[120:] if r["keyframe"]],95),**pixels}
        result["runs"].append(run)
        (args.out/"results.json").write_text(json.dumps(result,indent=2),encoding="utf-8")
        print(json.dumps(run),flush=True)


if __name__=="__main__": main()
