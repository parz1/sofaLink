package dev.livingroom.streamlab;
import java.util.ArrayDeque;
/** Four-timestamp offset estimate with a measured uncertainty interval; never subtract raw clocks. */
final class ClockEstimate {
    private final ArrayDeque<long[]> samples=new ArrayDeque<>();
    synchronized boolean add(long c1,long h2,long h3,long c4) {
        if(c1<=0 || h2<=0 || h3<h2 || c4<c1)return false;
        long rtt=(c4-c1)-(h3-h2);if(rtt<0 || rtt>1000000000L)return false;
        samples.addLast(new long[]{c4,((h2-c1)+(h3-c4))/2,rtt/2});while(samples.size()>20)samples.removeFirst();return true;
    }
    synchronized long[] best(long now) {
        long[] best=null;for(long[] s:samples)if(now>=s[0] && now-s[0]<=5000000000L && (best==null || s[2]<best[2]))best=s;
        return best==null?null:best.clone();
    }
}
