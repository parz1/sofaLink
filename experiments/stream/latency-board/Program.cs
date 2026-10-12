using System.Diagnostics;
using System.Drawing.Drawing2D;

ApplicationConfiguration.Initialize();
Application.Run(new TimingBoard());

sealed class TimingBoard : Form
{
    readonly Stopwatch clock = Stopwatch.StartNew();
    readonly System.Windows.Forms.Timer timer = new() { Interval = 8 };
    readonly Font digits = new("Consolas", 100, FontStyle.Bold);
    readonly Font caption = new("Microsoft YaHei UI", 20);
    long flashUntil;
    public TimingBoard()
    {
        Text = "Livingroom 延迟计时板 — 对照电脑与电视";
        WindowState = FormWindowState.Maximized;
        DoubleBuffered = true;
        KeyPreview = true;
        timer.Tick += (_, _) => Invalidate();
        timer.Start();
        KeyDown += (_, e) => { if (e.KeyCode == Keys.Space) flashUntil = clock.ElapsedMilliseconds + 200; };
    }
    protected override void OnPaint(PaintEventArgs e)
    {
        var ms = clock.ElapsedMilliseconds;
        var g = e.Graphics;
        g.Clear(ms < flashUntil ? Color.White : Color.FromArgb(15, 20, 30));
        g.SmoothingMode = SmoothingMode.AntiAlias;
        using var brush = new SolidBrush(ms < flashUntil ? Color.Black : Color.White);
        g.DrawString($"{ms / 1000:0000}.{ms % 1000:000}", digits, brush, 60, 100);
        g.DrawString("手机高速拍摄电脑和电视，对比同一画面中的计时差。", caption, brush, 70, 300);
        g.DrawString("空格闪白 · 差值包括两台显示器各自的显示延迟。", caption, brush, 70, 360);
        g.DrawString("计时板重绘受屏幕刷新率限制，数字并不代表 1 ms 测量精度。", caption, brush, 70, 420);
        var width = Math.Max(1, ClientSize.Width - 140);
        g.FillRectangle(Brushes.DeepSkyBlue, 70 + (int)(ms % 2000 * width / 2000), 500, 20, 100);
    }
    protected override void Dispose(bool disposing)
    {
        if (disposing) { timer.Dispose(); digits.Dispose(); caption.Dispose(); }
        base.Dispose(disposing);
    }
}
