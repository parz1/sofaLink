use sofalink_core::*;

#[test]
fn performance_feedback_is_independent_fresh_and_session_scoped() {
    let mut core = Core::default();
    let token = start_to_waiting(&mut core);
    let p = ClientPerformance {
        sequence: 10,
        frames: 120.0,
        decode_p95_ms: 40.0,
        ..Default::default()
    };
    core.event(
        Event {
            token,
            kind: EventKind::ClientPerformance(p.clone()),
        },
        400,
    );
    assert_eq!(core.snapshot().phase, Phase::WaitingMedia);
    assert_eq!(
        core.snapshot().performance.message,
        "卡顿线索：解码输出等待"
    );
    let mut replay = p.clone();
    replay.decode_p95_ms = 0.0;
    core.event(
        Event {
            token,
            kind: EventKind::ClientPerformance(replay),
        },
        500,
    );
    assert_eq!(
        core.snapshot()
            .performance
            .client
            .as_ref()
            .unwrap()
            .decode_p95_ms,
        40.0
    );
    core.command(Command::Stop, 600).unwrap();
    core.event(
        Event {
            token,
            kind: EventKind::ClientPerformance(p),
        },
        700,
    );
    assert!(core.snapshot().performance.client.is_none());
}

#[test]
fn input_failure_does_not_fake_media_failure_and_recovery_invalidates_old_input() {
    let mut core = Core::default();
    let token = start_to_waiting(&mut core);
    core.event(
        Event {
            token,
            kind: EventKind::Media(sample(1, 60, 48000)),
        },
        400,
    );
    core.event(
        Event {
            token,
            kind: EventKind::Media(sample(2, 120, 96000)),
        },
        500,
    );
    core.event(
        Event {
            token,
            kind: EventKind::Input(InputState::new(InputStatus::Ready, "ready", 2)),
        },
        550,
    );
    assert_eq!(core.snapshot().primary_command(), Some(Command::Stop));
    core.event(
        Event {
            token,
            kind: EventKind::Input(InputState::new(InputStatus::Disconnected, "released", 4)),
        },
        600,
    );
    assert_eq!(core.snapshot().phase, Phase::Playing);
    assert_eq!(core.snapshot().video, SignalState::Healthy);
    assert_eq!(core.snapshot().primary_command(), Some(Command::Recover));
    core.command(Command::Recover, 650).unwrap();
    core.event(
        Event {
            token,
            kind: EventKind::Input(InputState::new(InputStatus::Ready, "old", 99)),
        },
        700,
    );
    assert_eq!(core.snapshot().input.status, InputStatus::Unavailable);
}

fn sample(sequence: u64, video: u64, audio: u64) -> MediaSample {
    MediaSample {
        sequence,
        rendered_frames: video,
        audio_played_frames: audio,
        video_fps: 60.0,
        receive_to_render_ms: 65.0,
        audio_buffer_ms: 24.0,
    }
}
fn start_to_waiting(core: &mut Core) -> Token {
    core.command(
        Command::Start {
            activity: ActivityKind::DesktopStream,
        },
        0,
    )
    .unwrap();
    let token = core.snapshot().token;
    for (now, kind) in [
        (100, EventKind::Checked),
        (200, EventKind::HostReady),
        (300, EventKind::ClientReady),
    ] {
        core.event(Event { token, kind }, now);
    }
    token
}

#[test]
fn device_timeout_budget_is_owned_by_core_and_expires_at_configured_deadline() {
    let mut core = Core::with_timeouts(Timeouts {
        startup_ms: 20_000,
        feedback_ms: 2_000,
        recovery_ms: 20_000,
        cleanup_ms: 8_000,
    });
    core.command(
        Command::Start {
            activity: ActivityKind::DesktopStream,
        },
        0,
    )
    .unwrap();
    assert!(core.tick(5_000).is_empty());
    assert_eq!(core.snapshot().phase, Phase::Checking);
    let effects = core.tick(20_000);
    assert_eq!(effects[0].kind, EffectKind::Cleanup);
    assert_eq!(core.snapshot().phase, Phase::Stopping);
    core.tick(27_999);
    assert_eq!(core.snapshot().phase, Phase::Stopping);
    core.tick(28_000);
    assert_eq!(core.snapshot().phase, Phase::Faulted);
}

