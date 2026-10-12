using System.Text.Json;
using LivingroomHub.Adapters.FireTv;
using LivingroomHub.Core;
using LivingroomHub.Cli;

void Print(object value) => Console.WriteLine(JsonSerializer.Serialize(value, new JsonSerializerOptions
{
    WriteIndented = true,
    Encoder = System.Text.Encodings.Web.JavaScriptEncoder.Create(System.Text.Unicode.UnicodeRanges.All)
}));
if (args.Length > 0 && args[0] is "help" or "--help")
{
    Console.WriteLine("sofaLink\n用法：tui（默认） | session-check [--seconds 15] [--test]\n设备诊断：doctor | inspect | launch | home [--device IPv4[:port]] [--adb 路径] [--package 包名]\n串流使用 config.local.json；旧 launch/home 命令只控制 Moonlight 应用。");
    return 0;
}
using var cancellation = new CancellationTokenSource();
Console.CancelKeyPress += (_, e) => { e.Cancel = true; cancellation.Cancel(); };
try
{
    if (args.Length == 0 || args[0] is "tui" or "session-check")
    {
        var root = new DirectoryInfo(Environment.CurrentDirectory);
        while (root is not null && !File.Exists(Path.Combine(root.FullName, "experiments", "stream", "host.py"))) root = root.Parent;
        if (root is null) throw new Exception("请在项目目录运行");
        return await StreamConsole.Run(root.FullName, args, cancellation.Token);
    }
    var command = args[0];
    if (command is not ("doctor" or "inspect" or "launch" or "home"))
        throw new DeviceException("invalid_command", "未知命令。运行 help 查看用法。");
    var options = new Dictionary<string, string>();
    for (var i = 1; i < args.Length; i += 2)
        if (args[i] is not ("--adb" or "--device" or "--package") || i + 1 >= args.Length || !options.TryAdd(args[i], args[i + 1]))
            throw new DeviceException("invalid_arguments", "选项缺少值、重复或无法识别。运行 help 查看用法。");
    var address = options.TryGetValue("--device", out var raw) ? AdbClient.NormalizeAddress(raw) : "";
    var adb = new AdbClient(options.GetValueOrDefault("--adb", "adb"), address);
    var package = options.GetValueOrDefault("--package", "com.limelight");
    AdbClient.ValidatePackage(package);
    if (command == "doctor")
    {
        var version = await adb.RunAsync(cancellation.Token, "version");
        Print(new { ok = true, adb = version, sunshineRunning = System.Diagnostics.Process.GetProcessesByName("Sunshine").Length > 0,
            deviceConfigured = address.Length > 0 });
    }
    else
    {
        if (address.Length == 0) throw new DeviceException("device_required", "此命令需要 --device 指定 Fire TV IP。");
        await adb.ConnectAsync(cancellation.Token);
        if (command == "launch") await adb.LaunchAsync(package, cancellation.Token);
        if (command == "home") await adb.HomeAsync(package, cancellation.Token);
        Print(new { ok = true, command, device = await adb.InspectAsync(package, cancellation.Token), streaming = "unknown" });
    }
    return 0;
}
catch (DeviceException error) { Print(new { ok = false, code = error.Code, message = error.Message }); return 1; }
catch (OperationCanceledException) { Print(new { ok = false, code = "cancelled", message = "操作已取消。" }); return 2; }
catch (Exception error) { Print(new { ok = false, code = "unexpected_error", message = error.Message }); return 1; }
