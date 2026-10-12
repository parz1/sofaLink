package dev.livingroom.streamlab;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.view.View;

/** Static RGB display control: no network, codec, video Surface, timer, or audio. */
final class ColorChartView extends View {
    private final Paint paint=new Paint();
    private final int[] colors={Color.WHITE,Color.YELLOW,Color.CYAN,Color.GREEN,Color.MAGENTA,Color.RED,Color.BLUE,Color.BLACK};
    ColorChartView(Context context) {super(context);}
    @Override protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        canvas.save();canvas.scale(getWidth()/1920f,getHeight()/1080f);
        canvas.drawColor(Color.rgb(24,24,24));
        paint.setColor(Color.WHITE);paint.setTextSize(40);paint.setAntiAlias(true);
        canvas.drawText("LOCAL RGB CONTROL / NO VIDEO DECODER",40,80,paint);
        paint.setAntiAlias(false);
        for(int i=0;i<8;i++) {
            paint.setColor(colors[i]);canvas.drawRect(i*240,160,(i+1)*240,480,paint);
        }
        for(int i=0;i<16;i++) {
            int gray=i*17;paint.setColor(Color.rgb(gray,gray,gray));
            canvas.drawRect(i*120,520,(i+1)*120,700,paint);
        }
        for(int i=0;i<480;i++) {
            paint.setColor(Color.WHITE);canvas.drawRect(240+i*3,740,241+i*3,940,paint);
            paint.setColor(Color.RED);canvas.drawRect(241+i*3,740,242+i*3,940,paint);
            paint.setColor(Color.BLUE);canvas.drawRect(242+i*3,740,243+i*3,940,paint);
        }
        canvas.restore();
    }
}