#[test]
fn actual_process_exit_updates_resources_and_late_metrics_cannot_restore_playback() {
    for (loss, code) in [
        (EventKind::HostExited, IssueCode::HostExited),
        (EventKind::ClientExited, IssueCode::ClientExited),
    ] {
        let mut core = Core::default();
        let token = start_to_waiting(&mut core);
        core.event(
            Event {
                token,
                kind: EventKind::Media(sample(1, 60, 48000)),
            },
            400,
        );
        core.event(
            Event {
                token,
                kind: EventKind::Media(sample(2, 120, 96000)),
            },
            600,
        );
        core.event(Event { token, kind: loss }, 700);
        assert_eq!(core.snapshot().phase, Phase::Degraded);
        let resources = core.snapshot().resources;
        assert!(
            resources.host == ResourceState::Stopped || resources.client == ResourceState::Stopped
        );
        core.event(
            Event {
                token,
                kind: EventKind::Media(sample(3, 180, 144000)),
            },
            800,
        );
        core.tick(3000);
        assert_eq!(core.snapshot().phase, Phase::Degraded);
        assert_eq!(core.snapshot().issue.as_ref().unwrap().code, code);
        assert!(core.snapshot().can(Command::Recover));
        let effects = core.command(Command::Recover, 3100).unwrap();
        assert_eq!(effects[0].kind, EffectKind::RestartMedia);
        assert_ne!(effects[0].token, token);
    }
}
fn observe(core: &mut Core, token: Token, now: u64, sample: MediaSample) {
    core.event(
        Event {
            token,
            kind: EventKind::Media(sample),
        },
        now,
    );
}

#[test]
fn ready_processes_and_one_sample_do_not_prove_playback() {
    let mut core = Core::default();
    let token = start_to_waiting(&mut core);
    assert_eq!(core.snapshot().phase, Phase::WaitingMedia);
    observe(&mut core, token, 400, sample(1, 60, 48_000));
    assert_eq!(core.snapshot().phase, Phase::WaitingMedia);
    observe(&mut core, token, 500, sample(2, 120, 48_000));
    assert_ne!(core.snapshot().phase, Phase::Playing);
    observe(&mut core, token, 600, sample(3, 180, 96_000));
    assert_eq!(core.snapshot().phase, Phase::Playing);
}

#[test]
fn duplicate_start_and_unsupported_activity_cannot_acquire_resources() {
    let mut core = Core::default();
    assert_eq!(
        core.command(
            Command::Start {
                activity: ActivityKind::MediaPlayback
            },
            0
        ),
        Err(CommandError::UnsupportedActivity)
    );
    assert_eq!(core.snapshot().phase, Phase::Idle);
    start_to_waiting(&mut core);
    let before = core.snapshot().clone();
    assert_eq!(
        core.command(
            Command::Start {
                activity: ActivityKind::DesktopStream
            },
            400
        ),
        Err(CommandError::NotAvailable)
    );
    assert_eq!(*core.snapshot(), before);
}

#[test]
fn old_session_generation_and_replayed_samples_are_ignored() {
    let mut core = Core::default();
    let old = start_to_waiting(&mut core);
    observe(&mut core, old, 400, sample(1, 60, 48_000));
    observe(&mut core, old, 500, sample(2, 120, 96_000));
    let before = core.snapshot().clone();
    observe(&mut core, old, 600, sample(1, 500, 500_000));
    observe(
        &mut core,
        Token {
            session: 99,
            generation: 0,
        },
        700,
        sample(3, 500, 500_000),
    );
    assert_eq!(*core.snapshot(), before);
    core.command(Command::Stop, 800).unwrap();
    core.event(
        Event {
            token: old,
            kind: EventKind::CleanupResult {
                host_stopped: true,
                client_stopped: true,
            },
        },
        900,
    );
    observe(&mut core, old, 1_000, sample(3, 500, 500_000));
    assert_eq!(core.snapshot().phase, Phase::Stopping);
}

