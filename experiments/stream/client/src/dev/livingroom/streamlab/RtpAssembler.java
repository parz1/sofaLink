package dev.livingroom.streamlab;

import java.io.ByteArrayOutputStream;
import java.util.ArrayList;
import java.util.Arrays;

/** RFC 6184. At most 8 out-of-order packets / 2 ms; loss discards AU and waits for IDR. */
public final class RtpAssembler {
    public interface Sink { void frame(byte[] annexB, long timestamp, long firstPacketNs, boolean idr); }
    private final Sink sink;
    private final ByteArrayOutputStream au = new ByteArrayOutputStream(262144);
    private int expected = -1;
    private long timestamp = -1, firstNs, ssrc = -1;
    private boolean bad, idr, fragmented;
    private volatile boolean waitingIdr = true;
    public boolean needsKeyframe() { return waitingIdr; }
    public long packets, lost, damagedFrames, malformed;
    private static final int MAX_AU = 2 * 1024 * 1024;
    private static final class Pending {
        final byte[] packet; final long time; final int seq;
        Pending(byte[] packet,long time,int seq){this.packet=packet;this.time=time;this.seq=seq;}
    }
    private final ArrayList<Pending> pending=new ArrayList<>();
    public RtpAssembler(Sink sink) { this.sink = sink; }
    private void prefix() { au.write(0); au.write(0); au.write(0); au.write(1); }
    private int u16(byte[] b, int p) { return ((b[p]&255)<<8)|(b[p+1]&255); }
    public void accept(byte[] b, int n, long now) {
        if(n<12) {process(b,n,now);return;}
        long source=((long)(b[8]&255)<<24)|((long)(b[9]&255)<<16)|((b[10]&255)<<8)|(b[11]&255);
        if(source!=ssrc) {pending.clear();process(b,n,now);return;}
        expire(now);
        int seq=u16(b,2),distance=(seq-expected)&65535;
        if(expected>=0 && distance>0 && distance<=8 && pending.size()<8) {
            for(Pending item:pending)if(item.seq==seq)return;
            pending.add(new Pending(Arrays.copyOf(b,n),now,seq));return;
        }
        process(b,n,now);drain();
    }
    public void expire(long now) {
        if(pending.isEmpty())return;
        long oldest=Long.MAX_VALUE;for(Pending item:pending)oldest=Math.min(oldest,item.time);
        if(now-oldest<2000000L)return;
        Pending next=null;
        for(Pending item:pending)if(next==null || ((item.seq-expected)&65535)<((next.seq-expected)&65535))next=item;
        pending.remove(next);process(next.packet,next.packet.length,next.time);drain();
    }
    private void drain() {
        boolean found;
        do {
            found=false;
            for(int i=0;i<pending.size();i++)if(pending.get(i).seq==expected) {
                Pending next=pending.remove(i);process(next.packet,next.packet.length,next.time);found=true;break;
            }
        } while(found);
    }
    private void process(byte[] b,int n,long now) {
        if (n < 12 || (b[0]&192) != 128 || (b[1]&127) != 96) { malformed++; return; }
        int p = 12 + (b[0]&15)*4;
        if (p > n) { malformed++; return; }
        if ((b[0]&16) != 0) {
            if (p+4 > n) { malformed++; return; }
            p += 4 + u16(b,p+2)*4;
        }
        int end = n;
        if ((b[0]&32) != 0) end -= b[n-1]&255;
        if (p >= end || end > n) { malformed++; return; }
        int sequence = u16(b,2);
        long ts = ((long)(b[4]&255)<<24)|((long)(b[5]&255)<<16)|((b[6]&255)<<8)|(b[7]&255);
        long source = ((long)(b[8]&255)<<24)|((long)(b[9]&255)<<16)|((b[10]&255)<<8)|(b[11]&255);
        if(source!=ssrc) {
            ssrc=source; expected=-1; timestamp=-1; au.reset(); waitingIdr=true; fragmented=false;
        }
        packets++;
        boolean gap = expected >= 0 && expected != sequence;
        if (gap) {
            int distance = (sequence-expected)&65535;
            if (distance > 32768) return; // late/duplicate packet, never rewind sequence
            lost += distance;
            waitingIdr = true;
        }
        expected = (sequence+1)&65535;
        if (ts != timestamp) {
            if (au.size() > 0) { damagedFrames++; waitingIdr = true; }
            au.reset(); timestamp=ts; firstNs=now; bad=false; idr=false; fragmented=false;
        }
        if (gap) bad=true;
        firstNs=Math.min(firstNs,now);
        int type = b[p]&31;
        if (type >= 1 && type <= 23) {
            if (fragmented) bad=true;
            prefix(); au.write(b,p,end-p); idr |= type==5;
        } else if (type==24) {
            p++;
            while (p+2 <= end) {
                int size=u16(b,p); p+=2;
                if (size==0 || p+size>end) { bad=true; break; }
                prefix(); au.write(b,p,size); idr |= (b[p]&31)==5; p+=size;
            }
            if (p!=end) bad=true;
        } else if (type==28 && p+2 < end) {
            int header=b[p+1]&255;
            if ((header&128)!=0) {
                if (fragmented) bad=true;
                prefix(); au.write((b[p]&224)|(header&31)); fragmented=true; idr |= (header&31)==5;
            } else if (!fragmented) bad=true;
            au.write(b,p+2,end-p-2);
            if ((header&64)!=0) fragmented=false;
        } else bad=true;
        if (au.size()>MAX_AU) { bad=true; au.reset(); }
        if ((b[1]&128)!=0) {
            if (bad || fragmented || au.size()==0) { damagedFrames++; waitingIdr=true; }
            else if (!waitingIdr || idr) {
                waitingIdr=false;
                sink.frame(au.toByteArray(),timestamp,firstNs,idr);
            }
            au.reset(); fragmented=false;
        }
    }
}
