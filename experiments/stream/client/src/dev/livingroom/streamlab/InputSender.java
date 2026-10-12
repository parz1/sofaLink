package dev.livingroom.streamlab;

import android.view.KeyEvent;
import java.net.Socket;
import java.net.InetSocketAddress;
import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.util.TreeSet;
import java.util.ArrayDeque;
import org.json.JSONArray;
import org.json.JSONObject;

/** Input snapshots use a separate reliable, small-packet connection. UI never does network I/O. */
final class InputSender implements AutoCloseable {
    private final Object lock=new Object();
    private final TreeSet<Integer> keys=new TreeSet<>();
    private final ArrayDeque<JSONObject> pending=new ArrayDeque<>();
    private final String host,session,key;
    private final int port;
    private int buttons,dx,dy,wheel;
    private boolean dirty=true;
    private volatile boolean running=true;
    private volatile Socket socket;
    private final Thread worker;
    volatile boolean ready;
    volatile String status="等待电脑控制握手";
    volatile double roundTripMs;
    InputSender(String host,int port,String session,String key) {
        this.host=host;this.port=port;this.session=session;this.key=key;
        worker=new Thread(this::run,"input-return");worker.start();
    }
    boolean key(KeyEvent event,boolean down) {
        int vk=mapKey(event.getKeyCode());if(vk==0)return false;
        synchronized(lock) {
            if(!ready)return true;
            boolean changed=down?keys.add(vk):keys.remove(vk);
            if(changed)enqueue();
        }
        return true;
    }
    void mouse(int x,int y,int buttonState,int scroll) {
        synchronized(lock) {
            if(!ready)return;
            boolean changed=buttons!=(buttonState&7);
            dx=Math.max(-4096,Math.min(4096,dx+x));dy=Math.max(-4096,Math.min(4096,dy+y));
            buttons=buttonState&7;wheel=Math.max(-1200,Math.min(1200,wheel+scroll));dirty=true;lock.notifyAll();
            if(changed)enqueue();
        }
    }
    private JSONObject snapshot() {
        JSONObject p=new JSONObject();
        try {p.put("keys",new JSONArray(keys));p.put("buttons",buttons);p.put("dx",dx);p.put("dy",dy);p.put("wheel",wheel);}catch(Exception ignored){}
        dx=dy=wheel=0;return p;
    }
    private void enqueue() {
        if(pending.size()>=128) {pending.clear();keys.clear();buttons=dx=dy=wheel=0;}
        pending.addLast(snapshot());dirty=true;lock.notifyAll();
    }
    void release() {synchronized(lock){keys.clear();buttons=dx=dy=wheel=0;pending.clear();enqueue();}}
    private void run() {
        while(running) {
            try {
                Socket s=new Socket();socket=s;s.setTcpNoDelay(true);s.setSoTimeout(500);
                s.connect(new InetSocketAddress(host,port),1000);
                BufferedReader input=new BufferedReader(new InputStreamReader(s.getInputStream(),"UTF-8"));
                OutputStream output=s.getOutputStream();long sequence=0;
                release();
                while(running) {
                    JSONObject p;
                    synchronized(lock) {
                        if(!dirty && pending.isEmpty())lock.wait(100);
                        if(!running)break;
                        p=pending.isEmpty()?snapshot():pending.removeFirst();
                        p.put("version",1);p.put("session",session);p.put("key",key);p.put("sequence",++sequence);
                        dirty=false;
                    }
                    long sent=System.nanoTime();output.write((p.toString()+"\n").getBytes("UTF-8"));output.flush();
                    String line=input.readLine();if(line==null)throw new Exception("电脑控制断开");
                    JSONObject ack=new JSONObject(line);if(ack.getLong("sequence")!=sequence)throw new Exception("输入确认不匹配");
                    roundTripMs=(System.nanoTime()-sent)/1e6;ready=true;status="键鼠控制可用";
                }
            } catch(Exception e) {ready=false;status="电脑控制断开，重新连接中";}
            finally {
                ready=false;release();Socket s=socket;socket=null;if(s!=null)try{s.close();}catch(Exception ignored){}
            }
            if(running)try{Thread.sleep(250);}catch(InterruptedException ignored){}
        }
    }
    public void close() {
        running=false;ready=false;release();Socket s=socket;if(s!=null)try{s.close();}catch(Exception ignored){}
        worker.interrupt();try{worker.join(1500);}catch(InterruptedException e){Thread.currentThread().interrupt();}
    }
    private static int mapKey(int code) {
        if(code>=KeyEvent.KEYCODE_A && code<=KeyEvent.KEYCODE_Z)return 65+code-KeyEvent.KEYCODE_A;
        if(code>=KeyEvent.KEYCODE_0 && code<=KeyEvent.KEYCODE_9)return 48+code-KeyEvent.KEYCODE_0;
        if(code>=KeyEvent.KEYCODE_F1 && code<=KeyEvent.KEYCODE_F12)return 112+code-KeyEvent.KEYCODE_F1;
        switch(code) {
            case KeyEvent.KEYCODE_DPAD_LEFT:return 37;case KeyEvent.KEYCODE_DPAD_UP:return 38;
            case KeyEvent.KEYCODE_DPAD_RIGHT:return 39;case KeyEvent.KEYCODE_DPAD_DOWN:return 40;
            case KeyEvent.KEYCODE_DPAD_CENTER:case KeyEvent.KEYCODE_ENTER:case KeyEvent.KEYCODE_NUMPAD_ENTER:return 13;
            case KeyEvent.KEYCODE_BACK:case KeyEvent.KEYCODE_ESCAPE:return 27;case KeyEvent.KEYCODE_SPACE:return 32;case KeyEvent.KEYCODE_TAB:return 9;
            case KeyEvent.KEYCODE_DEL:return 8;case KeyEvent.KEYCODE_FORWARD_DEL:return 46;
            case KeyEvent.KEYCODE_INSERT:return 45;case KeyEvent.KEYCODE_MOVE_HOME:return 36;case KeyEvent.KEYCODE_MOVE_END:return 35;
            case KeyEvent.KEYCODE_PAGE_UP:return 33;case KeyEvent.KEYCODE_PAGE_DOWN:return 34;
            case KeyEvent.KEYCODE_SHIFT_LEFT:return 160;case KeyEvent.KEYCODE_SHIFT_RIGHT:return 161;
            case KeyEvent.KEYCODE_CTRL_LEFT:return 162;case KeyEvent.KEYCODE_CTRL_RIGHT:return 163;
            case KeyEvent.KEYCODE_ALT_LEFT:return 164;case KeyEvent.KEYCODE_ALT_RIGHT:return 165;
            case KeyEvent.KEYCODE_SEMICOLON:return 186;case KeyEvent.KEYCODE_EQUALS:return 187;
            case KeyEvent.KEYCODE_COMMA:return 188;case KeyEvent.KEYCODE_MINUS:return 189;
            case KeyEvent.KEYCODE_PERIOD:return 190;case KeyEvent.KEYCODE_SLASH:return 191;
            case KeyEvent.KEYCODE_GRAVE:return 192;case KeyEvent.KEYCODE_LEFT_BRACKET:return 219;
            case KeyEvent.KEYCODE_BACKSLASH:return 220;case KeyEvent.KEYCODE_RIGHT_BRACKET:return 221;
            case KeyEvent.KEYCODE_APOSTROPHE:return 222;
            default:return 0;
        }
    }
}
