package dev.livingroom.streamlab;
import android.content.*;
import android.os.*;
import java.lang.ref.WeakReference;
import java.net.*;
import java.io.*;
import java.util.UUID;
import java.util.concurrent.ArrayBlockingQueue;
import org.json.*;

/** Control runs independently of media. Business states/actions come only from core. */
final class HostControl {
    static HostControl instance;
    final SharedPreferences prefs;
    final Context context;
    final Handler ui=new Handler(Looper.getMainLooper());
    final ArrayBlockingQueue<JSONObject> commands=new ArrayBlockingQueue<>(4);
    volatile JSONObject snapshot;
    volatile String connection="请输入电脑地址与配对码",session="";
    volatile String stoppedSession="",desiredStart="";
    volatile String hostInstance="";
    volatile boolean clientRunning;
    volatile String host,key,pairCode="";
    final String device;
    long lastReply=System.nanoTime();
    String handledStart="",handledProbe="";
    JSONArray probe;
    String probeComplete="",probeError="";
    WeakReference<LauncherActivity> home=new WeakReference<>(null);
    WeakReference<MainActivity> player=new WeakReference<>(null);
    private HostControl(Context context){
        this.context=context.getApplicationContext();prefs=this.context.getSharedPreferences("sofalink-control",Context.MODE_PRIVATE);
        device=prefs.getString("deviceId",UUID.randomUUID().toString());prefs.edit().putString("deviceId",device).apply();
        host=prefs.getString("host","");key=prefs.getString("key","");
        Thread worker=new Thread(this::run,"sofalink-control");worker.setDaemon(true);worker.start();
    }
    static synchronized HostControl get(Context c){if(instance==null)instance=new HostControl(c);return instance;}
    void home(LauncherActivity a){home=new WeakReference<>(a);notifyHome();}
    void player(MainActivity a,boolean running){player=new WeakReference<>(a);clientRunning=running;}
    void running(boolean running){clientRunning=running;}
    void pair(String address,String code){
        String value=address.trim();if(!value.matches("(?:[0-9]{1,3}\\.){3}[0-9]{1,3}(?::[0-9]{1,5})?")||!code.trim().matches("[0-9]{6}")){connection="请输入电脑 IPv4 地址和 6 位配对码";notifyHome();return;}
        host=value;key="";pairCode=code.trim();prefs.edit().putString("host",host).remove("key").apply();connection="正在配对…";notifyHome();
    }
    void command(JSONObject c){JSONObject s=snapshot;if(s==null)return;try{
        JSONObject r=new JSONObject();r.put("command",c);r.put("expected_token",s.getJSONObject("token"));r.put("expected_instance",hostInstance);
        if(!commands.offer(r)){connection="操作正在处理中";notifyHome();}
    }catch(Exception e){connection="状态暂不可用";notifyHome();}}
    void stop(){JSONObject s=snapshot;if(s==null)return;JSONArray a=s.optJSONArray("available_commands");if(a!=null)for(int i=0;i<a.length();i++){JSONObject c=a.optJSONObject(i);if(c!=null&&"stop".equals(c.optString("command"))){command(c);return;}}}
    JSONObject exchange(JSONObject request)throws Exception{
        String[] address=host.split(":",-1);if(address.length>2)throw new IllegalArgumentException("IPv4 required");
        int port=address.length==2?Integer.parseInt(address[1]):47990;
        try(Socket socket=new Socket()){
            socket.connect(new InetSocketAddress(address[0],port),800);socket.setSoTimeout(1200);socket.setTcpNoDelay(true);
            byte[] bytes=request.toString().getBytes("UTF-8");DataOutputStream output=new DataOutputStream(socket.getOutputStream());output.writeInt(bytes.length);output.write(bytes);output.flush();
            DataInputStream input=new DataInputStream(socket.getInputStream());int size=input.readInt();if(size<1||size>65536)throw new IOException("Invalid host response");
            bytes=new byte[size];input.readFully(bytes);return new JSONObject(new String(bytes,"UTF-8"));
        }
    }
    void run(){while(true){try{
        if(!host.isEmpty()&&(!key.isEmpty()||!pairCode.isEmpty())){
            JSONObject queued=commands.poll(),r=queued==null?new JSONObject():queued;boolean pairing=key.isEmpty();
            r.put("version",1);r.put("op",pairing?"pair":queued==null?"poll":"command");r.put("device_id",device);r.put("key",key);r.put("pair_code",pairCode);
            r.put("client_session",session);r.put("client_running",clientRunning);r.put("client_stopped_session",stoppedSession);r.put("probe_complete",probeComplete);r.put("probe_error",probeError);if(probe!=null)r.put("probe",probe);
            JSONObject reply=exchange(r);if(reply.getInt("version")!=1)throw new IOException("Host protocol mismatch");lastReply=System.nanoTime();
            if(reply.has("paired_key")){key=reply.getString("paired_key");pairCode="";prefs.edit().putString("key",key).apply();}
            if(reply.has("snapshot")){
                hostInstance=reply.getString("instance");
                snapshot=reply.getJSONObject("snapshot");
                String phase=snapshot.optString("phase");
                if(clientRunning&&("idle".equals(phase)||"faulted".equals(phase)))ui.post(()->{MainActivity a=player.get();if(a!=null)a.endManagedStream();});
            }
            connection=reply.has("snapshot")||reply.isNull("error")||!reply.has("error")?"已连接电脑":reply.optString("error");
            if(reply.optBoolean("pairing_required",false)){forget();connection="需要重新配对，请输入电脑显示的配对码";ui.post(()->{MainActivity a=player.get();if(a!=null)a.endManagedStream();});}
            if(pairing&&reply.has("error"))pairCode="";
            JSONObject action=reply.optJSONObject("action");if(action!=null)act(action);notifyHome();
        }
    }catch(Exception e){connection="电脑连接中断 · 正在重连";notifyHome();}
        if(clientRunning&&System.nanoTime()-lastReply>3500000000L)ui.post(()->{MainActivity a=player.get();if(a!=null)a.endManagedStream();});
        try{Thread.sleep(250);}catch(InterruptedException ignored){return;}
    }}
    void act(JSONObject action)throws Exception{
        String kind=action.getString("kind");
        if("probe".equals(kind)){
            String id=action.getString("id");if(!id.equals(handledProbe)){
                probe=null;probeError="";
                try{JSONObject caps=new JSONObject(MediaCapabilities.query(context,action.getJSONObject("request").toString()).getString("capabilities"));
                    if(caps.has("error"))probeError=caps.getString("error");else probe=new JSONArray().put(id).put(caps);
                }catch(Exception e){probeError="无法完成电视音视频能力探测";}
                finally{probeComplete=id;handledProbe=id;}
            }
        }else if("start".equals(kind)){
            String phase=snapshot==null?"":snapshot.optString("phase");if(!"starting_client".equals(phase)&&!"recovering".equals(phase))return;
            String id=action.getString("session");if(!id.equals(handledStart)){
                JSONObject extras=action.getJSONObject("extras");handledStart=id;desiredStart=id;session=id;stoppedSession="";probe=null;clientRunning=false;
                ui.post(()->{if(!id.equals(desiredStart))return;LauncherActivity a=home.get();if(a==null)return;MainActivity old=player.get();if(old!=null)old.endManagedStream();
                    Intent intent=new Intent(a,MainActivity.class);java.util.Iterator<String> keys=extras.keys();while(keys.hasNext()){
                        String name=keys.next();Object value=extras.opt(name);if(value instanceof Boolean)intent.putExtra(name,(Boolean)value);
                        else if(value instanceof Number)intent.putExtra(name,((Number)value).intValue());else if(value instanceof String)intent.putExtra(name,(String)value);
                    }a.startActivity(intent);
                });
            }
        }else if("stop".equals(kind)&&(action.getString("session").equals(session)||(!clientRunning&&player.get()==null))){
            String id=action.getString("session");session=id;desiredStart="";
            if(!id.equals(stoppedSession))ui.post(()->{MainActivity a=player.get();if(a!=null)a.endManagedStream();clientRunning=false;stoppedSession=id;});
        }
    }
    void notifyHome(){ui.post(()->{LauncherActivity a=home.get();if(a!=null)a.render(snapshot,connection);});}
    void forget(){key="";pairCode="";snapshot=null;commands.clear();prefs.edit().remove("key").apply();notifyHome();}
    void address(String value){host=value.trim();prefs.edit().putString("host",host).apply();connection="正在重新连接电脑…";notifyHome();}
}
