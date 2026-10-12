package dev.livingroom.streamlab;
public final class PresentationClockTest {
    static void check(boolean value,String reason){if(!value)throw new AssertionError(reason);}
    public static void main(String[] args) {
        PresentationClock clock=new PresentationClock(60);
        long now=1000000000L;
        check(clock.request(now+100000000,now,true)==now+100000000,"uncalibrated preserves audio target");
        clock.vsync(now,2000000,now);
        long audio=now+70000000;
        check(Math.abs(clock.request(audio,now,true)-audio)<10000000,"nearest phase bounded by half refresh plus margin");
        check(clock.request(audio,now,false)==audio,"baseline exact");
        check(clock.request(now+1000000,now,true)==now,"never asks to render before decoding");
        check(clock.request(audio,now+2000000000L,true)==audio,"stale fallback");
        clock.setRefreshRate(120);check(clock.request(audio,now,true)==audio,"display change invalidates phase");
        clock.vsync(now,2000000,now);clock.reset();check(clock.request(audio,now,true)==audio,"flush invalidates phase");
        for(double hz:new double[]{30,59.94,60,120,144}) {
            PresentationClock varied=new PresentationClock(hz);
            long period=Math.round(1e9/hz);
            varied.vsync(now,2000000,now);
            for(int i=1;i<=200;i++) {
                long target=now+i*period/100;
                long request=varied.request(target,now,true);
                check(request>=now && Math.abs(request-target)<=period*.551,"bounded scheduling across refresh rates");
            }
        }
        PresentationClock invalid=new PresentationClock(Double.NaN);
        invalid.vsync(now,0,now);check(invalid.request(audio,now,true)==audio,"unknown display fallback");
        PresentationClock invalidPhase=new PresentationClock(60);
        invalidPhase.vsync(now,20000000,now);check(invalidPhase.request(audio,now,true)==audio,"invalid app offset fallback");
        invalidPhase.vsync(now,0,now+100000000);check(invalidPhase.request(audio,now+100000000,true)==audio,"delayed observation fallback");
        System.out.println("Presentation VSYNC / audio authority / bounded phase checks PASS");
    }
}
