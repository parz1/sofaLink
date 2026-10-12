package dev.livingroom.streamlab;
import android.app.Activity;
import android.os.Bundle;
import android.graphics.Color;
import android.view.*;
import android.widget.*;
import android.text.InputType;
import org.json.*;

/** TV home projects Snapshot; pairing is connection setup, not another session FSM. */
public final class LauncherActivity extends Activity {
    HostControl control;TextView state,details,connection;LinearLayout pairing;
    Button primary,end;EditText address,code;JSONObject primaryCommand;boolean firstReady;
    boolean visible;long lastRevision=-1;String lastConnection="",lastInstance="";
    public void onCreate(Bundle saved){
        super.onCreate(saved);control=HostControl.get(this);
        FrameLayout root=new FrameLayout(this);root.setBackgroundColor(Color.rgb(17,22,30));
        LinearLayout content=new LinearLayout(this);content.setOrientation(LinearLayout.VERTICAL);content.setPadding(36,24,36,24);
        int width=Math.min(getResources().getDisplayMetrics().widthPixels-64,Math.round(620*getResources().getDisplayMetrics().density));
        root.addView(content,new FrameLayout.LayoutParams(width,-2,Gravity.CENTER));
        TextView title=text("sofaLink",38);title.setTextColor(Color.rgb(138,226,210));content.addView(title);content.addView(text("电脑游戏，坐下来玩。",20));
        state=text("连接你的电脑",26);details=text("首次使用：电脑运行主机服务，在这里输入配对码。",18);connection=text("",15);
        content.addView(state);content.addView(details);content.addView(connection);
        pairing=new LinearLayout(this);pairing.setOrientation(LinearLayout.VERTICAL);content.addView(pairing);
        address=new EditText(this);address.setTextColor(Color.WHITE);address.setSingleLine(true);address.setHintTextColor(Color.LTGRAY);address.setHint("电脑局域网 IP，例如 192.0.2.10");address.setText(control.prefs.getString("host",""));pairing.addView(address);
        code=new EditText(this);code.setTextColor(Color.WHITE);code.setHintTextColor(Color.LTGRAY);code.setSingleLine(true);code.setInputType(InputType.TYPE_CLASS_NUMBER);code.setHint("电脑显示的 6 位配对码");pairing.addView(code);
        Button pair=new Button(this);pair.setText("连接并配对");pair.setOnClickListener(v->control.pair(address.getText().toString(),code.getText().toString()));pairing.addView(pair);
        primary=new Button(this);primary.setTextSize(20);putText(primary,"等待电脑状态");primary.setEnabled(false);primary.setOnClickListener(v->{if(primaryCommand!=null){control.command(primaryCommand);primary.setEnabled(false);}});
        primary.setOnFocusChangeListener((v,focused)->{primary.setBackgroundTintList(android.content.res.ColorStateList.valueOf(focused?Color.rgb(138,226,210):Color.rgb(42,54,67)));primary.setTextColor(focused?Color.rgb(17,22,30):Color.WHITE);});content.addView(primary);
        end=new Button(this);end.setText("结束会话");end.setVisibility(View.GONE);end.setOnClickListener(v->control.stop());content.addView(end);
        Button settings=new Button(this);settings.setText("连接设置");settings.setOnClickListener(v->{
            EditText input=new EditText(this);input.setSingleLine(true);input.setText(control.prefs.getString("host",""));
            new android.app.AlertDialog.Builder(this).setTitle("电脑地址").setView(input).setPositiveButton("保存地址",(d,w)->control.address(input.getText().toString()))
                .setNeutralButton("重新配对",(d,w)->control.forget()).setNegativeButton("取消",null).show();
        });content.addView(settings);
        setContentView(root);control.home(this);
    }
    static void putText(TextView view,String value){if(!value.contentEquals(view.getText()))view.setText(value);}
    TextView text(String value,int size){TextView t=new TextView(this);t.setText(value);t.setTextSize(size);t.setTextColor(Color.WHITE);t.setPadding(0,8,0,8);return t;}
    public void onResume(){super.onResume();visible=true;lastRevision=-1;if(control!=null)control.home(this);}
    public void onPause(){visible=false;super.onPause();}
    void render(JSONObject s,String message){
        if(!visible)return;
        if(lastConnection.equals(message)&&lastInstance.equals(control.hostInstance)&&s!=null&&lastRevision==s.optLong("revision"))return;
        lastConnection=message;lastInstance=control.hostInstance;if(s!=null)lastRevision=s.optLong("revision");
        putText(connection,message);
        if(s==null){pairing.setVisibility(View.VISIBLE);putText(state,"连接你的电脑");putText(details,"输入电脑地址与电脑显示的配对码。");primary.setEnabled(false);primaryCommand=null;end.setVisibility(View.GONE);firstReady=false;return;}
        pairing.setVisibility(View.GONE);
        if(!"已连接电脑".equals(message)){putText(state,"最新会话状态暂不可用");putText(details,"上次状态："+s.optString("message"));primary.setEnabled(false);primaryCommand=null;putText(primary,"等待连接…");end.setVisibility(View.GONE);return;}
        if("idle".equals(s.optString("phase"))||"faulted".equals(s.optString("phase")))getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);else getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        putText(state,s.optString("message"));
        JSONObject app=s.optJSONObject("application"),audio=s.optJSONObject("host_audio"),issue=s.optJSONObject("issue");
        String info=app==null?"":app.optString("message");if(audio!=null)info+=(info.isEmpty()?"":"\n")+audio.optString("message");if(issue!=null)info+="\n"+issue.optString("message");putText(details,info);
        String previous=primaryCommand==null?"":primaryCommand.optString("command");
        JSONArray available=s.optJSONArray("available_commands");primaryCommand=available==null?null:available.optJSONObject(0);
        primary.setEnabled(primaryCommand!=null&&"已连接电脑".equals(message));String c=primaryCommand==null?"":primaryCommand.optString("command");
        putText(primary,"start".equals(c)?"开始游戏":"stop".equals(c)?"结束会话":"retry".equals(c)?"重新开始":"recover".equals(c)?"恢复连接":"请稍候…");
        boolean stop=false;if(available!=null)for(int i=0;i<available.length();i++)if("stop".equals(available.optJSONObject(i).optString("command")))stop=true;
        end.setVisibility(stop&&!"stop".equals(c)?View.VISIBLE:View.GONE);
        if(primary.isEnabled()&&(!firstReady||!previous.equals(c))){firstReady=true;primary.requestFocus();}
    }
}
