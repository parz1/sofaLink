package dev.livingroom.streamlab;

import android.media.AudioAttributes;
import android.media.AudioFormat;
import android.media.AudioTimestamp;
import android.media.AudioTrack;
import android.media.AudioDeviceInfo;
import android.media.AudioManager;
import android.content.Context;
import android.util.Log;

/** Native AAudio first, portable AudioTrack fallback; no global audio settings changed. */
final class AudioOutput {
    static final class Disconnected extends IllegalStateException {
        Disconnected(){super("AAudio output disconnected");}
    }
    private long nativeHandle;
    private AudioTrack fallback;
    private final int sampleRate;
    private static native long openNative(boolean exclusive,int device,int sampleRate);
    private static native int writeNative(long handle,byte[] data,int offset,int length,long mediaNs);
    private static native boolean timestampNative(long handle,long[] values);
    private static native int statNative(long handle,int kind);
    private static native int commandNative(long handle,int command);
    AudioOutput(String selection,Context context,int sampleRate) {
        this.sampleRate=sampleRate;
        int device=0;
        if("aaudio-routed".equals(selection)) {
            AudioManager manager=(AudioManager)context.getSystemService(Context.AUDIO_SERVICE);
            for(AudioDeviceInfo info:manager.getDevices(AudioManager.GET_DEVICES_OUTPUTS)) {
                Log.i("StreamLab","AUDIO_DEVICE id="+info.getId()+" type="+info.getType()+" name="+info.getProductName());
                if(info.getType()==AudioDeviceInfo.TYPE_HDMI)device=info.getId();
            }
        }
        if(!"audiotrack".equals(selection))try {
            System.loadLibrary("streamaudio");
            boolean automatic=selection==null || "auto".equals(selection);
            nativeHandle=openNative(automatic || "aaudio-exclusive".equals(selection),device,sampleRate);
            if(automatic && nativeHandle!=0 && statNative(nativeHandle,2)!=12) {
                Log.w("StreamLab","AAudio low latency denied; selecting bounded AudioTrack fallback");
                int closed=commandNative(nativeHandle,2);
                if(closed<0)throw new IllegalStateException("Cannot release rejected AAudio stream: "+closed);
                nativeHandle=0;
            }
        }catch(UnsatisfiedLinkError ignored){}
        if(nativeHandle==0) {
            int requestedFrames=sampleRate/50;
            int min=AudioTrack.getMinBufferSize(sampleRate,AudioFormat.CHANNEL_OUT_STEREO,AudioFormat.ENCODING_PCM_16BIT);
            try {fallback=createTrack(requestedFrames*4);}catch(IllegalArgumentException | UnsupportedOperationException e){fallback=createTrack(Math.max(min,requestedFrames*4));}
            if(fallback.getState()!=AudioTrack.STATE_INITIALIZED)throw new IllegalStateException("AudioTrack not initialized");
            fallback.setBufferSizeInFrames(requestedFrames);
        }
        Log.i("StreamLab","AUDIO_READY backend="+backend()+" bufferFrames="+bufferFrames()+" mode="+mode()+" requestedDevice="+device+" actualDevice="+(nativeHandle!=0?statNative(nativeHandle,9):0));
    }
    private AudioTrack createTrack(int bytes) {
        return new AudioTrack.Builder()
                .setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_GAME).build())
                .setAudioFormat(new AudioFormat.Builder().setSampleRate(sampleRate).setChannelMask(AudioFormat.CHANNEL_OUT_STEREO).setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
                .setTransferMode(AudioTrack.MODE_STREAM).setBufferSizeInBytes(bytes)
                .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY).build();
    }
    String backend(){return nativeHandle!=0?"AAudio":"AudioTrack";}
    boolean lowLatency(){return nativeHandle!=0?mode()==12:mode()==AudioTrack.PERFORMANCE_MODE_LOW_LATENCY;}
    int bufferFrames(){return nativeHandle!=0?statNative(nativeHandle,0):fallback.getBufferSizeInFrames();}
    int burstFrames(){return nativeHandle!=0?statNative(nativeHandle,3):0;}
    int capacityFrames(){return nativeHandle!=0?statNative(nativeHandle,4):fallback.getBufferCapacityInFrames();}
    int mode(){return nativeHandle!=0?statNative(nativeHandle,2):fallback.getPerformanceMode();}
    int deviceId(){if(nativeHandle!=0)return statNative(nativeHandle,9);AudioDeviceInfo device=fallback.getRoutedDevice();return device==null?0:device.getId();}
    int sharingMode(){return nativeHandle!=0?statNative(nativeHandle,10):-1;}
    private void command(int kind){int result=commandNative(nativeHandle,kind);if(result==-899)throw new Disconnected();if(result<0)throw new IllegalStateException("AAudio command failed: "+result);}
    void play(){if(nativeHandle!=0)command(0);else fallback.play();}
    void pause(){if(nativeHandle==0)fallback.pause();}
    void flush(){if(nativeHandle!=0)command(1);else fallback.flush();}
    int write(byte[] pcm,int offset,int length,long mediaNs){
        if(nativeHandle!=0)return writeNative(nativeHandle,pcm,offset,length,mediaNs);
        return fallback.write(pcm,offset,length,AudioTrack.WRITE_BLOCKING);
    }
    boolean getTimestamp(long[] values,long anchorMediaNs,AudioTimestamp fallbackTimestamp){
        if(nativeHandle!=0)return timestampNative(nativeHandle,values);
        if(!fallback.getTimestamp(fallbackTimestamp))return false;
        values[0]=fallbackTimestamp.framePosition;values[1]=fallbackTimestamp.nanoTime;
        values[2]=anchorMediaNs+values[0]*1000000000L/sampleRate;return true;
    }
    int getUnderrunCount(){return nativeHandle!=0?statNative(nativeHandle,1):fallback.getUnderrunCount();}
    int sourceUnderruns(){return nativeHandle!=0?statNative(nativeHandle,5):0;}
    int silenceFrames(){return nativeHandle!=0?statNative(nativeHandle,6):0;}
    int clockDrops(){return nativeHandle!=0?statNative(nativeHandle,7):0;}
    int queueLimitFrames(){return nativeHandle!=0?statNative(nativeHandle,8):0;}
    void release(){if(nativeHandle!=0){int result=commandNative(nativeHandle,2);if(result<0)throw new IllegalStateException("AAudio release failed: "+result);nativeHandle=0;}else fallback.release();}
}
