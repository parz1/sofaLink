package dev.livingroom.streamlab;
public final class AudioClockTest {
    static void check(boolean ok,String message){if(!ok)throw new AssertionError(message);}
    public static void main(String[] args){
        AudioClock c=new AudioClock();
        check(c.target(90000,1000000000L)==0,"uninitialized");
        check(c.update(1000000000L,2000000000L,2010000000L,0),"first mapping");
        check(c.target(90900,2020000000L)==2010000000L,"media delta mapped to actual device time");
        check(!c.update(900000000L,2010000000L,2020000000L,0),"backwards media rejected");
        check(!c.update(1010000000L,1990000000L,2020000000L,0),"backwards timestamp rejected");
        check(c.target(90000,2510000001L)==0,"device timestamp expires even if observed recently");
        c.reset();check(c.target(90000,2020000000L)==0,"reset removes old anchor");
        long wrap=(1L<<32)*1000000000L/90000;
        check(c.update(wrap+1000000000L,3000000000L,3010000000L,0),"reanchor after device silence");
        check(c.target(90000,3020000000L)==3000000000L,"RTP wrap");
        check(!c.update(1,4000000000L,3000000000L,0),"implausible future device time");
        System.out.println("Audio clock checks PASS");
    }
}
