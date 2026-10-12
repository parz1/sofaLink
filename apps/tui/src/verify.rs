//! Opt-in integration acceptance against the dedicated Android TV AVD.
use crate::{App, render_text};
use sofalink_core::*;
use std::{
    process::Command as ProcessCommand,
    thread,
    time::{Duration, Instant},
};

pub fn run(app: &mut App) -> Result<(), String> {
    if !app.real_device {
        return Err("verify requires the device backend".into());
    }
    let clock = Instant::now();
    let mut phases = vec![];
    let mut stage = 0;
    let mut played_since = None;
    let mut healthy_samples = 0;
    let mut last_sequence = 0;
    let mute_enabled = app.runtime.snapshot().host_audio.enabled;
    let mut last_host_audio = app.runtime.snapshot().host_audio.clone();
    let mut mute_confirmations = 0;
    app.primary(0);
    let result = (|| {
        while clock.elapsed() < Duration::from_secs(80) {
            let now = clock.elapsed().as_millis() as TimeMs;
            app.advance(now);
            let snapshot = app.runtime.snapshot().clone();
            let phase_changed = phases.last() != Some(&snapshot.phase);
            let audio_changed = last_host_audio != snapshot.host_audio;
            if phase_changed {
                phases.push(snapshot.phase);
            }
            if audio_changed {
                if snapshot.host_audio.status == HostAudioStatus::Muted {
                    mute_confirmations += 1;
                }
                last_host_audio = snapshot.host_audio.clone();
            }
            if phase_changed || audio_changed {
                println!(
                    "{}",
                    serde_json::to_string(&snapshot).map_err(|e| e.to_string())?
                );
                let screen = render_text(app, 90, 25).map_err(|e| e.to_string())?;
                if screen.contains("合成测试数据") || screen.contains("模拟呈现") {
                    return Err("设备 TUI 错误标注为模拟数据".into());
                }
            }
            if snapshot.phase == Phase::Faulted {
                return Err(format!("验收失败：{}", snapshot.message));
            }
            if snapshot.phase == Phase::Playing {
                let since = *played_since.get_or_insert(now);
                if let Some(metrics) = &snapshot.metrics
                    && metrics.sequence != last_sequence
                {
                    healthy_samples += 1;
                    last_sequence = metrics.sequence;
                }
                if stage == 0 && now - since >= 8_000 {
                    // Actual process termination: no injected core event or synthetic metric.
                    let adb = std::path::PathBuf::from(
                        std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA unavailable")?,
                    )
                    .join("Android/Sdk/platform-tools/adb.exe");
                    let mut cmd = ProcessCommand::new(adb);
                    #[cfg(windows)]
                    {
                        use std::os::windows::process::CommandExt;
                        cmd.creation_flags(0x08000000);
                    }
                    let status = cmd
                        .args([
                            "-s",
                            match app.runtime.backend() {
                                crate::app::AppBackend::Emulator(backend) => &backend.serial,
                                _ => return Err("device backend unavailable".into()),
                            },
                            "shell",
                            "am",
                            "force-stop",
                            "dev.livingroom.streamlab",
                        ])
                        .status()
                        .map_err(|e| e.to_string())?;
                    if !status.success() {
                        return Err("无法中断测试客户端".into());
                    }
                    stage = 1;
                    played_since = None;
                } else if stage == 2 && now - since >= 8_000 {
                    if let crate::app::AppBackend::Emulator(backend) = app.runtime.backend() {
                        backend.terminate_owned_host()?;
                    }
                    stage = 3;
                    played_since = None;
                } else if stage == 4 && now - since >= 8_000 {
                    app.primary(now);
                    stage = 5;
                }
            } else {
                played_since = None;
            }
            if stage == 1 && snapshot.phase == Phase::Degraded {
                if !matches!(
                    snapshot.issue.as_ref().map(|i| i.code),
                    Some(IssueCode::FeedbackExpired | IssueCode::ClientExited)
                ) {
                    return Err("异常未由真实反馈中断识别".into());
                }
                app.primary(now);
                stage = 2;
            }
            if stage == 3 && snapshot.phase == Phase::Degraded {
                if snapshot.issue.as_ref().map(|i| i.code) != Some(IssueCode::HostExited)
                    || snapshot.resources.host != ResourceState::Stopped
                {
                    return Err("媒体主机退出未被正确确认".into());
                }
                app.primary(now);
                stage = 4;
            }
            if stage == 5 && snapshot.phase == Phase::Idle {
                if !snapshot.resources.clear() || healthy_samples < 20 {
                    return Err("播放证据或清理确认不足".into());
                }
                app.primary(now);
                stage = 6;
            }
            // HostReady has confirmed a real pipeline before cancellation is requested.
            if stage == 6 && snapshot.phase == Phase::StartingClient {
                app.command(Command::Stop, now);
                stage = 7;
            }
            if stage == 7 && snapshot.phase == Phase::Idle {
                if !snapshot.resources.clear() {
                    return Err("启动取消后的清理未确认".into());
                }
                if mute_enabled
                    && (mute_confirmations < 3
                        || snapshot.host_audio.status != HostAudioStatus::Original)
                {
                    return Err("电脑静音 / 恢复的实际确认不足".into());
                }
                println!(
                    "PASS: actual audio/video, client and host interruption, recovery, confirmed stop, startup cancellation; {healthy_samples} healthy samples; {mute_confirmations} confirmed host mutes"
                );
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err("设备验收超时".into())
    })();
    app.request_exit(clock.elapsed().as_millis() as TimeMs);
    let cleanup = Instant::now();
    while !app.can_exit() && cleanup.elapsed() < Duration::from_secs(15) {
        app.advance(clock.elapsed().as_millis() as TimeMs);
        thread::sleep(Duration::from_millis(50));
    }
    if !app.can_exit() {
        return Err(format!("{:?}；退出清理未确认", result.err()));
    }
    result
}
