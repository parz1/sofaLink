using System.ComponentModel;
using System.Diagnostics;
using System.Net;
using System.Text.RegularExpressions;
using LivingroomHub.Core;

namespace LivingroomHub.Adapters.FireTv;

public sealed class AdbClient(string executable, string address)
{
    public static string NormalizeAddress(string address)
    {
        var parts = address.Split(':');
        if (parts.Length is < 1 or > 2 || !IPAddress.TryParse(parts[0], out var ip) ||
            ip.AddressFamily != System.Net.Sockets.AddressFamily.InterNetwork ||
            (parts.Length == 2 && (!int.TryParse(parts[1], out var port) || port is < 1 or > 65535)))
            throw new DeviceException("invalid_address", "请输入 IPv4 地址，可附带端口，例如 192.168.1.50:5555。");
        return ip + ":" + (parts.Length == 2 ? int.Parse(parts[1]) : 5555);
    }

    public async Task<string> RunAsync(CancellationToken cancellation, params string[] arguments)
    {
        using var process = new Process { StartInfo = new ProcessStartInfo(executable)
        { UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true } };
        foreach (var argument in arguments) process.StartInfo.ArgumentList.Add(argument);
        try { process.Start(); }
        catch (Win32Exception) { throw new DeviceException("adb_unavailable", "无法运行 ADB，请安装 Android SDK Platform Tools，或使用 --adb 指定 adb.exe。"); }
        var stdout = process.StandardOutput.ReadToEndAsync();
        var stderr = process.StandardError.ReadToEndAsync();
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellation);
        timeout.CancelAfter(TimeSpan.FromSeconds(15));
        try { await process.WaitForExitAsync(timeout.Token); }
        catch (OperationCanceledException)
        {
            if (!process.HasExited) process.Kill(entireProcessTree: true);
            await process.WaitForExitAsync(CancellationToken.None);
            await Task.WhenAll(stdout, stderr);
            cancellation.ThrowIfCancellationRequested();
            throw new DeviceException("adb_timeout", "ADB 操作超过 15 秒，请检查设备网络和调试授权。");
        }
        var output = (await stdout).Trim();
        var error = (await stderr).Trim();
        if (process.ExitCode != 0) throw new DeviceException("adb_failed", Classify(output + " " + error));
        return output;
    }

    private static string Classify(string output) => output.Contains("unauthorized", StringComparison.OrdinalIgnoreCase)
        ? "请在 Fire TV 上确认此电脑的 ADB 调试授权。"
        : output.Contains("offline", StringComparison.OrdinalIgnoreCase) ? "设备离线，请检查 Fire TV 和网络。"
        : "ADB 操作失败，请检查网络、IP 和设备调试设置。";

    public async Task ConnectAsync(CancellationToken cancellation)
    {
        await RunAsync(cancellation, "connect", address);
        var devices = await RunAsync(cancellation, "devices");
        var line = devices.Split('\n').Select(x => x.Trim()).FirstOrDefault(x => x.StartsWith(address + "\t"));
        if (line is null || line.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries).ElementAtOrDefault(1) != "device")
            throw new DeviceException("device_not_ready", line is null ? "未连接到指定 Fire TV，请检查 IP、网络和 ADB 调试设置。" : Classify(line));
    }

    public Task<string> ShellAsync(CancellationToken cancellation, params string[] arguments) =>
        RunAsync(cancellation, ["-s", address, "shell", .. arguments]);

    public static void ValidatePackage(string package)
    {
        if (!Regex.IsMatch(package, @"^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z][A-Za-z0-9_]*)+$"))
            throw new DeviceException("invalid_package", "应用包名格式无效。");
    }

    public async Task<DeviceStatus> InspectAsync(string package, CancellationToken cancellation)
    {
        ValidatePackage(package);
        var model = await ShellAsync(cancellation, "getprop", "ro.product.model");
        var version = await ShellAsync(cancellation, "getprop", "ro.build.version.release");
        var packages = await ShellAsync(cancellation, "pm", "list", "packages", package);
        if (!packages.Split('\n').Any(x => x.Trim() == "package:" + package))
            throw new DeviceException("moonlight_missing", "未发现指定 Moonlight 包，可使用 --package 指定实际包名。");
        return new(model, version, package, await IsForegroundAsync(package, cancellation));
    }

    public async Task<bool?> IsForegroundAsync(string package, CancellationToken cancellation)
    {
        var output = await ShellAsync(cancellation, "dumpsys", "activity", "activities");
        var match = Regex.Match(output, @"(?:mResumedActivity|topResumedActivity):[^\r\n]*?\s([A-Za-z0-9_.]+)/");
        return match.Success ? match.Groups[1].Value == package : null;
    }

    public async Task LaunchAsync(string package, CancellationToken cancellation)
    {
        ValidatePackage(package);
        await InspectAsync(package, cancellation);
        var component = await ShellAsync(cancellation, "cmd", "package", "resolve-activity", "--brief", "-a",
            "android.intent.action.MAIN", "-c", "android.intent.category.LAUNCHER", package);
        var target = component.Split('\n').Select(x => x.Trim()).LastOrDefault(x => Regex.IsMatch(x, @"^[A-Za-z0-9_.]+/[A-Za-z0-9_.$]+$"));
        if (target is null || !target.StartsWith(package + "/", StringComparison.Ordinal))
            throw new DeviceException("launch_activity_missing", "无法解析 Moonlight 启动入口，需要真机适配。");
        await ShellAsync(cancellation, "am", "start", "-n", target);
        await ConfirmAsync(package, true, cancellation);
    }

    public async Task HomeAsync(string package, CancellationToken cancellation)
    {
        await ShellAsync(cancellation, "input", "keyevent", "KEYCODE_HOME");
        await ConfirmAsync(package, false, cancellation);
    }

    private async Task ConfirmAsync(string package, bool expected, CancellationToken cancellation)
    {
        for (var attempt = 0; attempt < 10; attempt++)
        {
            if (await IsForegroundAsync(package, cancellation) == expected) return;
            await Task.Delay(500, cancellation);
        }
        throw new DeviceException("foreground_unconfirmed", "已发送操作，但无法确认预期前台状态。");
    }
}
