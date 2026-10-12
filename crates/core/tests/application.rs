use sofalink_core::*;
fn event(core: &mut Core, kind: EventKind, now: u64) {
    core.event(
        Event {
            token: core.snapshot().token,
            kind,
        },
        now,
    );
}
fn app(status: ApplicationStatus) -> EventKind {
    EventKind::Application(ApplicationState {
        target: ApplicationTarget::SteamBigPicture,
        status,
        message: "evidence".into(),
    })
}
#[test]
fn target_exit_during_stream_start_ends_through_core_cleanup() {
    let mut core = Core::default().with_application_target(ApplicationTarget::SteamBigPicture);
    core.command(
        Command::Start {
            activity: ActivityKind::DesktopStream,
        },
        0,
    )
    .unwrap();
    event(&mut core, EventKind::Checked, 1);
    event(&mut core, app(ApplicationStatus::Ready), 2);
    event(&mut core, EventKind::HostReady, 3);
    event(&mut core, EventKind::ClientReady, 4);
    let effects = core.event(
        Event {
            token: core.snapshot().token,
            kind: app(ApplicationStatus::Inactive),
        },
        5,
    );
    assert_eq!(core.snapshot().phase, Phase::Stopping);
    assert!(effects.iter().any(|e| e.kind == EffectKind::Cleanup));
    event(
        &mut core,
        EventKind::CleanupResult {
            host_stopped: true,
            client_stopped: true,
        },
        6,
    );
    assert_eq!(core.snapshot().phase, Phase::Idle);
}
#[test]
fn target_readiness_and_cleanup_are_real_evidence_not_host_process_readiness() {
    let mut core = Core::default().with_application_target(ApplicationTarget::SteamBigPicture);
    core.command(
        Command::Start {
            activity: ActivityKind::DesktopStream,
        },
        0,
    )
    .unwrap();
    event(&mut core, EventKind::Checked, 1);
    event(&mut core, EventKind::HostReady, 2);
    assert_eq!(core.snapshot().phase, Phase::StartingHost);
    core.event(
        Event {
            token: Token::default(),
            kind: app(ApplicationStatus::Ready),
        },
        3,
    );
    event(&mut core, EventKind::HostReady, 4);
    assert_eq!(core.snapshot().phase, Phase::StartingHost);
    event(&mut core, app(ApplicationStatus::Ready), 5);
    event(&mut core, EventKind::HostReady, 6);
    assert_eq!(core.snapshot().phase, Phase::StartingClient);
    core.command(Command::Stop, 7).unwrap();
    event(
        &mut core,
        EventKind::CleanupResult {
            host_stopped: true,
            client_stopped: true,
        },
        8,
    );
    assert_eq!(core.snapshot().phase, Phase::Faulted);
    assert_eq!(core.snapshot().resources.host, ResourceState::Unknown);
    assert!(!core.snapshot().can(Command::Retry));
    core.command(Command::Recover, 9).unwrap();
    event(&mut core, app(ApplicationStatus::Inactive), 10);
    event(
        &mut core,
        EventKind::CleanupResult {
            host_stopped: true,
            client_stopped: true,
        },
        11,
    );
    assert_eq!(core.snapshot().phase, Phase::Idle);
    event(&mut core, app(ApplicationStatus::Ready), 12);
    assert_eq!(
        core.snapshot().application.status,
        ApplicationStatus::Inactive
    );
}
