use sofalink_core::*;
fn resources(
    display: ResourceState,
    media: ResourceState,
    game_window: ResourceState,
) -> SpaceResources {
    SpaceResources {
        display,
        media,
        game_window,
        driver_installed: true,
    }
}
fn idle() -> GameSpaceCore {
    let mut core = GameSpaceCore::default();
    let effect = core.initialize();
    core.complete(
        effect,
        resources(
            ResourceState::Stopped,
            ResourceState::Stopped,
            ResourceState::Stopped,
        ),
        None,
    );
    core
}
#[test]
fn cleanup_stops_media_restores_window_then_reclaims_display() {
    let mut core = idle();
    let create = core.command(SpaceCommand::Start).unwrap().unwrap();
    let start = core
        .complete(
            create,
            resources(
                ResourceState::Running,
                ResourceState::Stopped,
                ResourceState::Stopped,
            ),
            None,
        )
        .unwrap();
    core.complete(
        start,
        resources(
            ResourceState::Running,
            ResourceState::Running,
            ResourceState::Running,
        ),
        None,
    );
    assert_eq!(core.snapshot().phase, SpacePhase::Streaming);
    let stop = core.command(SpaceCommand::Stop).unwrap().unwrap();
    assert_eq!(stop.operation, SpaceOperation::StopMedia);
    let restore = core
        .complete(
            stop,
            resources(
                ResourceState::Running,
                ResourceState::Stopped,
                ResourceState::Running,
            ),
            None,
        )
        .unwrap();
    assert_eq!(restore.operation, SpaceOperation::RestoreGame);
    let remove = core
        .complete(
            restore,
            resources(
                ResourceState::Running,
                ResourceState::Stopped,
                ResourceState::Stopped,
            ),
            None,
        )
        .unwrap();
    assert_eq!(remove.operation, SpaceOperation::RemoveDisplay);
    core.complete(
        remove,
        resources(
            ResourceState::Stopped,
            ResourceState::Stopped,
            ResourceState::Stopped,
        ),
        None,
    );
    assert_eq!(core.snapshot().phase, SpacePhase::Idle);
}
#[test]
fn cancellation_waits_for_inflight_creation_and_cleans_late_resource() {
    let mut core = idle();
    let create = core.command(SpaceCommand::Start).unwrap().unwrap();
    assert!(core.command(SpaceCommand::Stop).unwrap().is_none());
    assert!(core.snapshot().available_commands.is_empty());
    let cleanup = core
        .complete(
            create,
            resources(
                ResourceState::Running,
                ResourceState::Stopped,
                ResourceState::Stopped,
            ),
            None,
        )
        .unwrap();
    assert_eq!(cleanup.operation, SpaceOperation::RemoveDisplay);
    assert!(
        core.complete(create, SpaceResources::default(), Some("stale".into()))
            .is_none()
    );
    assert_eq!(core.snapshot().pending, Some(cleanup));
}
#[test]
fn restore_failure_blocks_removal_and_allows_recovery() {
    let mut core = GameSpaceCore::default();
    let inspect = core.initialize();
    core.complete(
        inspect,
        resources(
            ResourceState::Running,
            ResourceState::Stopped,
            ResourceState::Unknown,
        ),
        None,
    );
    let restore = core.command(SpaceCommand::Recover).unwrap().unwrap();
    assert_eq!(restore.operation, SpaceOperation::RestoreGame);
    core.complete(
        restore,
        resources(
            ResourceState::Running,
            ResourceState::Stopped,
            ResourceState::Unknown,
        ),
        Some("window unconfirmed".into()),
    );
    assert_eq!(
        core.snapshot().available_commands,
        vec![SpaceCommand::Recover]
    );
    assert!(core.command(SpaceCommand::Uninstall).is_err());
}

#[test]
fn failed_readiness_stops_owned_startup_before_removing_display() {
    let mut core = idle();
    let create = core.command(SpaceCommand::Start).unwrap().unwrap();
    let start = core
        .complete(
            create,
            resources(
                ResourceState::Running,
                ResourceState::Stopped,
                ResourceState::Stopped,
            ),
            None,
        )
        .unwrap();
    let stop = core
        .complete(
            start,
            resources(
                ResourceState::Running,
                ResourceState::Running,
                ResourceState::Unknown,
            ),
            Some("TV readiness timed out".into()),
        )
        .unwrap();
    assert_eq!(stop.operation, SpaceOperation::StopMedia);
    assert!(
        !core
            .snapshot()
            .available_commands
            .contains(&SpaceCommand::Start)
    );
    let restore = core
        .complete(
            stop,
            resources(
                ResourceState::Running,
                ResourceState::Stopped,
                ResourceState::Unknown,
            ),
            None,
        )
        .unwrap();
    assert_eq!(restore.operation, SpaceOperation::RestoreGame);
}

#[test]
fn cancelled_authorization_with_no_resources_can_retry() {
    let mut core = idle();
    let create = core.command(SpaceCommand::Start).unwrap().unwrap();
    core.complete(
        create,
        resources(
            ResourceState::Stopped,
            ResourceState::Stopped,
            ResourceState::Stopped,
        ),
        Some("UAC cancelled".into()),
    );
    assert_eq!(core.snapshot().phase, SpacePhase::Faulted);
    assert!(core.snapshot().resources.clear());
    assert!(core.command(SpaceCommand::Start).unwrap().is_some());
}

#[test]
fn uninstall_requires_evidence_driver_package_is_gone() {
    let mut core = idle();
    let uninstall = core.command(SpaceCommand::Uninstall).unwrap().unwrap();
    core.complete(
        uninstall,
        resources(
            ResourceState::Stopped,
            ResourceState::Stopped,
            ResourceState::Stopped,
        ),
        None,
    );
    assert_eq!(core.snapshot().phase, SpacePhase::Faulted);
    assert!(core.snapshot().resources.driver_installed);
    let retry = core.command(SpaceCommand::Uninstall).unwrap().unwrap();
    let mut gone = resources(
        ResourceState::Stopped,
        ResourceState::Stopped,
        ResourceState::Stopped,
    );
    gone.driver_installed = false;
    core.complete(retry, gone, None);
    assert_eq!(core.snapshot().phase, SpacePhase::Idle);
    assert!(
        !core
            .snapshot()
            .available_commands
            .contains(&SpaceCommand::Uninstall)
    );
}
