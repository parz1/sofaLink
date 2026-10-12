package dev.livingroom.streamlab;
import java.util.ArrayList;
import java.util.List;

public final class RtpAssemblerTest {
    static byte[] packet(int seq,int timestamp,int source,boolean marker,int... payload) {
        byte[] p=new byte[12+payload.length];p[0]=(byte)128;p[1]=(byte)(96|(marker?128:0));
        p[2]=(byte)(seq>>8);p[3]=(byte)seq;
        for(int i=0;i<4;i++){p[4+i]=(byte)(timestamp>>(24-8*i));p[8+i]=(byte)(source>>(24-8*i));}
        for(int i=0;i<payload.length;i++)p[12+i]=(byte)payload[i];return p;
    }
    static void send(RtpAssembler a,byte[] p){a.accept(p,p.length,System.nanoTime());}
    static void check(boolean b,String message){if(!b)throw new AssertionError(message);}
    public static void main(String[] args) {
        List<byte[]> frames=new ArrayList<>();
        RtpAssembler a=new RtpAssembler((data,ts,ns,idr)->frames.add(data));
        check(a.needsKeyframe(),"startup requires a reference reset");
        send(a,packet(65535,1,1,true,0x65,11));
        check(frames.size()==1 && frames.get(0).length==6,"single IDR / Annex B");
        check(!a.needsKeyframe(),"valid IDR clears recovery demand");
        send(a,packet(0,2,1,false,0x7c,0x85,12));
        send(a,packet(1,2,1,true,0x7c,0x45,13));
        check(frames.size()==2 && frames.get(1)[4]==0x65 && frames.get(1).length==7,"FU-A / sequence wrap");
        send(a,packet(3,3,1,true,0x41,1));
        send(a,packet(4,4,1,true,0x41,1));
        a.expire(System.nanoTime()+3000000L);
        check(frames.size()==2 && a.lost==1,"loss suppresses predicted frames");
        check(a.needsKeyframe(),"loss exposes recovery demand without waiting for a delivered frame");
        send(a,packet(5,5,1,true,0x65,1));
        check(frames.size()==3,"IDR recovers after loss");
        check(!a.needsKeyframe(),"recovered IDR clears demand");
        send(a,packet(0,1,2,true,0x65,2));
        check(frames.size()==4 && a.lost==1,"SSRC restart resets sequence without artificial loss");
        send(a,packet(1,2,2,true,24,0,2,0x67,1,0,2,0x65,2));
        check(frames.size()==5 && frames.get(4).length==12,"STAP-A");
        send(a,packet(2,3,2,true,24,0,20,0x65));
        check(frames.size()==5 && a.damagedFrames>0,"invalid STAP-A discarded");
        send(a,packet(3,4,2,true,0x7c,0x45,1));
        check(frames.size()==5,"FU-A end without start discarded");
        a.accept(new byte[2],2,0);
        check(a.malformed==1,"short packet rejected");
        int before=frames.size();long now=System.nanoTime();
        byte[] head=packet(4,5,2,false,0x7c,0x85,10);
        byte[] tail=packet(5,5,2,true,0x7c,0x45,11);
        a.accept(tail,tail.length,now);a.accept(head,head.length,now+500000);
        check(frames.size()==before+1,"bounded packet reordering recovers FU-A");
        System.out.println("PASS: single NAL, FU-A, STAP-A, sequence wrap, loss/IDR recovery, SSRC restart, malformed packets, bounded reordering");
    }
}
