package dev.livingroom.streamlab;
import android.content.Context;
import android.hardware.display.DisplayManager;
import android.media.AudioFormat;
import android.media.AudioTrack;
import android.media.MediaCodecInfo;
import android.media.MediaCodecList;
import android.os.Bundle;
import android.view.Display;
import org.json.JSONArray;
import org.json.JSONObject;

/** Android adapter: exact requested mode, no decoder allocation or playback. */
final class MediaCapabilities {
    private static boolean levelSupports(int level,int width,int height,int fps) {
        int[] levels={1,2,4,8,16,32,64,128,256,512,1024,2048,4096,8192,16384,32768,65536};
        int[] size={99,99,396,396,396,396,792,1620,1620,3600,5120,8192,8192,8704,22080,36864,36864};
        int[] rate={1485,1485,3000,6000,11880,11880,19800,20250,40500,108000,216000,245760,245760,522240,589824,983040,2073600};
        long blocks=((width+15)/16)*((height+15)/16);
        for(int i=0;i<levels.length;i++)if(level==levels[i])return blocks<=size[i] && blocks*fps<=rate[i];
        return false;
    }
    static Bundle query(Context context,String arg) {
        try {
            JSONObject request=new JSONObject(arg),video=request.getJSONObject("video"),audio=request.getJSONObject("audio");
            int width=video.getInt("width"),height=video.getInt("height"),fps=video.getInt("fps");
            if(request.getInt("version")!=1 || width<160 || height<160 || width>4096 || height>4096 || (width&1)!=0 || (height&1)!=0 || fps<1 || fps>240)throw new IllegalArgumentException("Invalid media request");
            JSONObject result=new JSONObject();result.put("version",1);
            Display display=((DisplayManager)context.getSystemService(Context.DISPLAY_SERVICE)).getDisplay(Display.DEFAULT_DISPLAY);
            if(display==null)throw new IllegalStateException("No default display");
            Display.Mode mode=display.getMode();
            JSONObject output=new JSONObject();output.put("width",mode.getPhysicalWidth());output.put("height",mode.getPhysicalHeight());output.put("refresh_hz",mode.getRefreshRate());result.put("display",output);
            JSONArray decoders=new JSONArray();
            for(MediaCodecInfo info:new MediaCodecList(MediaCodecList.REGULAR_CODECS).getCodecInfos()) {
                if(info.isEncoder() || !info.isHardwareAccelerated())continue;
                for(String type:info.getSupportedTypes())if(type.equalsIgnoreCase("video/avc")) {
                    try {
                        MediaCodecInfo.CodecCapabilities caps=info.getCapabilitiesForType(type);
                        MediaCodecInfo.VideoCapabilities limits=caps.getVideoCapabilities();
                        if(!limits.areSizeAndRateSupported(width,height,fps))continue;
                        boolean high=false,baseline=false;
                        for(MediaCodecInfo.CodecProfileLevel p:caps.profileLevels) {
                            if(!levelSupports(p.level,width,height,fps))continue;
                            high|=p.profile==MediaCodecInfo.CodecProfileLevel.AVCProfileHigh;
                            baseline|=p.profile==MediaCodecInfo.CodecProfileLevel.AVCProfileBaseline || p.profile==MediaCodecInfo.CodecProfileLevel.AVCProfileConstrainedBaseline;
                        }
                        if(!high && !baseline)continue;
                        JSONObject decoder=new JSONObject();decoder.put("implementation",info.getName());decoder.put("codec","avc");decoder.put("mode",video);decoder.put("high",high);decoder.put("baseline",baseline);decoder.put("hardware",true);decoder.put("on_demand_idr",true);decoder.put("color","sdr709");decoder.put("max_bitrate_kbit",limits.getBitrateRange().getUpper()/1000);decoders.put(decoder);
                    }catch(IllegalArgumentException ignored){}
                }
            }
            result.put("video",decoders);
            JSONArray formats=new JSONArray();int sampleRate=audio.getInt("sample_rate");
            if(audio.getInt("channels")==2 && "pcm_s16_le".equals(audio.getString("sample_format")) && (sampleRate==44100 || sampleRate==48000) && AudioTrack.getMinBufferSize(sampleRate,AudioFormat.CHANNEL_OUT_STEREO,AudioFormat.ENCODING_PCM_16BIT)>0) {
                AudioOutput probe=new AudioOutput("auto",context,sampleRate);
                try {JSONObject format=new JSONObject();format.put("format",audio);format.put("low_latency_output",probe.lowLatency());formats.put(format);}finally{probe.release();}
            }
            result.put("audio",formats);
            Bundle bundle=new Bundle();bundle.putString("capabilities",result.toString());return bundle;
        }catch(Exception error){throw new IllegalArgumentException("Media capability query failed",error);}
    }
}
