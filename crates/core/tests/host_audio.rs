use sofalink_core::*;

fn waiting() -> (Core, Token) {
    let mut core = Core::with_host_mute(Timeouts::default(), true);
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
        assert!(
            core.event(Event { token, kind }, now)
                .iter()
                .all(|e| !matches!(e.kind, EffectKind::HostAudio { .. }))
        );
    }
    (core, token)
}
fn media(core: &mut Core, token: Token, now: u64, sequence: u64, frames: u64) -> Vec<Effect> {
    core.event(
        Event {
            token,
            kind: EventKind::Media(MediaSample {
                sequence,
                rendered_frames: frames,
                audio_played_frames: frames * 800,
                video_fps: 60.0,
                receive_to_render_ms: 50.0,
                audio_buffer_ms: 20.0,
            }),
        },
        now,
    )
}
#[test]
fn actual_playback_triggers_mute_and_stale_confirmation_cannot_override_restore() {
    let (mut core, token) = waiting();
    assert!(media(&mut core, token, 400, 1, 60).is_empty());
    assert_eq!(core.snapshot().host_audio.status, HostAudioStatus::Original);
    let effects = media(&mut core, token, 500, 2, 120);
    assert_eq!(
        effects[0].kind,
        EffectKind::HostAudio {
            mute: true,
            request: 1
        }
    );
    assert_eq!(core.snapshot().host_audio.status, HostAudioStatus::Muting);
    let restore = media(&mut core, token, 600, 3, 120);
    assert_eq!(
        restore[0].kind,
        EffectKind::HostAudio {
            mute: false,
            request: 2
        }
    );
    core.event(
        Event {
            token,
            kind: EventKind::HostAudio {
                request: 1,
                status: HostAudioStatus::Muted,
                message: "late".into(),
            },
        },
        650,
    );
    assert_eq!(
        core.snapshot().host_audio.status,
        HostAudioStatus::Restoring
    );
    core.event(
        Event {
            token,
            kind: EventKind::HostAudio {
                request: 2,
                status: HostAudioStatus::Original,
                message: "restored".into(),
            },
        },
        700,
    );
    assert_eq!(core.snapshot().host_audio.status, HostAudioStatus::Original);
    assert_eq!(
        media(&mut core, token, 800, 4, 180)[0].kind,
        EffectKind::HostAudio {
            mute: true,
            request: 3
        }
    );
}
#[test]
fn lost_feedback_requests_restore_once_and_cleanup_failure_never_claims_restored() {
    let (mut core, token) = waiting();
    media(&mut core, token, 400, 1, 60);
    media(&mut core, token, 500, 2, 120);
    assert_eq!(
        core.tick(2000)[0].kind,
        EffectKind::HostAudio {
            mute: false,
            request: 2
        }
    );
    assert!(core.tick(2100).is_empty());
    core.command(Command::Stop, 2200).unwrap();
    let cleanup = core.snapshot().token;
    core.event(
        Event {
            token: cleanup,
            kind: EventKind::CleanupResult {
                host_stopped: false,
                client_stopped: true,
            },
        },
        2300,
    );
    assert_eq!(core.snapshot().phase, Phase::Faulted);
    assert_eq!(core.snapshot().host_audio.status, HostAudioStatus::Error);
    core.command(Command::Recover, 2400).unwrap();
    core.event(
        Event {
            token: core.snapshot().token,
            kind: EventKind::CleanupResult {
                host_stopped: true,
                client_stopped: true,
            },
        },
        2500,
    );
    assert_eq!(core.snapshot().phase, Phase::Idle);
    assert_eq!(core.snapshot().host_audio.status, HostAudioStatus::Original);
}
#[test]
fn mute_failure_stops_session_and_old_generation_acknowledgement_is_ignored() {
    let (mut core, token) = waiting();
    media(&mut core, token, 400, 1, 60);
    media(&mut core, token, 500, 2, 120);
    let effects = core.event(
        Event {
            token,
            kind: EventKind::HostAudio {
                request: 1,
                status: HostAudioStatus::Error,
                message: "device unavailable".into(),
            },
        },
        600,
    );
    assert_eq!(effects[0].kind, EffectKind::Cleanup);
    assert_eq!(core.snapshot().phase, Phase::Stopping);
    assert_eq!(
        core.snapshot().issue.as_ref().unwrap().code,
        IssueCode::HostAudioFailed
    );
    core.event(
        Event {
            token,
            kind: EventKind::HostAudio {
                request: 1,
                status: HostAudioStatus::Muted,
                message: "old".into(),
            },
        },
        700,
    );
    assert_eq!(
        core.snapshot().host_audio.status,
        HostAudioStatus::Restoring
    );
}
