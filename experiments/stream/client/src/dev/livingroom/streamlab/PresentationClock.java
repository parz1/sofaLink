package dev.livingroom.streamlab;

/** Public VSYNC phase scheduling. Codec render notifications are not a display clock. */
final class PresentationClock {
    private long periodNs,phaseNs,lastObservedNs;
    PresentationClock(double refreshHz){setRefreshRate(refreshHz);}
    synchronized void setRefreshRate(double hz){periodNs=Double.isFinite(hz) && hz>=1 && hz<=240?Math.round(1000000000.0/hz):0;reset();}
    synchronized void reset(){phaseNs=0;lastObservedNs=0;}
    synchronized void vsync(long frameTime,long appOffset,long observed) {
        if(periodNs==0 || frameTime<=0 || appOffset<0 || appOffset>=periodNs || observed<frameTime || observed-frameTime>periodNs*2)return;
        phaseNs=frameTime-appOffset;lastObservedNs=observed;
    }
    synchronized long request(long audioTarget,long now,boolean align) {
        if(!align || periodNs==0 || lastObservedNs==0 || now<lastObservedNs || now-lastObservedNs>periodNs*4)return audioTarget;
        long slot=phaseNs+Math.round((audioTarget-phaseNs)/(double)periodNs)*periodNs;
        // Small phase-derived margin avoids rounding a fence timestamp into the next slot.
        // The true audio target is never modified; phase rounding is bounded by
        // half a refresh plus the 5% margin. Already-late output cannot go backwards.
        return Math.max(now,slot-periodNs/20);
    }
}
