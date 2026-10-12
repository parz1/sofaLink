package dev.livingroom.streamlab;

import android.app.Activity;
import android.graphics.Color;
import android.graphics.SurfaceTexture;
import android.media.MediaCodec;
import android.media.MediaCodecInfo;
import android.media.MediaCodecList;
import android.media.MediaFormat;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.Process;
import android.util.Log;
import android.view.Display;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.InputDevice;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.TextureView;
import android.view.View;
import android.view.WindowManager;
import android.widget.FrameLayout;
import android.widget.TextView;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.SocketTimeoutException;
import java.net.Socket;
import java.io.DataInputStream;
import java.nio.ByteBuffer;
import java.util.ArrayList;
import java.util.ArrayDeque;
import java.util.Collections;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import org.json.JSONObject;
import org.json.JSONArray;

public final class MainActivity extends Activity implements SurfaceHolder.Callback {
    private static final String TAG="StreamLab";
    private final Handler ui=new Handler(Looper.getMainLooper());
    private TextView status;
    private SurfaceView videoSurface;
    private TextureView textureSurface;
    private Surface textureOutput;
    private long textureUpdates,lastTextureUpdateNs;
    private Receiver receiver;
    private PcmPlayer audio;
    private SessionFormat sessionFormat;
    private final android.view.Choreographer.FrameCallback presentationVsync=new android.view.Choreographer.FrameCallback(){public void doFrame(long frameTime){
        Receiver r=receiver;if(r==null || !r.running)return;
        r.presentationClock.vsync(frameTime,r.appVsyncOffsetNs,System.nanoTime());
        android.view.Choreographer.getInstance().postFrameCallback(this);
    }};
    private InputSender input;
    private ClockSync clockSync;
    private View inputView;
    private float pointerX,pointerY;
    private boolean pointerKnown;
    private long feedbackSequence;
    private final ArrayDeque<long[]> renderProgress=new ArrayDeque<>();
    private final Runnable tick=new Runnable() { public void run() {
        Receiver r=receiver;
        if (r!=null) {
            Display display=getWindowManager().getDefaultDisplay();
            double refresh=display.getMode().getRefreshRate();
            if(refresh!=r.presentationRefreshHz){r.presentationRefreshHz=refresh;r.appVsyncOffsetNs=display.getAppVsyncOffsetNanos();r.presentationClock.setRefreshRate(refresh);}
            long now=System.nanoTime();
            long presented=textureSurface==null?r.rendered:textureUpdates;
            renderProgress.addLast(new long[]{now,presented});
            while(renderProgress.size()>5)renderProgress.removeFirst();
            long[] first=renderProgress.peekFirst();
            Map<String,Double> performance=r.timings.summary(now);
            double callbackFps=now==first[0]?0:(presented-first[1])*1e9/(now-first[0]);
            double fps=performance.get("lastPresentAgeMs")>2000?0:performance.get("renderFps");
            if(textureSurface!=null)fps=now-lastTextureUpdateNs>2000000000L?0:callbackFps;
            List<Double> samples;
            synchronized(r.latencies) {samples=new ArrayList<>(r.latencies);r.latencies.clear();}
            Collections.sort(samples);
            double p50=samples.isEmpty()?0:samples.get(samples.size()/2);
            double p95=samples.isEmpty()?0:samples.get(Math.min(samples.size()-1,(int)(samples.size()*.95)));
            boolean waiting=r.running && (now-r.lastPacketNs>2000000000L || fps<=0);
            String timing=samples.isEmpty()?"—":String.format(Locale.US,"%.1f ms",p50);
            boolean sound=audio!=null && audio.playing && audio.playedFrames>0 && audio.timestampAgeMs<500 && audio.error.isEmpty() && now-audio.lastPacketNs<2000000000L;
            String phase=!r.running?r.message:waiting?"等待画面…":audio!=null && !audio.error.isEmpty()?"声音异常":sound?"正在播放":"等待声音…";
            String text=String.format(Locale.US,"sofaLink · %s\n%s · %.1f fps\n画面 %s · 声音 %s\n客户端呈现 %s\n电脑控制 %s\n方向键/确认/返回 → 电脑\n菜单隐藏统计 · 长按返回结束",
                phase,r.format,fps,waiting?"等待":"正常",sound?"正常":"等待",timing,input==null?"未启用":input.status);
            if(input!=null && input.ready && hasWindowFocus() && inputView!=null && !inputView.hasPointerCapture())inputView.requestPointerCapture();
            if(status.getVisibility()==View.VISIBLE)status.setText(text);
            try {
                JSONObject metrics=new JSONObject();
                metrics.put("sequence",++feedbackSequence);
                metrics.put("statsVisible",status.getVisibility()==View.VISIBLE);
                metrics.put("fps",fps); metrics.put("rendered",presented); metrics.put("receivedFrames",r.received);
                metrics.put("renderer",textureSurface==null?"surface-view":"texture-view");
                metrics.put("codecRendered",r.rendered);
                if(textureSurface!=null){metrics.put("textureUpdates",textureUpdates);metrics.put("textureLastUpdateAgeMs",(now-lastTextureUpdateNs)/1000000.0);}
                metrics.put("submittedFrames",r.submittedFrames);metrics.put("decodedOutputs",r.decodedOutputs);metrics.put("releasedForRender",r.releasedForRender);metrics.put("arrivalTraceDrops",r.arrivalTraceDrops.get());
                metrics.put("receiveToRenderP50Ms",p50); metrics.put("receiveToRenderP95Ms",p95);
                metrics.put("lostPackets",r.assembler.lost); metrics.put("damagedFrames",r.assembler.damagedFrames);
                metrics.put("queueDrops",r.dropped); metrics.put("malformed",r.assembler.malformed);
                metrics.put("decoder",r.decoderName); metrics.put("format",r.format); metrics.put("status",r.message);
                metrics.put("videoColorStandard",r.colorStandard);metrics.put("videoColorRange",r.colorRange);metrics.put("videoColorTransfer",r.colorTransfer);
                metrics.put("receiveToDecodeP50Ms",r.decodeP50());
                metrics.put("sessionId",getIntent().getStringExtra("sessionId")==null?"experiment":getIntent().getStringExtra("sessionId"));
                if(r.needsIdr || r.assembler.needsKeyframe())r.requestKeyframe(metrics.getString("sessionId"),now);
                metrics.put("keyframeRequests",r.keyframeRequests);
                metrics.put("videoHealthy",r.running && !waiting && fps>0);
                metrics.put("audioEnabled",audio!=null);metrics.put("audioHealthy",sound);
                metrics.put("audioTimestampAgeMs",audio!=null?audio.timestampAgeMs:0);
                metrics.put("videoAudioWaitMs",r.audioWaitMs);
                metrics.put("audioVideoSyncEnabled",getIntent().getBooleanExtra("syncVideo",true));
                if(audio!=null){metrics.put("audioNetworkDrops",audio.networkDrops);metrics.put("audioClockMediaNs",audio.clockMediaNs);metrics.put("audioClockLocalNs",audio.clockLocalNs);metrics.put("audioLastMediaNs",audio.lastMediaNs);metrics.put("videoMediaNs",audio.videoMediaNs);metrics.put("observedClientNs",now);}
                if(audio!=null) {
                    metrics.put("audioBackend",audio.backend);metrics.put("audioOutputMode",audio.outputMode);
                    metrics.put("audioOutputDeviceId",audio.outputDeviceId);metrics.put("audioOutputSharingMode",audio.outputSharingMode);
                    metrics.put("audioPlayedFrames",audio.playedFrames);metrics.put("audioReceivedFrames",audio.receivedFrames);
                    metrics.put("audioBufferedMs",audio.bufferedMs);metrics.put("audioRms",audio.rms);
                    metrics.put("audioUnderruns",audio.underruns);metrics.put("audioResets",audio.resets);metrics.put("audioError",audio.error);
                    metrics.put("audioSourceUnderruns",audio.sourceUnderruns);metrics.put("audioSilenceFrames",audio.silenceFrames);
                    metrics.put("audioClockDrops",audio.clockDrops);metrics.put("audioQueueLimitFrames",audio.queueLimitFrames);
                    metrics.put("audioOutputBufferFrames",audio.outputBufferFrames);metrics.put("audioOutputBurstFrames",audio.outputBurstFrames);metrics.put("audioOutputCapacityFrames",audio.outputCapacityFrames);
                }
                metrics.put("videoLateDrops",r.lateDrops);
                metrics.put("videoSyncScheduledFrames",r.syncScheduled);metrics.put("videoSyncClockMisses",r.syncClockMisses);
                metrics.put("scope","first RTP packet to MediaCodec Surface presentation timestamp; excludes host, preceding network, HDMI and TV processing");
                JSONObject perf=new JSONObject(performance);
                if(audio!=null){perf.put("audioOutputLowLatency",audio.backend.equals("AAudio")?audio.outputMode==12:audio.outputMode==1);perf.put("audioOutputBufferMs",audio.outputBufferFrames/48.0);}
                metrics.put("performance",perf);
                metrics.put("callbackFps",callbackFps);
                metrics.put("lastPresentAgeMs",performance.get("lastPresentAgeMs"));
                metrics.put("telemetryPacketDrops",r.telemetryPacketDrops.get());
                metrics.put("traceDrops",r.traceDrops.get());
                String line=metrics.toString(); Log.i(TAG,"METRICS "+line);
                if (r.host!=null && r.telemetry!=null && !r.telemetry.isClosed()) {
                    byte[] payload=line.getBytes("UTF-8");
                    r.sendTelemetry(payload);
                    if(getIntent().getBooleanExtra("diagnostics",false)) {
                        for(int batch=0;batch<16;batch++) {
                            JSONArray rows=new JSONArray();for(int i=0;i<4;i++){FrameTiming f=r.traces.poll();if(f==null)break;rows.put(new JSONArray(f.row()));}
                            JSONArray arrivals=new JSONArray();for(int i=0;i<8;i++){long[] arrival=r.arrivalTraces.poll();if(arrival==null)break;arrivals.put(new JSONArray(arrival));}
                            JSONArray outputs=new JSONArray();for(int i=0;i<8;i++){long[] output=r.outputTraces.poll();if(output==null)break;outputs.put(new JSONArray(output));}
                            if(rows.length()==0 && arrivals.length()==0 && outputs.length()==0)break;
                            JSONObject trace=new JSONObject();trace.put("type","client_frames");trace.put("sessionId",metrics.getString("sessionId"));trace.put("rows",rows);trace.put("arrivals",arrivals);trace.put("outputs",outputs);
                            long[] sync=clockSync==null?null:clockSync.estimate.best(now);
                            if(sync!=null){trace.put("clockSampleClientNs",sync[0]);trace.put("hostMinusClientNs",sync[1]);trace.put("clockUncertaintyNs",sync[2]);}
                            r.sendTelemetry(trace.toString().getBytes("UTF-8"));
                        }
                    }
                }
            } catch (Exception e) { Log.w(TAG,"telemetry",e); }
        }
        ui.postDelayed(this,250);
    }};

