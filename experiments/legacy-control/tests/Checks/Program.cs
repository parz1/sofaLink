using LivingroomHub.Adapters.FireTv;
using LivingroomHub.Core;

// The test executable doubles as an adb fixture; every response crosses a real process boundary.
if (args.Length > 0)
{
    var scenario = Environment.GetEnvironmentVariable("HUB_CHECK_SCENARIO");
    var stateFile = Environment.GetEnvironmentVariable("HUB_CHECK_STATE")!;
    if (args[0] == "connect") Console.WriteLine("connected");
    else if (args[0] == "devices") Console.WriteLine("List of devices attached\n" +
        (scenario == "absent" ? "" : "192.168.1.50:5555\t" + (scenario == "unauthorized" ? "unauthorized" : "device")));
    else if (args.Contains("getprop")) Console.WriteLine("fixture");
    else if (args.Contains("pm")) Console.WriteLine(scenario == "missing" ? "" : "package:com.limelight");
    else if (args.Contains("resolve-activity")) Console.WriteLine("com.limelight/.MainActivity");
    else if (args.Contains("am")) File.WriteAllText(stateFile, "com.limelight");
    else if (args.Contains("input")) File.WriteAllText(stateFile, "com.amazon.launcher");
    else if (args.Contains("dumpsys")) Console.WriteLine(scenario == "unknown" ? "unrecognized activity output" :
        "mResumedActivity: ActivityRecord{123 u0 " + File.ReadAllText(stateFile) + "/.MainActivity t1}");
    else return 1;
    return 0;
}

var state = Path.GetTempFileName();
var clock=DateTimeOffset.UtcNow;
var health=new StreamHealth("active");
StreamEvidence Evidence(string id,int tick,long video,long audio,bool sound=true) =>
    new(id,clock.AddSeconds(tick),video,audio,true,sound,60,65,30,"");
health.Accept(Evidence("old",0,100,48000));
if(health.Latest is not null)throw new Exception("Old session evidence accepted");
health.Accept(Evidence("active",0,100,0,false));
health.Accept(Evidence("active",1,160,0,false));
if(health.Phase(clock.AddSeconds(1))!=StreamPhase.WaitingMedia)throw new Exception("Missing audio became Playing");
health.Accept(Evidence("active",2,220,48000));
if(health.Phase(clock.AddSeconds(2))!=StreamPhase.Playing)throw new Exception("Playback not recognized");
if(health.Phase(clock.AddSeconds(6))!=StreamPhase.Degraded)throw new Exception("Stale feedback remained healthy");
health.Accept(Evidence("active",7,220,48000));
if(health.Phase(clock.AddSeconds(7))!=StreamPhase.Degraded)throw new Exception("Stopped counters remained healthy");
health.Accept(Evidence("active",8,280,96000));
if(health.Phase(clock.AddSeconds(8))!=StreamPhase.Playing)throw new Exception("Recovery failed");
health.Accept(Evidence("active",1,1,1));
if(health.Latest!.Rendered!=280)throw new Exception("Out-of-order evidence accepted");
Console.WriteLine("PASS: stream states, wrong session, missing audio, stale feedback, stalled counters, recovery");
Environment.SetEnvironmentVariable("HUB_CHECK_STATE", state);
var client = new AdbClient(Environment.ProcessPath!, AdbClient.NormalizeAddress("192.168.1.50"));
async Task ExpectError(string code, Func<Task> action)
{
    try { await action(); throw new Exception("Expected " + code); }
    catch (DeviceException e) when (e.Code == code) { }
}
try
{
    File.WriteAllText(state, "com.amazon.launcher");
    await client.ConnectAsync(default);
    await client.LaunchAsync("com.limelight", default);
    if (await client.IsForegroundAsync("com.limelight", default) != true) throw new Exception("Launch failed");
    await client.HomeAsync("com.limelight", default);
    if (await client.IsForegroundAsync("com.limelight", default) != false) throw new Exception("Home failed");
    foreach (var scenario in new[] { "absent", "unauthorized" })
    {
        Environment.SetEnvironmentVariable("HUB_CHECK_SCENARIO", scenario);
        await ExpectError("device_not_ready", () => client.ConnectAsync(default));
    }
    Environment.SetEnvironmentVariable("HUB_CHECK_SCENARIO", "missing");
    await ExpectError("moonlight_missing", async () => { await client.InspectAsync("com.limelight", default); });
    Environment.SetEnvironmentVariable("HUB_CHECK_SCENARIO", "unknown");
    if (await client.IsForegroundAsync("com.limelight", default) is not null) throw new Exception("Unknown must remain unknown");
    await ExpectError("invalid_address", () => { AdbClient.NormalizeAddress("192.168.1.50;echo"); return Task.CompletedTask; });
    await ExpectError("invalid_package", () => { AdbClient.ValidatePackage("com.test;echo"); return Task.CompletedTask; });
    Console.WriteLine("PASS: launch, home, unauthorized, absent, missing package, unknown foreground, argument validation");
    return 0;
}
finally { File.Delete(state); }
