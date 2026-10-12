use sofalink_core::*;
use sofalink_simulator::Scenario;
use sofalink_tui::{App, primary_label, render_text};

fn screen(app: &App, width: u16, height: u16) -> String {
    render_text(app, width, height).unwrap()
}
fn advance(app: &mut App, start: u64, end: u64) {
    for now in (start..=end).step_by(50) {
        app.advance(now);
    }
}

#[test]
fn same_commands_drive_start_stop_and_clear_playback_metrics() {
    let mut app = App::new(Scenario::Healthy);
    assert_eq!(primary_label(app.runtime.snapshot()), "开始");
    app.primary(0);
    assert_eq!(primary_label(app.runtime.snapshot()), "取消");
    advance(&mut app, 50, 1_800);
    assert_eq!(app.runtime.snapshot().phase, Phase::Playing);
    assert_eq!(primary_label(app.runtime.snapshot()), "结束");
    let playing = screen(&app, 90, 25);
    assert!(playing.contains("正在播放"));
    assert!(playing.contains("合成测试数据"));
    app.primary(2_000);
    assert_eq!(primary_label(app.runtime.snapshot()), "等待清理确认…");
    advance(&mut app, 2_050, 2_500);
    assert_eq!(app.runtime.snapshot().phase, Phase::Idle);
    let idle = screen(&app, 90, 25);
    assert!(!idle.contains("60.0 fps"));
    assert!(idle.contains("已释放"));
}

#[test]
fn anomaly_and_recovery_are_visible_in_the_actual_rendered_screen() {
    let mut app = App::new(Scenario::AudioStall);
    app.primary(0);
    advance(&mut app, 50, 4_500);
    assert_eq!(app.runtime.snapshot().phase, Phase::Degraded);
    let degraded = screen(&app, 90, 25);
    assert!(degraded.contains("播放异常"));
    assert!(degraded.contains("声音播放进度停止"));
    assert!(degraded.contains("恢复连接"));
    app.primary(4_600);
    advance(&mut app, 4_650, 6_000);
    assert_eq!(app.runtime.snapshot().phase, Phase::Playing);
    assert!(screen(&app, 90, 25).contains("正在播放"));
}

#[test]
fn quitting_waits_for_cleanup_including_a_failed_first_attempt() {
    let mut app = App::new(Scenario::StopFail);
    app.primary(0);
    advance(&mut app, 50, 1_800);
    app.request_exit(2_000);
    assert!(!app.can_exit());
    advance(&mut app, 2_050, 2_500);
    assert!(!app.can_exit());
    assert_eq!(app.runtime.snapshot().phase, Phase::Stopping);
    advance(&mut app, 2_550, 3_100);
    assert!(app.can_exit());
    assert!(app.runtime.snapshot().resources.clear());
}

#[test]
fn compact_terminal_preserves_status_and_actions_without_panicking() {
    let app = App::new(Scenario::Healthy);
    let small = screen(&app, 45, 10);
    assert!(small.contains("准备就绪"));
    assert!(small.contains("开始"));
    screen(&app, 1, 1);
    screen(&app, 56, 21);
}

#[test]
fn scenario_change_is_rejected_during_a_session() {
    let mut app = App::new(Scenario::Healthy);
    app.primary(0);
    app.select_scenario(3);
    assert_eq!(app.scenario, Scenario::Healthy);
    assert_eq!(app.runtime.snapshot().phase, Phase::Checking);
}

#[test]
fn host_mute_is_confirmed_and_restore_is_visible_on_anomaly_and_stop() {
    let mut app = App::with_simulated_host_mute(Scenario::AudioStall, true);
    app.primary(0);
    advance(&mut app, 50, 1800);
    assert_eq!(
        app.runtime.snapshot().host_audio.status,
        HostAudioStatus::Muted
    );
    assert!(screen(&app, 90, 25).contains("电脑已静音"));
    advance(&mut app, 1850, 4500);
    assert_eq!(app.runtime.snapshot().phase, Phase::Degraded);
    assert_eq!(
        app.runtime.snapshot().host_audio.status,
        HostAudioStatus::Original
    );
    assert!(screen(&app, 90, 25).contains("已恢复电脑原静音状态"));
    app.primary(4600);
    advance(&mut app, 4650, 6000);
    assert_eq!(
        app.runtime.snapshot().host_audio.status,
        HostAudioStatus::Muted
    );
    app.command(Command::Stop, 6100);
    assert!(screen(&app, 90, 25).contains("正在恢复电脑原静音状态"));
    advance(&mut app, 6150, 6700);
    assert_eq!(app.runtime.snapshot().phase, Phase::Idle);
    assert_eq!(
        app.runtime.snapshot().host_audio.status,
        HostAudioStatus::Original
    );
}
