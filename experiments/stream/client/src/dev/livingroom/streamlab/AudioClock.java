package dev.livingroom.streamlab;

/** Publish one coherent mapping from actual device playback to media time. */
final class AudioClock {
    private static final long FRESH_NS=500000000L;
    private static final long WRAP_NS=(1L<<32)*1000000000L/90000;
    private static final class Sample {
        final long media,local,observed,offset;
        Sample(long media,long local,long observed,long offset){this.media=media;this.local=local;this.observed=observed;this.offset=offset;}
    }
    private volatile Sample sample;
    void reset(){sample=null;}
    boolean update(long media,long local,long observed,long offset) {
        if(media<0 || local<=0 || observed<=0 || local>observed+100000000L)return false;
        Sample previous=sample;
        if(previous!=null && (local<previous.local || media<previous.media))return false;
        sample=new Sample(media,local,observed,offset);return true;
    }
    long target(long rtp,long now) {
        Sample s=sample;
        if(s==null || now<s.observed || now-s.observed>FRESH_NS || now-s.local>FRESH_NS)return 0;
        long media=(rtp&0xffffffffL)*1000000000L/90000+s.offset;
        while(media-s.media>WRAP_NS/2)media-=WRAP_NS;
        while(s.media-media>WRAP_NS/2)media+=WRAP_NS;
        return s.local+media-s.media;
    }
}
