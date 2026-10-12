using System.Diagnostics;
using System.Net;
using System.Net.Sockets;
using System.Text.Json;
using LivingroomHub.Adapters.FireTv;
using LivingroomHub.Core;

namespace LivingroomHub.Cli;

public sealed class StreamSession
{
    private readonly object sync = new();
    private StreamView view = new(StreamPhase.Idle, "准备就绪 · 1080p60 · 声音开启");
    private readonly string root, address, ip, python;
    private readonly AdbClient adb;
    private readonly bool test;
    private CancellationTokenSource? lifetime;
    private Task? operation;
    public StreamView View { get { lock (sync) return view; } }
    private void Set(StreamView value) { lock (sync) view = value; }
    public StreamSession(string root, bool test = false)
    {
        this.root = root; this.test = test;
        using var config = JsonDocument.Parse(File.ReadAllText(Path.Combine(root, "config.local.json")));
        var data = config.RootElement;
        address = AdbClient.NormalizeAddress(data.GetProperty("device").GetString()!);
        ip = address.Split(':')[0];
        adb = new(data.TryGetProperty("adb", out var path) ? path.GetString()! : "adb", address);
        python = data.TryGetProperty("python", out var py) ? py.GetString()! : "python";
    }
    public void Start()
    {
        lock (sync)
        {
            if (operation is { IsCompleted: false } || view.Active) return;
            lifetime?.Dispose(); lifetime = new();
            view = new(StreamPhase.Checking, "检查 Fire TV 和串流依赖");
            operation = Run(lifetime.Token);
        }
    }
    public async Task Stop()
    {
        Task? pending;
        lock (sync) { lifetime?.Cancel(); pending = operation; }
        if (pending is not null) await pending;
    }
    private async Task Run(CancellationToken cancel)
    {
        Process? host = null;
        Task? reader = null, errors = null;
        bool clientTouched = false;
        bool hostStarted = false;
        string? failure = null;
        var sessionId = Guid.NewGuid().ToString("N");
        var log = Path.Combine(root, "local", "sessions", sessionId + ".jsonl");
        var health = new StreamHealth(sessionId);
        var ready = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var stderr = "";
        try
        {
            await adb.ConnectAsync(cancel);
            var packages = await adb.ShellAsync(cancel, "pm", "list", "packages", "dev.livingroom.streamlab");
            if (!packages.Split('\n').Any(x => x.Trim() == "package:dev.livingroom.streamlab"))
                throw new Exception("电视端客户端未安装，请先运行 experiments/stream/build-client.ps1 并安装 APK");
            using var route = new UdpClient(); route.Connect(IPAddress.Parse(ip), 5004);
            var hostIp = ((IPEndPoint)route.Client.LocalEndPoint!).Address.ToString();
            Directory.CreateDirectory(Path.GetDirectoryName(log)!);
            Set(new(StreamPhase.StartingHost, "准备桌面捕获、硬件编码和系统声音", LogPath: log));
            host = new() { StartInfo = new(python) { WorkingDirectory = root, UseShellExecute = false,
                RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true } };
            foreach (var arg in new[] { Path.Combine(root, "experiments", "stream", "host.py"), "--device", ip,
                "--session-id", sessionId, "--control-stdin", "--log", log }) host.StartInfo.ArgumentList.Add(arg);
            if (test) { host.StartInfo.ArgumentList.Add("--test-pattern"); host.StartInfo.ArgumentList.Add("--test-tone"); }
            host.Start(); hostStarted = true;
            errors = Task.Run(async () => { stderr = await host.StandardError.ReadToEndAsync(); });
            reader = Task.Run(async () =>
            {
                while (await host.StandardOutput.ReadLineAsync() is { } line)
                {
                    try
                    {
                        using var doc = JsonDocument.Parse(line); var e = doc.RootElement;
                        if (e.GetProperty("session_id").GetString() != sessionId) continue;
                        var type = e.GetProperty("type").GetString();
                        if (type == "host_ready") ready.TrySetResult();
                        if (type == "error") ready.TrySetException(new Exception(e.GetProperty("message").GetString()));
                        if (type == "client")
                        {
                            var m = e.GetProperty("metrics");
                            var evidence = new StreamEvidence(m.GetProperty("sessionId").GetString()!, DateTimeOffset.UtcNow,
                                m.GetProperty("rendered").GetInt64(), m.GetProperty("audioPlayedFrames").GetInt64(),
                                m.GetProperty("videoHealthy").GetBoolean(), m.GetProperty("audioHealthy").GetBoolean(),
                                m.GetProperty("fps").GetDouble(), m.GetProperty("receiveToRenderP50Ms").GetDouble(),
                                m.GetProperty("audioBufferedMs").GetDouble(), m.GetProperty("audioError").GetString()!);
                            lock (sync) health.Accept(evidence);
                        }
                    }
                    catch (JsonException) { /* Native diagnostics can accompany structured events. */ }
                }
                ready.TrySetException(new Exception("主机未就绪便退出，请查看会话日志"));
            });
            await ready.Task.WaitAsync(TimeSpan.FromSeconds(15), cancel);
            Set(new(StreamPhase.StartingClient, "启动 Fire TV，等待实际音画播放", LogPath: log));
            clientTouched = true;
            await adb.ShellAsync(cancel, "am", "force-stop", "dev.livingroom.streamlab");
            var launched = await adb.ShellAsync(cancel, "am", "start", "-n", "dev.livingroom.streamlab/.MainActivity",
                "--es", "host", hostIp, "--es", "transport", "tcp", "--es", "sessionId", sessionId);
            if (launched.Contains("Error:", StringComparison.OrdinalIgnoreCase)) throw new Exception("Fire TV 应用启动失败：" + launched);
            var deadline = DateTimeOffset.UtcNow.AddSeconds(20);
            DateTimeOffset? missingSince = null;
            while (true)
            {
                cancel.ThrowIfCancellationRequested();
                if (host.HasExited) throw new Exception("串流主机退出：" + (stderr.Length > 0 ? stderr.Trim()[..Math.Min(300, stderr.Trim().Length)] : "请查看日志"));
                lock (sync)
                {
                    var now = DateTimeOffset.UtcNow; var phase = health.Phase(now);
                    view = new(phase, health.Reason(now), health.Latest, log);
                    if (!health.HasPlayed && now > deadline) throw new Exception("启动超时：" + view.Message);
                    if (health.HasPlayed && (health.Latest is null || now - health.Latest.At > TimeSpan.FromSeconds(3)))
                        missingSince ??= now;
                    else missingSince = null;
                    if (missingSince is not null && now - missingSince > TimeSpan.FromSeconds(5))
                        throw new Exception("客户端反馈持续中断，会话已结束");
                }
                await Task.Delay(200, cancel);
            }
        }
        catch (OperationCanceledException) when (cancel.IsCancellationRequested) { }
        catch (Exception error) { failure = error.Message; }
        finally
        {
            Set(View with { Phase = StreamPhase.Stopping, Message = "停止串流并退出电视端" });
            if (host is not null && hostStarted)
            {
                try
                {
                    if (!host.HasExited)
                    {
                        await host.StandardInput.WriteLineAsync("stop"); await host.StandardInput.FlushAsync();
                        await host.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5));
                    }
                }
                catch (Exception) { if (!host.HasExited) host.Kill(entireProcessTree: true); }
                try { await host.WaitForExitAsync(); if (reader is not null) await reader; if (errors is not null) await errors; }
                catch (Exception e) { failure ??= "主机清理异常：" + e.Message; }
                host.Dispose();
            }
            else host?.Dispose();
            if (clientTouched)
            {
                try
                {
                    await adb.ShellAsync(default, "am", "force-stop", "dev.livingroom.streamlab");
                    var pid = await adb.ShellAsync(default, "ps", "-A");
                    if (pid.Split('\n').Any(x => x.Trim().EndsWith(" dev.livingroom.streamlab")))
                        throw new Exception("电视端仍在运行");
                }
                catch (Exception e) { failure = (failure is null ? "" : failure + "；") + "退出电视端失败：" + e.Message; }
            }
            Set(new(failure is null ? StreamPhase.Idle : StreamPhase.Faulted,
                failure ?? "已结束 · 可以再次开始", LogPath: log));
        }
    }
}
