package dev.livingroom.streamlab;
import java.net.*;
import org.json.JSONObject;
final class ClockSync implements AutoCloseable {
    final ClockEstimate estimate=new ClockEstimate();
    private volatile boolean running=true;
    private DatagramSocket socket;
    private final Thread worker;
    ClockSync(String host,String session) {
        worker=new Thread(()->{
            try {
                InetAddress address=InetAddress.getByName(host);socket=new DatagramSocket();socket.setSoTimeout(500);long seq=0;
                while(running) {
                    long c1=System.nanoTime();JSONObject p=new JSONObject();p.put("type","clock_ping");p.put("sessionId",session);p.put("sequence",++seq);p.put("clientNs",c1);
                    byte[] data=p.toString().getBytes("UTF-8");socket.send(new DatagramPacket(data,data.length,address,5005));
                    byte[] bytes=new byte[1024];DatagramPacket response=new DatagramPacket(bytes,bytes.length);
                    try {
                        socket.receive(response);long c4=System.nanoTime();
                        JSONObject r=new JSONObject(new String(bytes,0,response.getLength(),"UTF-8"));
                        if(response.getAddress().equals(address) && response.getPort()==5005 && r.optString("type").equals("clock_pong") && r.optString("sessionId").equals(session) && r.getLong("sequence")==seq && r.getLong("clientNs")==c1)
                            estimate.add(c1,r.getLong("hostReceiveNs"),r.getLong("hostSendNs"),c4);
                    }catch(SocketTimeoutException ignored){}
                    Thread.sleep(500);
                }
            }catch(Exception ignored){}finally{if(socket!=null)socket.close();}
        },"clock-probe");worker.start();
    }
    public void close(){running=false;if(socket!=null)socket.close();worker.interrupt();try{worker.join(1000);}catch(InterruptedException e){Thread.currentThread().interrupt();}}
}
