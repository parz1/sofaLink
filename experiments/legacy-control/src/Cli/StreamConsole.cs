using System.Runtime.InteropServices;
using LivingroomHub.Core;

namespace LivingroomHub.Cli;

public static class StreamConsole
{
    [DllImport("kernel32.dll")] private static extern IntPtr GetStdHandle(int handle);
    [DllImport("kernel32.dll")] private static extern bool GetConsoleMode(IntPtr handle, out uint mode);
    [DllImport("kernel32.dll")] private static extern bool SetConsoleMode(IntPtr handle, uint mode);
    public static async Task<int> Run(string root, string[] args, CancellationToken cancel)
    {
        var test = args.Contains("--test");
        var session = new StreamSession(root, test);
        if (args.FirstOrDefault() == "session-check")
        {
            var index = Array.IndexOf(args, "--seconds");
            var seconds = index >= 0 ? int.Parse(args[index + 1]) : 15;
            if (seconds is < 1 or > 3600) throw new ArgumentException("seconds must be 1..3600");
            session.Start();
            var deadline = DateTimeOffset.UtcNow.AddSeconds(40);
            var last = "";
            try
            {
                while (session.View.Phase != StreamPhase.Playing)
                {
                    cancel.ThrowIfCancellationRequested();
                    var v = session.View;
                    if (last != v.Message) { Console.WriteLine($"{v.Phase}: {v.Message}"); last = v.Message; }
                    if (v.Phase == StreamPhase.Faulted || DateTimeOffset.UtcNow > deadline) throw new Exception(v.Message);
                    await Task.Delay(200, cancel);
                }
                Console.WriteLine("Playing: 已验证音画播放进度");
                var until = DateTimeOffset.UtcNow.AddSeconds(seconds);
                while (DateTimeOffset.UtcNow < until)
                {
                    await Task.Delay(1000, cancel);
                    var v = session.View;
                    Console.WriteLine($"{v.Phase}: {v.Message} | fps={v.Evidence?.Fps:F1} | render={v.Evidence?.RenderMs:F1}ms | audio={v.Evidence?.AudioBufferMs:F1}ms");
                    if (v.Phase != StreamPhase.Playing) throw new Exception("持续播放验证失败：" + v.Message);
                }
            }
            finally { await session.Stop(); }
            Console.WriteLine($"{session.View.Phase}: {session.View.Message} | {session.View.LogPath}");
            return session.View.Phase == StreamPhase.Idle ? 0 : 1;
        }
        if (Console.IsInputRedirected || Console.IsOutputRedirected)
            throw new Exception("TUI 需要交互终端；自动验证请用 session-check");
        if (!OperatingSystem.IsWindows()) throw new Exception("当前串流主机需要 Windows");
        var handle = GetStdHandle(-11); GetConsoleMode(handle, out var mode); SetConsoleMode(handle, mode | 4);
        var cursorVisible = Console.CursorVisible;
        Console.Write("\u001b[?1049h\u001b[?25l");
        bool details = false; string previous = "";
        try
        {
            while (!cancel.IsCancellationRequested)
            {
                var v = session.View; var m = v.Evidence;
                var fresh = m is not null && DateTimeOffset.UtcNow - m.At < TimeSpan.FromSeconds(3);
                var video = !fresh ? "等待反馈" : m!.VideoHealthy ? $"{m.Fps:F1} fps" : "等待画面";
                var audio = !fresh ? "等待反馈" : m!.AudioError.Length > 0 ? "异常" : m.AudioHealthy ? "正在播放 · 48 kHz 立体声" : "等待播放";
                var phase = v.Phase switch { StreamPhase.Idle => "准备就绪", StreamPhase.Checking => "检查设备",
                    StreamPhase.StartingHost => "准备主机", StreamPhase.StartingClient => "启动电视端", StreamPhase.WaitingMedia => "等待音画",
                    StreamPhase.Playing => "正在播放", StreamPhase.Degraded => "播放异常", StreamPhase.Stopping => "正在结束", _ => "启动失败 / 需要处理" };
                var lines = new List<string> { "sofaLink", "1080p60 · 硬件编码 / 解码 · 系统声音", "",
                    "状态  " + phase, "      " + v.Message.Replace('\n', ' ').Replace('\r', ' '), "",
                    "画面  " + (v.Active ? video : "未播放"), "声音  " + (v.Active ? audio : "未播放"), "",
                    "[ Enter  " + v.Action + " ]     [ D  " + (details ? "收起详情" : "查看详情") + " ]     [ Q  退出 ]" };
                if (details)
                {
                    lines.Add(""); lines.Add($"客户端呈现 {m?.RenderMs:F1} ms · 音频剩余缓冲 {m?.AudioBufferMs:F1} ms");
                    lines.Add($"实际呈现 {m?.Rendered} 帧 · 音频播放 {m?.AudioPlayed} 帧");
                    lines.Add("呈现指标为客户端分段时间；音频缓冲不是端到端延迟。");
                    lines.Add("日志 " + v.LogPath);
                }
                var screen = string.Join("\n", lines);
                if (screen != previous)
                {
                    // Clear to line end; bounded redraw preserves a stable, state-driven screen.
                    Console.Write("\u001b[H" + string.Join("\n", lines.Select(x => x + "\u001b[K")) + "\u001b[J");
                    previous = screen;
                }
                if (Console.KeyAvailable)
                {
                    var key = Console.ReadKey(true).Key;
                    if (key == ConsoleKey.Q) break;
                    if (key == ConsoleKey.D) details = !details;
                    if (key == ConsoleKey.Enter && v.Phase != StreamPhase.Stopping)
                    {
                        if (v.Active) _ = session.Stop(); else session.Start();
                    }
                }
                await Task.Delay(150);
            }
        }
        finally
        {
            await session.Stop();
            Console.Write("\u001b[?25h\u001b[?1049l"); Console.CursorVisible = cursorVisible; SetConsoleMode(handle, mode);
        }
        Console.WriteLine(session.View.Message);
        return session.View.Phase == StreamPhase.Faulted ? 1 : 0;
    }
}