#[test]
fn stale_feedback_and_stalled_audio_degrade_then_recover() {
    let mut core = Core::default();
    let token = start_to_waiting(&mut core);
    observe(&mut core, token, 400, sample(1, 60, 48_000));
    observe(&mut core, token, 500, sample(2, 120, 96_000));
    core.tick(2_000);
    assert_eq!(core.snapshot().phase, Phase::Degraded);
    assert_eq!(core.snapshot().video, SignalState::Missing);
    observe(&mut core, token, 2_100, sample(3, 180, 96_000));
    assert_eq!(
        core.snapshot().issue.as_ref().unwrap().code,
        IssueCode::AudioStalled
    );
    observe(&mut core, token, 2_200, sample(4, 240, 144_000));
    assert_eq!(core.snapshot().phase, Phase::Playing);
}

#[test]
fn cleanup_failure_prevents_start_until_cleanup_is_confirmed() {
    let mut core = Core::default();
    start_to_waiting(&mut core);
    core.command(Command::Stop, 400).unwrap();
    let token = core.snapshot().token;
    core.event(
        Event {
            token,
            kind: EventKind::CleanupResult {
                host_stopped: false,
                client_stopped: true,
            },
        },
        500,
    );
    assert_eq!(core.snapshot().phase, Phase::Faulted);
    assert!(!core.snapshot().resources.clear());
    assert_eq!(core.snapshot().primary_command(), Some(Command::Recover));
    assert_eq!(
        core.command(Command::Retry, 600),
        Err(CommandError::NotAvailable)
    );
    core.command(Command::Recover, 700).unwrap();
    let token = core.snapshot().token;
    core.event(
        Event {
            token,
            kind: EventKind::CleanupResult {
                host_stopped: true,
                client_stopped: true,
            },
        },
        800,
    );
    assert_eq!(core.snapshot().phase, Phase::Idle);
}

#[test]
fn startup_and_cleanup_timeouts_never_report_idle_or_playing() {
    let mut core = Core::default();
    core.command(
        Command::Start {
            activity: ActivityKind::DesktopStream,
        },
        0,
    )
    .unwrap();
    let effects = core.tick(STARTUP_TIMEOUT_MS);
    assert_eq!(effects[0].kind, EffectKind::Cleanup);
    assert_eq!(core.snapshot().phase, Phase::Stopping);
    core.tick(STARTUP_TIMEOUT_MS + CLEANUP_TIMEOUT_MS);
    assert_eq!(core.snapshot().phase, Phase::Faulted);
    assert_eq!(
        core.snapshot().issue.as_ref().unwrap().code,
        IssueCode::CleanupFailed
    );
}

#[test]
fn malformed_metrics_and_backward_time_are_rejected() {
    let mut core = Core::default();
    let token = start_to_waiting(&mut core);
    let before = core.snapshot().clone();
    let mut bad = sample(1, 60, 48_000);
    bad.video_fps = f64::NAN;
    observe(&mut core, token, 400, bad);
    assert_eq!(*core.snapshot(), before);
    assert_eq!(
        core.command(Command::Stop, 200),
        Err(CommandError::ClockWentBackward)
    );
}

#[test]
fn recovery_invalidates_old_feedback_and_requires_fresh_progress() {
    let mut core = Core::default();
    let old = start_to_waiting(&mut core);
    observe(&mut core, old, 400, sample(1, 60, 48_000));
    observe(&mut core, old, 500, sample(2, 120, 96_000));
    core.tick(2_000);
    core.command(Command::Recover, 2_100).unwrap();
    observe(&mut core, old, 2_200, sample(3, 180, 144_000));
    assert_eq!(core.snapshot().phase, Phase::Recovering);
    let token = core.snapshot().token;
    core.event(
        Event {
            token,
            kind: EventKind::Recovered,
        },
        2_300,
    );
    observe(&mut core, token, 2_400, sample(1, 60, 48_000));
    assert_eq!(core.snapshot().phase, Phase::WaitingMedia);
    observe(&mut core, token, 2_500, sample(2, 120, 96_000));
    assert_eq!(core.snapshot().phase, Phase::Playing);
}