    public void onCreate(Bundle saved) {
        super.onCreate(saved);
        sessionFormat=new SessionFormat(getIntent());
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_FULLSCREEN|View.SYSTEM_UI_FLAG_HIDE_NAVIGATION|View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY);
        Display.Mode currentMode=getWindowManager().getDefaultDisplay().getMode();
        // Even same-resolution refresh changes may renegotiate HDMI/audio.
        // Preflight validates output capacity; starting a session never forces a mode.
        Log.i(TAG,"DISPLAY_PRESERVED width="+currentMode.getPhysicalWidth()+" height="+currentMode.getPhysicalHeight()+" refreshHz="+currentMode.getRefreshRate());
        if(getIntent().getBooleanExtra("colorChart",false)) {
            setContentView(new ColorChartView(this));
            Log.i(TAG,"LOCAL_RGB_CONTROL no codec, network, audio, or periodic redraw");
            return;
        }
        FrameLayout root=new FrameLayout(this);
        root.setBackgroundColor(Color.BLACK);
        if(getIntent().getBooleanExtra("textureVideo",false)) {
            textureSurface=new TextureView(this);
            root.addView(textureSurface,new FrameLayout.LayoutParams(-1,-1));
            textureSurface.setSurfaceTextureListener(new TextureView.SurfaceTextureListener() {
                public void onSurfaceTextureAvailable(SurfaceTexture texture,int width,int height) {
                    textureOutput=new Surface(texture);startReceiver(textureOutput);
                }
                public void onSurfaceTextureSizeChanged(SurfaceTexture texture,int width,int height) {}
                public void onSurfaceTextureUpdated(SurfaceTexture texture) {textureUpdates++;lastTextureUpdateNs=System.nanoTime();}
                public boolean onSurfaceTextureDestroyed(SurfaceTexture texture) {
                    stopReceiver();if(textureOutput!=null){textureOutput.release();textureOutput=null;}return true;
                }
            });
        } else {
            SurfaceView surface=new SurfaceView(this);videoSurface=surface;
            root.addView(surface,new FrameLayout.LayoutParams(-1,-1));
            surface.getHolder().addCallback(this);
        }
        status=new TextView(this); status.setTextColor(Color.WHITE); status.setTextSize(17);
        status.setBackgroundColor(0xB0000000); status.setPadding(24,16,24,16);
        status.setText("sofaLink Stream Lab\n等待电脑发送 "+sessionFormat.label()+" 视频…");
        FrameLayout.LayoutParams overlay=new FrameLayout.LayoutParams(-2,-2,Gravity.TOP|Gravity.LEFT);
        overlay.setMargins(32,32,0,0); root.addView(status,overlay);
        setContentView(root);
        inputView=root;root.setFocusableInTouchMode(true);root.requestFocus();
        root.setOnCapturedPointerListener((view,event)-> {
            if(input==null)return false;
            input.mouse(Math.round(event.getX()),Math.round(event.getY()),event.getButtonState(),Math.round(event.getAxisValue(MotionEvent.AXIS_VSCROLL)*120));
            return true;
        });
    }
    public void surfaceCreated(SurfaceHolder holder) {
        startReceiver(holder.getSurface());
    }
    private void startReceiver(Surface surface) {
        if(receiver!=null)return;
        textureUpdates=0;lastTextureUpdateNs=0;
        receiver=new Receiver(surface,getIntent().getIntExtra("port",5004),getIntent().getStringExtra("host"),getIntent().getStringExtra("transport"));
        if(getIntent().getBooleanExtra("audio",true)){audio=new PcmPlayer(getIntent().getStringExtra("host"),getIntent().getStringExtra("audioBackend"),this,sessionFormat.sampleRate,sessionFormat.requireLowLatencyAudio);audio.start();}
        int inputPort=getIntent().getIntExtra("inputPort",0);
        if(inputPort>0 && getIntent().getStringExtra("inputKey")!=null)input=new InputSender(getIntent().getStringExtra("host"),inputPort,getIntent().getStringExtra("sessionId"),getIntent().getStringExtra("inputKey"));
        if(getIntent().getBooleanExtra("diagnostics",false))clockSync=new ClockSync(getIntent().getStringExtra("host"),getIntent().getStringExtra("sessionId"));
        receiver.start(); android.view.Choreographer.getInstance().postFrameCallback(presentationVsync); renderProgress.clear(); ui.post(tick);
    }
    public void surfaceChanged(SurfaceHolder h,int format,int width,int height) {}
    public void surfaceDestroyed(SurfaceHolder h) { stopReceiver(); }
    public void onStop() { android.view.Choreographer.getInstance().removeFrameCallback(presentationVsync); stopReceiver(); super.onStop(); }
    public void onResume() {
        super.onResume();
        if(receiver==null && videoSurface!=null && videoSurface.getHolder().getSurface().isValid()) surfaceCreated(videoSurface.getHolder());
        if(receiver==null && textureSurface!=null && textureSurface.isAvailable() && textureOutput!=null) startReceiver(textureOutput);
    }
    private void stopReceiver() {android.view.Choreographer.getInstance().removeFrameCallback(presentationVsync);ui.removeCallbacks(tick);if(clockSync!=null){clockSync.close();clockSync=null;}if(input!=null){input.close();input=null;}if(inputView!=null)inputView.releasePointerCapture();if(receiver!=null){receiver.close();receiver=null;}if(audio!=null){audio.close();audio=null;}}
    public boolean onKeyDown(int code,KeyEvent event) {
        if(code==KeyEvent.KEYCODE_BACK && input!=null && event.getRepeatCount()>0){input.release();finish();return true;}
        if(code==KeyEvent.KEYCODE_MENU) {
            if(status!=null && event.getRepeatCount()==0)status.setVisibility(status.getVisibility()==View.VISIBLE?View.GONE:View.VISIBLE); return true;
        }
        if(input!=null && input.key(event,true))return true;
        return super.onKeyDown(code,event);
    }
    public boolean onKeyUp(int code,KeyEvent event) {
        if(code==KeyEvent.KEYCODE_MENU)return true;
        if(input!=null && input.key(event,false))return true;
        return super.onKeyUp(code,event);
    }
    public void onWindowFocusChanged(boolean focus) {
        super.onWindowFocusChanged(focus);
        if(!focus){if(input!=null)input.release();pointerKnown=false;if(inputView!=null)inputView.releasePointerCapture();}
    }
    public boolean dispatchGenericMotionEvent(MotionEvent event) {
        if(input!=null && (event.getSource()&InputDevice.SOURCE_MOUSE)==InputDevice.SOURCE_MOUSE && (inputView==null || !inputView.hasPointerCapture())) {
            float x=event.getX(),y=event.getY();
            input.mouse(pointerKnown?Math.round(x-pointerX):0,pointerKnown?Math.round(y-pointerY):0,event.getButtonState(),Math.round(event.getAxisValue(MotionEvent.AXIS_VSCROLL)*120));
            pointerX=x;pointerY=y;pointerKnown=true;return true;
        }
        return super.dispatchGenericMotionEvent(event);
    }
    public boolean dispatchTouchEvent(MotionEvent event) {
        if(input!=null && (event.getSource()&InputDevice.SOURCE_MOUSE)==InputDevice.SOURCE_MOUSE && (inputView==null || !inputView.hasPointerCapture())) {
            float x=event.getX(),y=event.getY();int action=event.getActionMasked();
            int buttons=event.getButtonState();if(action==MotionEvent.ACTION_DOWN && buttons==0)buttons=1;if(action==MotionEvent.ACTION_UP || action==MotionEvent.ACTION_CANCEL)buttons=0;
            input.mouse(pointerKnown?Math.round(x-pointerX):0,pointerKnown?Math.round(y-pointerY):0,buttons,0);
            pointerX=x;pointerY=y;pointerKnown=true;return true;
        }
        return super.dispatchTouchEvent(event);
    }

    private static final class Frame {
        final byte[] data; final long receivedNs,mediaTimestamp; final boolean idr;
        boolean resetDecoder;
        final FrameTiming timing;
        Frame(byte[] data,long receivedNs,boolean idr,long mediaTimestamp) {this.data=data;this.receivedNs=receivedNs;this.idr=idr;this.mediaTimestamp=mediaTimestamp;this.timing=new FrameTiming(mediaTimestamp,receivedNs,System.nanoTime());}
    }
    private final class Receiver {
        final Surface surface; final int port; final String expectedHost,transport;
        final CountDownLatch decoderReady=new CountDownLatch(1);
        final ArrayBlockingQueue<Frame> queue=new ArrayBlockingQueue<>(Math.max(4,Math.min(16,getIntent().getIntExtra("videoQueueFrames",4))));
        final RtpAssembler assembler;
        final Map<Long,Long> inFlight=new ConcurrentHashMap<>();
        final Map<Long,Long> mediaTimes=new ConcurrentHashMap<>();
        final Map<Long,FrameTiming> frameTimings=new ConcurrentHashMap<>();
        final FrameTiming.Window timings=new FrameTiming.Window();
        final PresentationClock presentationClock=new PresentationClock(getWindowManager().getDefaultDisplay().getMode().getRefreshRate());
        volatile double presentationRefreshHz=getWindowManager().getDefaultDisplay().getMode().getRefreshRate();
        volatile long appVsyncOffsetNs=getWindowManager().getDefaultDisplay().getAppVsyncOffsetNanos();
        final boolean alignPresentation=!"timed".equals(getIntent().getStringExtra("presentationMode"));
        final ArrayBlockingQueue<FrameTiming> traces=new ArrayBlockingQueue<>(240);
        final ArrayBlockingQueue<long[]> arrivalTraces=new ArrayBlockingQueue<>(240);
        final ArrayBlockingQueue<long[]> outputTraces=new ArrayBlockingQueue<>(240);
        final java.util.concurrent.atomic.AtomicLong arrivalTraceDrops=new java.util.concurrent.atomic.AtomicLong();
        final java.util.concurrent.atomic.AtomicLong traceDrops=new java.util.concurrent.atomic.AtomicLong();
        void trace(FrameTiming f,int outcome) {
            f.outcome=outcome;
            if(outcome==0)timings.add(f);
            if(getIntent().getBooleanExtra("diagnostics",false) && !traces.offer(f))traceDrops.incrementAndGet();
        }
        volatile double audioWaitMs;
        final List<Double> latencies=Collections.synchronizedList(new ArrayList<>());
        final List<Double> decodeTimes=Collections.synchronizedList(new ArrayList<>());
        double decodeP50() { synchronized(decodeTimes) {
            if(decodeTimes.isEmpty())return 0;
            Collections.sort(decodeTimes);double value=decodeTimes.get(decodeTimes.size()/2);decodeTimes.clear();return value;
        } }
        volatile boolean running=true, needsIdr=false;
        long previousLost;
        long lastKeyframeRequestNs,keyframeRequests;
        volatile long rendered,received,dropped,lateDrops,submittedFrames,decodedOutputs,releasedForRender;
        volatile long syncScheduled,syncClockMisses;
        volatile long lastPacketNs;
        volatile String message="等待视频…",format=sessionFormat.label(),decoderName="";
        volatile int colorStandard=-1,colorRange=-1,colorTransfer=-1;
        volatile InetAddress host;
        DatagramSocket socket,telemetry; Thread network,decode;
        Socket tcp;
        final java.util.concurrent.atomic.AtomicLong telemetryPacketDrops=new java.util.concurrent.atomic.AtomicLong();
        final ExecutorService feedbackWorker=new java.util.concurrent.ThreadPoolExecutor(1,1,0,TimeUnit.MILLISECONDS,new ArrayBlockingQueue<Runnable>(64),(task,worker)->{telemetryPacketDrops.incrementAndGet();});
        void checkLoss() {if(assembler.lost!=previousLost) {needsIdr=true;previousLost=assembler.lost;}}
        void requestKeyframe(String session,long now) {
            if(!running || host==null || telemetry==null || assembler.packets==0 || now-lastKeyframeRequestNs<500000000L)return;
            try {
                JSONObject request=new JSONObject();request.put("type","keyframe_request");request.put("sessionId",session);
                // Device monotonic microseconds survive Activity restarts within a host session.
                request.put("sequence",now/1000);keyframeRequests++;lastKeyframeRequestNs=now;
                sendTelemetry(request.toString().getBytes("UTF-8"));
            }catch(Exception e){Log.w(TAG,"keyframe request",e);}
        }
        void sendTelemetry(byte[] payload) {
            if(!running)return;
            feedbackWorker.execute(()-> {try {
                if(telemetry!=null && !telemetry.isClosed() && host!=null)
                    telemetry.send(new DatagramPacket(payload,payload.length,host,5005));
            }catch(Exception e){if(running)Log.w(TAG,"telemetry",e);}});
        }
        Receiver(Surface surface,int port,String expectedHost,String transport) {
            this.surface=surface;this.port=port;this.expectedHost=expectedHost;this.transport=transport==null?"tcp":transport;
            assembler=new RtpAssembler((data,timestamp,first,idr)-> {
                received++;
                timings.arrival(first);
                checkLoss();
                Frame f=new Frame(data,first,idr,timestamp);f.timing.queueDepth=queue.size();
                if(getIntent().getBooleanExtra("diagnostics",false) && !arrivalTraces.offer(new long[]{timestamp,first,f.timing.assembledNs}))arrivalTraceDrops.incrementAndGet();
                if(needsIdr && !idr) {dropped++;trace(f.timing,1);return;}
                if(idr && needsIdr) {
                    Frame discarded;while((discarded=queue.poll())!=null)trace(discarded.timing,4);
                    f.resetDecoder=true;needsIdr=false;
                }
                if(!queue.offer(f)) {
                    dropped+=queue.size()+1;Frame discarded;while((discarded=queue.poll())!=null)trace(discarded.timing,2);needsIdr=true;
                    // Reset once AT the fresh IDR, not on every dropped P frame.
                    // Otherwise old scheduled outputs can perpetuate decode backpressure.
                    if(idr) {f.resetDecoder=true;needsIdr=false;queue.offer(f);}else trace(f.timing,2);
                }
            });
        }
        void start() {
            network=new Thread(()->receive(),"rtp-receive"); decode=new Thread(()->decode(),"hardware-decode");
            decode.start();network.start();
        }
        void receive() {
            Process.setThreadPriority(Process.THREAD_PRIORITY_DISPLAY);
            try {
                if(!decoderReady.await(3,TimeUnit.SECONDS))throw new IllegalStateException("Decoder startup timeout");
                if(!running)return;
            }catch(Exception e){if(running)fail("解码器未就绪",e);return;}
            if(transport.equals("tcp")) {receiveTcp();return;}
            try {
                InetAddress allowed=expectedHost==null?null:InetAddress.getByName(expectedHost);
                socket=new DatagramSocket(null);socket.setReuseAddress(true);socket.setReceiveBufferSize(512*1024);
                socket.bind(new InetSocketAddress(port));socket.setSoTimeout(3);
                telemetry=new DatagramSocket();
                byte[] buffer=new byte[65536];DatagramPacket packet=new DatagramPacket(buffer,buffer.length);
                Log.i(TAG,"LISTENING UDP "+port+" allowed="+expectedHost);
                while(running) {
                    try {
                        packet.setLength(buffer.length);socket.receive(packet);
                        if(allowed!=null && !packet.getAddress().equals(allowed)) continue;
                        if(host==null)host=packet.getAddress();
                        if(!packet.getAddress().equals(host))continue;
                        lastPacketNs=System.nanoTime();assembler.accept(buffer,packet.getLength(),lastPacketNs);
                    } catch(SocketTimeoutException ignored) {assembler.expire(System.nanoTime());}
                }
            } catch(Exception e) {if(running)fail("接收失败",e);}
            finally {if(socket!=null)socket.close();if(telemetry!=null)telemetry.close();}
        }
        void receiveTcp() {
            try {
                if(expectedHost==null)throw new IllegalArgumentException("Host IP required for TCP");
                host=InetAddress.getByName(expectedHost);telemetry=new DatagramSocket();
                while(running) {
                    try {
                        tcp=new Socket();tcp.setTcpNoDelay(true);tcp.setReceiveBufferSize(64*1024);
                        tcp.connect(new InetSocketAddress(host,port),1000);break;
                    } catch(Exception e) {
                        try{tcp.close();}catch(Exception ignored){}
                        if(!running)return;Thread.sleep(250);
                    }
                }
                Log.i(TAG,"CONNECTED TCP "+expectedHost+":"+port);
                DataInputStream input=new DataInputStream(tcp.getInputStream());byte[] buffer=new byte[65536];
                while(running) {
                    int length=input.readUnsignedShort();
                    if(length<12)throw new IllegalArgumentException("Invalid RTP stream packet length");
                    input.readFully(buffer,0,length);lastPacketNs=System.nanoTime();assembler.accept(buffer,length,lastPacketNs);
                }
            } catch(Exception e) {if(running)fail("连接已结束",e);}
            finally {try{if(tcp!=null)tcp.close();}catch(Exception ignored){}if(telemetry!=null)telemetry.close();}
        }
        void decode() {
            Process.setThreadPriority(Process.THREAD_PRIORITY_DISPLAY);
            MediaCodec codec=null;
            try {
                decoderName=AndroidDecoderAdapter.select(sessionFormat,getIntent().getStringExtra("decoder"));
                codec=MediaCodec.createByCodecName(decoderName);
                MediaFormat video=AndroidDecoderAdapter.format(sessionFormat,decoderName,getIntent().getStringExtra("deviceMode"));
                codec.configure(video,surface,null,0);
                codec.setOnFrameRenderedListener((mc,pts,nano)-> {
                    Long first=inFlight.remove(pts);
                    if(first!=null)latencies.add((nano-first)/1e6);
                    rendered++;
                    FrameTiming f=frameTimings.remove(pts);
                    if(f!=null){f.presentedNs=nano;f.callbackNs=System.nanoTime();
                        trace(f,0);}
                },ui);
                codec.start();message="接收中 · "+decoderName;
                decoderReady.countDown();
                Log.i(TAG,"DECODER "+decoderName+" format="+video);
                MediaCodec.BufferInfo info=new MediaCodec.BufferInfo();
                Frame pending=null;
                while(running) {
                    int output;
                    while((output=codec.dequeueOutputBuffer(info,0))>=0) {
                        decodedOutputs++;
                        Long first=inFlight.get(info.presentationTimeUs);
                        if(first!=null)decodeTimes.add((System.nanoTime()-first)/1e6);
                        Long media=mediaTimes.remove(info.presentationTimeUs);
                        FrameTiming frame=frameTimings.get(info.presentationTimeUs);
                        long decoded=System.nanoTime();if(frame!=null){frame.decodedNs=decoded;frame.inFlightDepth=inFlight.size();frame.audioBufferMs=audio==null?0:audio.bufferedMs;}
                        boolean sync=getIntent().getBooleanExtra("syncVideo",true);
                        long target=sync && audio!=null && media!=null?audio.videoTargetNs(media):0;
                        audioWaitMs=target==0?0:(target-System.nanoTime())/1000000.0;
                        long now=System.nanoTime();
                        if(target>0 && target<now-100000000L) {codec.releaseOutputBuffer(output,false);inFlight.remove(info.presentationTimeUs);frameTimings.remove(info.presentationTimeUs);if(frame!=null)trace(frame,3);lateDrops++;}
                        else if(target>0 && target<now+500000000L){
                            if(frame!=null)frame.targetNs=target;
                            long requested=presentationClock.request(target,now,alignPresentation);
                            if(frame!=null)frame.surfaceRequestNs=requested;
                            codec.releaseOutputBuffer(output,requested);
                            releasedForRender++;syncScheduled++;
                        }
                        else {if(sync)syncClockMisses++;if(frame!=null)frame.targetNs=now;codec.releaseOutputBuffer(output,true);releasedForRender++;}
                        if(getIntent().getBooleanExtra("diagnostics",false) && frame!=null && !outputTraces.offer(new long[]{frame.rtp,frame.submittedNs,frame.decodedNs,frame.targetNs,System.nanoTime(),frame.outcome==3?0:1}))traceDrops.incrementAndGet();
                    }
                    if(output==MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                        MediaFormat actual=codec.getOutputFormat();
                        colorStandard=actual.containsKey(MediaFormat.KEY_COLOR_STANDARD)?actual.getInteger(MediaFormat.KEY_COLOR_STANDARD):-1;
                        colorRange=actual.containsKey(MediaFormat.KEY_COLOR_RANGE)?actual.getInteger(MediaFormat.KEY_COLOR_RANGE):-1;
                        colorTransfer=actual.containsKey(MediaFormat.KEY_COLOR_TRANSFER)?actual.getInteger(MediaFormat.KEY_COLOR_TRANSFER):-1;
                        format=actual.getInteger(MediaFormat.KEY_WIDTH)+"×"+actual.getInteger(MediaFormat.KEY_HEIGHT)+" · H.264";
                        Log.i(TAG,"OUTPUT_FORMAT "+actual);
                    }
                    if(pending==null)pending=queue.poll();
                    if(pending!=null) {
                        if(pending.resetDecoder) {
                            pending.resetDecoder=false;codec.flush();presentationClock.reset();inFlight.clear();mediaTimes.clear();
                            for(FrameTiming f:frameTimings.values())trace(f,4);frameTimings.clear();
                        }
                        int input=codec.dequeueInputBuffer(1000);
                        if(input>=0) {
                            ByteBuffer data=codec.getInputBuffer(input);data.clear();data.put(pending.data);
                            long pts=pending.receivedNs/1000;
                            inFlight.put(pts,pending.receivedNs);
                            mediaTimes.put(pts,pending.mediaTimestamp);
                            pending.timing.submittedNs=System.nanoTime();frameTimings.put(pts,pending.timing);
                            if(inFlight.size()>240) {
                                long oldest=Collections.min(inFlight.keySet());inFlight.remove(oldest);mediaTimes.remove(oldest);FrameTiming expired=frameTimings.remove(oldest);if(expired!=null)trace(expired,5);
                            }
                            codec.queueInputBuffer(input,0,pending.data.length,pts,0);submittedFrames++;pending=null;
                        }
                    } else Thread.sleep(1);
                }
            } catch(Exception e) {if(running)fail("解码失败",e);}
            finally {if(codec!=null){try{codec.stop();}catch(Exception ignored){}codec.release();}}
        }
        void fail(String label,Exception e) {
            message=label+": "+e.getMessage();Log.e(TAG,label,e);running=false;if(socket!=null)socket.close();
            try{if(tcp!=null)tcp.close();}catch(Exception ignored){}
        }
        void close() {
            running=false;if(socket!=null)socket.close();
            try{if(tcp!=null)tcp.close();}catch(Exception ignored){}
            feedbackWorker.shutdownNow();
            try{if(network!=null)network.join(1000);if(decode!=null)decode.join(1000);}catch(InterruptedException e){Thread.currentThread().interrupt();}
        }
    }
}
