package dev.livingroom.streamlab;

import android.media.AudioAttributes;
import android.media.AudioFormat;
import android.media.AudioTimestamp;
import android.media.AudioTrack;
import android.os.Process;
import android.util.Log;
import java.io.DataInputStream;
import java.net.InetSocketAddress;
import java.net.Socket;

/** PCM stereo 48 kHz. Actual AAudio/AudioTrack playback timestamps drive video scheduling. */
public final class PcmPlayer implements AutoCloseable {
    volatile boolean running=true, playing=false;
    volatile long playedFrames, receivedFrames, resets, underruns, lastPacketNs;
    volatile double rms, bufferedMs, timestampAgeMs;
    volatile int outputBufferFrames,outputBurstFrames,outputCapacityFrames;
    volatile String error="";
    private long anchorMediaNs=-1, videoOffset, lastTimestampNs;
    private final AudioClock clock=new AudioClock();
    volatile long sourceUnderruns,silenceFrames,clockDrops;
    volatile long networkDrops;
    volatile long clockMediaNs,clockLocalNs,lastMediaNs,videoMediaNs;
    volatile int queueLimitFrames;
    private final String host;
    private final String outputSelection;
    private final int sampleRate;
    private final boolean requireLowLatency;
    private final android.content.Context context;
    private Socket socket;
    private AudioOutput track;
    volatile String backend="";
    volatile int outputMode;
    volatile int outputDeviceId,outputSharingMode;
    private Thread worker;
    public PcmPlayer(String host,String selection,android.content.Context context,int sampleRate,boolean requireLowLatency) {this.host=host;this.outputSelection=selection;this.context=context.getApplicationContext();this.sampleRate=sampleRate;this.requireLowLatency=requireLowLatency;}
    public void start() {worker=new Thread(()->run(),"pcm-playback");worker.start();}
    public long videoTargetNs(long rtpTimestamp) {
        videoMediaNs=(rtpTimestamp&0xffffffffL)*1000000000L/90000+videoOffset;
        return playing?clock.target(rtpTimestamp,System.nanoTime()):0;
    }
    private void run() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO);
        try {
            track=new AudioOutput(outputSelection,context,sampleRate);backend=track.backend();outputMode=track.mode();
            if(requireLowLatency && !track.lowLatency())throw new IllegalStateException("Required low-latency audio mode is unavailable");
            outputBufferFrames=track.bufferFrames();outputBurstFrames=track.burstFrames();outputCapacityFrames=track.capacityFrames();
            // Initialize the output before subscribing so device startup cannot backlog TCP audio.
            while(running) {
                try {socket=new Socket();socket.setTcpNoDelay(true);socket.setReceiveBufferSize(32768);socket.connect(new InetSocketAddress(host,5006),1000);break;}
                catch(Exception e){try{socket.close();}catch(Exception ignored){}if(!running)return;Thread.sleep(200);}
            }
            DataInputStream input=new DataInputStream(socket.getInputStream());byte[] pcm=new byte[65536];
            long written=0, expectedMedia=-1, playedBase=0;
            AudioTimestamp timestamp=new AudioTimestamp();
            long[] stamp=new long[3];
            while(running) {
                if(input.readInt()!=0x4c485041)throw new IllegalArgumentException("Invalid audio stream magic");
                int length=input.readInt();long media=input.readLong();videoOffset=input.readLong();
                if(length<4 || length>pcm.length || length%4!=0)throw new IllegalArgumentException("Invalid PCM length");
                input.readFully(pcm,0,length);lastPacketNs=System.nanoTime();lastMediaNs=media;
                // HDMI output startup/reopen can leave old PCM in the TCP socket.
                // Keep at most ~40 ms there; discard complete packets before the
                // next discontinuity flush. Discarded PCM never proves playback.
                if(input.available()>sampleRate*4*40/1000){networkDrops++;continue;}
                try {
                if(expectedMedia>=0 && Math.abs(media-expectedMedia)>5000000L) {
                    playedBase=playedFrames;
                    track.pause();track.flush();written=0;anchorMediaNs=-1;lastTimestampNs=0;playing=false;clock.reset();resets++;
                }
                if(anchorMediaNs<0)anchorMediaNs=media;
                int frames=length/4;expectedMedia=media+frames*1000000000L/sampleRate;
                long energy=0;
                for(int i=0;i<length;i+=2){int sample=(short)((pcm[i]&255)|((pcm[i+1]&255)<<8));energy+=(long)sample*sample;}
                rms=Math.sqrt(energy/(double)(length/2))/32768.0;
                // Native start is deferred until one burst / 10 ms is queued.
                if(!playing){track.play();playing=true;}
                int offset=0;
                while(running && offset<length) {
                    int count=track.write(pcm,offset,length-offset,media+(offset/4)*1000000000L/sampleRate);
                    // HDMI mode changes disconnect a native stream. Reopen it on this same worker.
                    if(count==-899)throw new AudioOutput.Disconnected();
                    if(count<0)throw new IllegalStateException(backend+" write failed: "+count);
                    if(count==0){Thread.sleep(1);continue;}offset+=count;
                }
                written+=frames;receivedFrames+=frames;
                if(track.getTimestamp(stamp,anchorMediaNs,timestamp)) {
                    long now=System.nanoTime();
                    clockMediaNs=stamp[2];clockLocalNs=stamp[1];
                    clock.update(stamp[2],stamp[1],now,videoOffset);
                    // Cumulative proof of actual playback survives a flush resetting the playback head.
                    lastTimestampNs=stamp[1];
                    long position=stamp[0]+Math.max(0,now-stamp[1])*sampleRate/1000000000L;
                    position=Math.min(written,position);
                    // Only timestamped device playback proves progress. Discarded/queued PCM
                    // and elapsed wall time must never increment the playback evidence counter.
                    playedFrames=playedBase+Math.min(written,Math.max(0,stamp[0]));
                    bufferedMs=Math.max(0,written-position)*1000.0/sampleRate;
                }
                timestampAgeMs=lastTimestampNs>0?(System.nanoTime()-lastTimestampNs)/1000000.0:1000000.0;
                underruns=track.getUnderrunCount();
                outputDeviceId=track.deviceId();outputSharingMode=track.sharingMode();
                sourceUnderruns=track.sourceUnderruns();silenceFrames=track.silenceFrames();
                clockDrops=track.clockDrops();queueLimitFrames=track.queueLimitFrames();
                } catch(AudioOutput.Disconnected e) {
                    // Handle disconnects during start / flush as well as write.
                    playedBase=playedFrames;playing=false;clock.reset();
                    track.release();Thread.sleep(100);track=new AudioOutput(outputSelection,context,sampleRate);backend=track.backend();outputMode=track.mode();
                    if(requireLowLatency && !track.lowLatency())throw new IllegalStateException("Required low-latency audio mode lost after disconnect");
                    outputBufferFrames=track.bufferFrames();outputBurstFrames=track.burstFrames();outputCapacityFrames=track.capacityFrames();
                    written=0;expectedMedia=-1;anchorMediaNs=-1;lastTimestampNs=0;resets++;
                }
            }
        } catch(Exception e){if(running){error=e.getMessage()==null?e.toString():e.getMessage();Log.e("StreamLab","AUDIO_ERROR",e);}}
        finally {playing=false;try{if(socket!=null)socket.close();}catch(Exception ignored){}if(track!=null){try{track.pause();track.flush();}catch(Exception ignored){}track.release();}}
    }
    public void close() {
        running=false;try{if(socket!=null)socket.close();}catch(Exception ignored){}
        try{if(worker!=null)worker.join(1000);}catch(InterruptedException e){Thread.currentThread().interrupt();}
    }
}
