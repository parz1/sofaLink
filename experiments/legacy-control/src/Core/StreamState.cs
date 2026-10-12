namespace LivingroomHub.Core;

public enum StreamPhase { Idle, Checking, StartingHost, StartingClient, WaitingMedia, Playing, Degraded, Stopping, Faulted }
public sealed record StreamEvidence(string SessionId, DateTimeOffset At, long Rendered, long AudioPlayed,
    bool VideoHealthy, bool AudioHealthy, double Fps, double RenderMs, double AudioBufferMs, string AudioError);
public sealed record StreamView(StreamPhase Phase, string Message, StreamEvidence? Evidence = null, string LogPath = "")
{
    public bool Active => Phase is not (StreamPhase.Idle or StreamPhase.Faulted);
    public string Action => Phase switch
    {
        StreamPhase.Idle => "开始", StreamPhase.Faulted => "重试",
        StreamPhase.Stopping => "正在结束…", StreamPhase.Playing or StreamPhase.Degraded => "结束", _ => "取消"
    };
}

// A process existing or an ADB launch succeeding is not proof of playback.
public sealed class StreamHealth(string sessionId)
{
    private StreamEvidence? previous;
    public StreamEvidence? Latest { get; private set; }
    public bool HasPlayed { get; private set; }
    private bool progressing;
    public void Accept(StreamEvidence evidence)
    {
        if (evidence.SessionId != sessionId || (Latest is not null && evidence.At <= Latest.At)) return;
        progressing = previous is not null && evidence.Rendered > previous.Rendered && evidence.AudioPlayed > previous.AudioPlayed;
        previous = Latest = evidence;
        if (progressing && evidence.VideoHealthy && evidence.AudioHealthy) HasPlayed = true;
    }
    public StreamPhase Phase(DateTimeOffset now)
    {
        var healthy = Latest is not null && now - Latest.At < TimeSpan.FromSeconds(3) &&
            progressing && Latest.VideoHealthy && Latest.AudioHealthy && Latest.AudioError.Length == 0;
        return healthy ? StreamPhase.Playing : HasPlayed ? StreamPhase.Degraded : StreamPhase.WaitingMedia;
    }
    public string Reason(DateTimeOffset now)
    {
        if (Latest is null) return "等待 Fire TV 的音画反馈";
        if (now - Latest.At >= TimeSpan.FromSeconds(3)) return "客户端反馈中断，请检查电视端或网络";
        if (Latest.AudioError.Length > 0) return "声音异常：" + Latest.AudioError;
        if (!Latest.VideoHealthy) return "等待画面实际呈现";
        if (!Latest.AudioHealthy) return "等待声音实际播放（静音内容也会验证播放进度）";
        if (!progressing) return "音画播放进度停止";
        return "音画正在播放";
    }
}
