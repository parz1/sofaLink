package dev.livingroom.streamlab;
import android.content.Intent;

/** Negotiated parameters; legacy standalone experiment defaults only here. */
final class SessionFormat {
    final int width,height,fps,sampleRate;
    final boolean requireLowLatencyAudio;
    SessionFormat(Intent intent) {
        width=intent.getIntExtra("videoWidth",1920);height=intent.getIntExtra("videoHeight",1080);fps=intent.getIntExtra("videoFps",60);sampleRate=intent.getIntExtra("audioRate",48000);
        requireLowLatencyAudio=intent.getBooleanExtra("requireLowLatencyAudio",false);
        if(width<160 || height<160 || width>4096 || height>4096 || (width&1)!=0 || (height&1)!=0 || fps<1 || fps>240 || (sampleRate!=44100 && sampleRate!=48000))throw new IllegalArgumentException("Unsupported session format");
    }
    String label(){return width+"×"+height+" · "+fps+" fps";}
}
