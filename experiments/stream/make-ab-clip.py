"""Generate one immutable native RGB clip for encoder regression, not live streaming."""
import argparse
import json
from pathlib import Path
import host


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image",type=Path)
    parser.add_argument("output",type=Path)
    parser.add_argument("--frames",type=int,default=360)
    args=parser.parse_args()
    if not 240<=args.frames<=1200: parser.error("frames must be 240..1200")
    args.output.parent.mkdir(parents=True,exist_ok=True)
    if args.output.exists(): parser.error("Reference clip already exists; keep experiment inputs immutable")
    Gst=host.Gst
    Gst.init(None)
    pipeline=Gst.parse_launch(
        "compositor name=mix sink_0::width=1920 sink_0::height=1080 sink_1::xpos=800 sink_1::ypos=400 "
        "! videoconvert ! video/x-raw,format=RGB,width=1920,height=1080,framerate=60/1 ! filesink name=record sync=false "
        f"filesrc name=source ! pngdec ! imagefreeze num-buffers={args.frames} ! videoconvert ! video/x-raw,format=BGRA,framerate=60/1 ! mix.sink_0 "
        f"videotestsrc num-buffers={args.frames} pattern=ball ! video/x-raw,width=640,height=360,framerate=60/1 ! videoconvert ! video/x-raw,format=BGRA ! mix.sink_1")
    pipeline.get_by_name("source").set_property("location",str(args.image))
    pipeline.get_by_name("record").set_property("location",str(args.output))
    try:
        pipeline.set_state(Gst.State.PLAYING)
        message=pipeline.get_bus().timed_pop_filtered(45*Gst.SECOND,Gst.MessageType.ERROR|Gst.MessageType.EOS)
        if message is None: raise RuntimeError("Reference generation timed out")
        if message.type==Gst.MessageType.ERROR: raise RuntimeError(str(message.parse_error()))
    finally:
        pipeline.set_state(Gst.State.NULL)
    if args.output.stat().st_size!=args.frames*1920*1080*3:
        raise RuntimeError("Reference frame count or RGB stride mismatch")
    print(json.dumps({"frames":args.frames,"format":"RGB","width":1920,"height":1080,"fps":60,
                      "moving_rectangle":[800,400,640,360],"output":str(args.output)}))


if __name__=="__main__":main()
