package dev.livingroom.streamlab;
import android.media.MediaCodecInfo;
import android.media.MediaCodecList;
import android.media.MediaFormat;
import java.util.Locale;

/** Android discovery and driver hints. No device quirks in core or UI state. */
final class AndroidDecoderAdapter {
    static String select(SessionFormat request,String selected) {
        if(selected!=null && !selected.isEmpty())return selected;
        for(MediaCodecInfo info:new MediaCodecList(MediaCodecList.REGULAR_CODECS).getCodecInfos()) {
            if(info.isEncoder() || !info.isHardwareAccelerated())continue;
            for(String type:info.getSupportedTypes())if(type.equalsIgnoreCase("video/avc") && info.getCapabilitiesForType(type).getVideoCapabilities().areSizeAndRateSupported(request.width,request.height,request.fps))return info.getName();
        }
        throw new IllegalStateException("No hardware AVC decoder for "+request.label());
    }
    static MediaFormat format(SessionFormat request,String decoder,String compatibility) {
        MediaFormat video=MediaFormat.createVideoFormat("video/avc",request.width,request.height);
        if("fire-tv".equals(compatibility)) {
            video.setInteger(MediaFormat.KEY_COLOR_STANDARD,MediaFormat.COLOR_STANDARD_BT709);
            video.setInteger(MediaFormat.KEY_COLOR_RANGE,MediaFormat.COLOR_RANGE_LIMITED);
            video.setInteger(MediaFormat.KEY_COLOR_TRANSFER,MediaFormat.COLOR_TRANSFER_SDR_VIDEO);
        }
        video.setInteger(MediaFormat.KEY_FRAME_RATE,request.fps);
        // Software goldfish does not accept this hint. Keep its measured test adapter.
        if(!decoder.startsWith("c2.goldfish"))video.setInteger(MediaFormat.KEY_LOW_LATENCY,1);
        video.setInteger(MediaFormat.KEY_PRIORITY,0);
        // MTK's vendor hint supplements the standard hint on the measured Fire TV decoder.
        if(decoder.toLowerCase(Locale.US).startsWith("omx.mtk"))video.setInteger("vdec-lowlatency",1);
        video.setInteger(MediaFormat.KEY_MAX_INPUT_SIZE,2*1024*1024);
        return video;
    }
}
