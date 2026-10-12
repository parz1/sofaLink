"""Bounded native capture/encode probe with real focus and frame evidence."""
import argparse
import ctypes
import json
from pathlib import Path
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'stream'))
from host import Gst
from capture_target import monitor_bounds


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--monitor-handle', type=lambda v: int(v, 0), required=True)
    parser.add_argument('--seconds', type=int, default=10)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--allow-primary', action='store_true', help='explicit baseline only')
    args = parser.parse_args()
    if not 1 <= args.seconds <= 60:
        parser.error('probe duration must be 1..60 seconds')
    monitor = monitor_bounds(args.monitor_handle)
    if monitor['primary'] and not args.allow_primary:
        raise RuntimeError('Independent-space probe requires a non-primary monitor')
    Gst.init(None)
    pipeline = Gst.parse_launch(
        'd3d11screencapturesrc name=capture show-cursor=true ! videorate drop-only=true '
        '! video/x-raw(memory:D3D11Memory),framerate=60/1 ! d3d11convert '
        '! video/x-raw(memory:D3D11Memory),format=NV12,width=1920,height=1080,colorimetry=2:3:5:1 '
        '! queue max-size-buffers=1 max-size-time=0 max-size-bytes=0 leaky=downstream '
        '! nvd3d11h264enc preset=p4 tune=ultra-low-latency rc-mode=cbr bitrate=20000 '
        'vbv-buffer-size=333 gop-size=-1 bframes=0 rc-lookahead=0 zerolatency=true '
        '! video/x-h264,profile=high ! appsink name=output sync=false max-buffers=2 drop=true')
    pipeline.get_by_name('capture').set_property('monitor-handle', args.monitor_handle)
    sink = pipeline.get_by_name('output')
    get_focus = ctypes.WinDLL('user32').GetForegroundWindow
    get_focus.restype = ctypes.c_void_p
    rows = []
    error = None
    start = time.perf_counter_ns()
    try:
        if pipeline.set_state(Gst.State.PLAYING) == Gst.StateChangeReturn.FAILURE:
            raise RuntimeError('Capture pipeline failed to start')
        deadline = time.monotonic() + args.seconds
        while time.monotonic() < deadline:
            sample = sink.emit('try-pull-sample', 200 * Gst.MSECOND)
            if sample:
                buffer = sample.get_buffer()
                rows.append({'elapsed_ns':time.perf_counter_ns()-start,
                             'pts_ns':int(buffer.pts), 'bytes':buffer.get_size(),
                             'foreground_window':int(get_focus() or 0)})
            message = pipeline.get_bus().timed_pop_filtered(0, Gst.MessageType.ERROR)
            if message:
                raise RuntimeError(str(message.parse_error()))
        if len(rows) < 2:
            raise RuntimeError('No sustained encoded output')
    except Exception as exc:
        error = str(exc)
    finally:
        pipeline.set_state(Gst.State.NULL)
    duration = (rows[-1]['elapsed_ns']-rows[0]['elapsed_ns'])/1e9 if len(rows)>1 else 0
    intervals = [(b['elapsed_ns']-a['elapsed_ns'])/1e6 for a,b in zip(rows,rows[1:])]
    result = {'version':1,'monitor':monitor,'success':error is None,'error':error,
              'scope':'host encoded-output cadence; not physical end-to-end latency',
              'encoded_frames':len(rows),'fps':(len(rows)-1)/duration if duration else 0,
              'gaps_over_25ms':sum(x>25 for x in intervals),'frames':rows}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(result,indent=2),encoding='utf-8')
    print(json.dumps({k:v for k,v in result.items() if k!='frames'}))
    return 0 if error is None else 1


if __name__=='__main__':
    sys.exit(main())
