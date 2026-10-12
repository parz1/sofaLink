"""Windows/NVENC adapter: probe the requested mode, never advertise untested GPU modes."""
import argparse
import json
from pathlib import Path
from host import Gst

def probe(request):
    Gst.init(None)
    video=request['video']; audio=request['audio']
    for name in ('d3d11screencapturesrc','d3d11upload','d3d11convert','nvd3d11h264enc','wasapi2src','audioconvert','audioresample','appsink','tcpserversink','rtph264pay','rtpstreampay'):
        if not Gst.ElementFactory.find(name):
            raise RuntimeError('Windows adapter plugin unavailable: '+name)
    encoder=Gst.ElementFactory.make('nvd3d11h264enc')
    max_bitrate=int(encoder.find_property('bitrate').maximum)
    if request['bitrate_kbit'] > max_bitrate:
        raise RuntimeError('Requested bitrate exceeds encoder capacity')
    profiles=[]
    for profile in ('high','baseline'):
        pipeline=Gst.parse_launch(
            f"videotestsrc num-buffers=3 ! video/x-raw,width={video['width']},height={video['height']},framerate={video['fps']}/1 ! d3d11upload ! d3d11convert ! video/x-raw(memory:D3D11Memory),format=NV12,colorimetry=2:3:5:1 ! nvd3d11h264enc preset=p4 tune=ultra-low-latency rc-mode=cbr bitrate={request['bitrate_kbit']} vbv-buffer-size={max(1,request['bitrate_kbit']//video['fps'])} gop-size=-1 bframes=0 rc-lookahead=0 zerolatency=true ! video/x-h264,profile={profile},stream-format=byte-stream,alignment=au ! fakesink sync=false")
        try:
            pipeline.set_state(Gst.State.PLAYING)
            message=pipeline.get_bus().timed_pop_filtered(5*Gst.SECOND,Gst.MessageType.EOS|Gst.MessageType.ERROR)
            if message and message.type==Gst.MessageType.EOS: profiles.append(profile)
        finally: pipeline.set_state(Gst.State.NULL)
    if not profiles: raise RuntimeError('NVENC cannot encode the requested mode/profile')
    audio_formats=[]
    # This adapter and LHPA v1 support stereo S16LE; unsupported requests are rejected.
    if audio['sample_format']=='pcm_s16_le' and audio['channels']==2 and audio['sample_rate'] in (44100,48000):
        pipeline=Gst.parse_launch(f"wasapi2src loopback=true low-latency=true provide-clock=false loopback-silence-on-device-mute=false ! audioconvert ! audioresample ! audio/x-raw,format=S16LE,rate={audio['sample_rate']},channels=2,layout=interleaved ! appsink max-buffers=2 drop=true sync=false")
        try:
            result=pipeline.set_state(Gst.State.PLAYING)
            state=pipeline.get_state(2*Gst.SECOND)
            error=pipeline.get_bus().timed_pop_filtered(0,Gst.MessageType.ERROR)
            if result!=Gst.StateChangeReturn.FAILURE and state[0]!=Gst.StateChangeReturn.FAILURE and error is None:
                audio_formats.append({'format':audio,'low_latency_output':None})
        finally: pipeline.set_state(Gst.State.NULL)
    return {'version':1,'video':[{'implementation':'nvd3d11h264enc','codec':'avc','mode':video,'high':'high' in profiles,'baseline':'baseline' in profiles,'hardware':True,'on_demand_idr':True,'color':'sdr709','max_bitrate_kbit':min(max_bitrate,100000)}], 'audio':audio_formats,'display':None}

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--request',required=True)
    request=json.loads(Path(parser.parse_args().request).read_text(encoding='utf-8-sig'))
    if request.get('version')!=1: raise RuntimeError('Unsupported request version')
    print(json.dumps(probe(request)),flush=True)
