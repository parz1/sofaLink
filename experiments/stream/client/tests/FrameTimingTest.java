package dev.livingroom.streamlab;
import java.util.Map;
public final class FrameTimingTest {
    static void check(boolean ok,String message){if(!ok)throw new AssertionError(message);}
    public static void main(String[] args){
        ClockEstimate clock=new ClockEstimate();
        check(clock.add(1000000,11000100,11000200,1000300),"valid clock");
        long[] best=clock.best(1000400);
        check(best[1]==10000000 && best[2]==100,"offset and half RTT");
        best[1]=0;check(clock.best(1000400)[1]==10000000,"immutable sample");
        check(clock.best(6001000400L)==null,"clock expiry");
        check(!clock.add(100,200,500,200),"negative RTT rejected");
        FrameTiming.Window w=new FrameTiming.Window();
        for(int i=0;i<130;i++){
            long start=1000000000L+i*16666667L;
            FrameTiming f=new FrameTiming(i,start,start+1000000);
            f.submittedNs=start+3000000;f.decodedNs=start+8000000;f.targetNs=start+48000000;f.presentedNs=start+49000000;f.callbackNs=4000000000L;
            w.add(f);w.arrival(start);
        }
        Map<String,Double> m=w.summary(4000000000L);
        check(m.get("frames")==120,"bounded window");
        check(Math.abs(m.get("renderGapP95Ms")-16.666667)<.00001,"presentation gaps ignore batched callback");
        check(Math.abs(m.get("renderFps")-60)<.001,"fps uses presentation clock");
        check(m.get("queueP50Ms")==2 && m.get("decodeP50Ms")==5 && m.get("syncWaitP50Ms")==40,"stage decomposition");
        check(w.summary(8000000000L).get("frames")==0,"expired window");
        System.out.println("Frame timing / clock checks PASS");
    }
}
