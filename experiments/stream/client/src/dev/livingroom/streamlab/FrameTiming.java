package dev.livingroom.streamlab;

import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Map;

/** Times share Android's monotonic clock. Stages are API boundaries, not pure hardware durations. */
final class FrameTiming {
    final long rtp,receivedNs,assembledNs;
    long submittedNs,decodedNs,targetNs,presentedNs,callbackNs;
    long surfaceRequestNs;
    int queueDepth,inFlightDepth,outcome;
    double audioBufferMs;
    FrameTiming(long rtp,long receivedNs,long assembledNs){this.rtp=rtp;this.receivedNs=receivedNs;this.assembledNs=assembledNs;}
    long[] row(){return new long[]{rtp,receivedNs,assembledNs,submittedNs,decodedNs,targetNs,presentedNs,callbackNs,queueDepth,inFlightDepth,Math.round(audioBufferMs*1000),outcome,surfaceRequestNs};}
    static double percentile(ArrayList<Double> values,double q) {
        if(values.isEmpty())return 0;
        Collections.sort(values);return values.get(Math.max(0,(int)Math.ceil(values.size()*q)-1));
    }
    static final class Window {
        private final ArrayDeque<FrameTiming> recent=new ArrayDeque<>();
        private final ArrayDeque<Long> arrivals=new ArrayDeque<>();
        synchronized void arrival(long ns){arrivals.addLast(ns);while(arrivals.size()>120)arrivals.removeFirst();}
        synchronized void add(FrameTiming f){recent.addLast(f);while(recent.size()>120)recent.removeFirst();}
        synchronized Map<String,Double> summary(long now) {
            ArrayList<Double> assembly=new ArrayList<>(),queue=new ArrayList<>(),decode=new ArrayList<>(),presentation=new ArrayList<>(),sync=new ArrayList<>(),late=new ArrayList<>(),callback=new ArrayList<>(),depth=new ArrayList<>(),flight=new ArrayList<>();
            ArrayList<Long> presents=new ArrayList<>(),rx=new ArrayList<>();
            for(FrameTiming f:recent)if(f.presentedNs>=f.decodedNs && f.decodedNs>=f.submittedNs && f.submittedNs>=f.assembledNs && f.assembledNs>=f.receivedNs && f.receivedNs>0 && now>=f.callbackNs && now-f.callbackNs<3000000000L) {
                presents.add(f.presentedNs);assembly.add((f.assembledNs-f.receivedNs)/1e6);queue.add((f.submittedNs-f.assembledNs)/1e6);
                decode.add((f.decodedNs-f.submittedNs)/1e6);presentation.add((f.presentedNs-f.decodedNs)/1e6);
                sync.add(Math.max(0,f.targetNs-f.decodedNs)/1e6);late.add(f.targetNs>0?Math.max(0,f.presentedNs-f.targetNs)/1e6:0);
                callback.add(Math.max(0,f.callbackNs-f.presentedNs)/1e6);depth.add((double)f.queueDepth);flight.add((double)f.inFlightDepth);
            }
            for(long ns:arrivals)if(now-ns<3000000000L)rx.add(ns);
            ArrayList<Double> gaps=gaps(presents),receiveGaps=gaps(rx);int longFrames=0;for(double g:gaps)if(g>25)longFrames++;
            Map<String,Double> m=new LinkedHashMap<>();m.put("frames",(double)presents.size());m.put("intervals",(double)gaps.size());m.put("longFrames",(double)longFrames);
            m.put("renderGapP95Ms",percentile(gaps,.95));m.put("renderGapP99Ms",percentile(gaps,.99));m.put("renderGapMaxMs",percentile(gaps,1));
            m.put("receiveGapP95Ms",percentile(receiveGaps,.95));
            double totalGap=0;for(double gap:gaps)totalGap+=gap;
            m.put("renderFps",totalGap<=0?0:gaps.size()*1000.0/totalGap);
            m.put("lastPresentAgeMs",presents.isEmpty()?3000.0:Math.max(0,now-presents.get(presents.size()-1))/1e6);
            for(Map.Entry<String,ArrayList<Double>> e:stages(assembly,queue,decode,presentation,sync,late,callback,depth,flight).entrySet()) {
                String unit=e.getKey().equals("queueDepth") || e.getKey().equals("inFlight")?"":"Ms";
                m.put(e.getKey()+"P50"+unit,percentile(e.getValue(),.5));m.put(e.getKey()+"P95"+unit,percentile(e.getValue(),.95));
            }
            return m;
        }
        private ArrayList<Double> gaps(ArrayList<Long> times){Collections.sort(times);ArrayList<Double> r=new ArrayList<>();for(int i=1;i<times.size();i++)if(times.get(i)>times.get(i-1))r.add((times.get(i)-times.get(i-1))/1e6);return r;}
        private Map<String,ArrayList<Double>> stages(ArrayList<Double> a,ArrayList<Double> q,ArrayList<Double> d,ArrayList<Double> p,ArrayList<Double> s,ArrayList<Double> l,ArrayList<Double> c,ArrayList<Double> depth,ArrayList<Double> flight){
            Map<String,ArrayList<Double>> m=new LinkedHashMap<>();m.put("assembly",a);m.put("queue",q);m.put("decode",d);m.put("presentation",p);m.put("syncWait",s);m.put("surfaceLate",l);m.put("callbackLag",c);m.put("queueDepth",depth);m.put("inFlight",flight);return m;
        }
    }
}
