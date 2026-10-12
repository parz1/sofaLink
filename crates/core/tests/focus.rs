use sofalink_core::*;
fn window(handle: u64) -> WindowRef {
    WindowRef {
        handle,
        process_id: handle as u32,
        process_created: 10,
    }
}
fn observation(sequence: u64, foreground: WindowRef, work: bool) -> FocusObservation {
    FocusObservation {
        sequence,
        foreground: Some(foreground),
        game_alive: true,
        game_in_space: true,
        work_alive: work,
        work_candidate: if foreground == window(2) {
            Some(foreground)
        } else {
            None
        },
    }
}
#[test]
fn switch_requires_actual_evidence_and_restores_work() {
    let mut core = FocusCore::default();
    core.attach(window(1));
    core.observe(observation(1, window(2), false), 0);
    let effect = core.command(FocusCommand::ReturnGame, 1).unwrap();
    assert_eq!(core.snapshot().owner, FocusOwner::Computer);
    assert_eq!(core.snapshot().pending, Some(effect));
    core.observe(observation(2, window(1), true), 2);
    assert_eq!(core.snapshot().owner, FocusOwner::Game);
    assert!(core.snapshot().pending.is_none());
    assert_eq!(
        core.command(FocusCommand::TakeComputer, 3).unwrap().target,
        window(2)
    );
}
#[test]
fn stale_failure_and_observation_cannot_override_new_request() {
    let mut core = FocusCore::default();
    core.attach(window(1));
    core.observe(observation(1, window(2), false), 0);
    let old = core.command(FocusCommand::ReturnGame, 1).unwrap();
    core.failed(old.request, "denied".into());
    let new = core.command(FocusCommand::ReturnGame, 2).unwrap();
    core.failed(old.request, "late".into());
    core.observe(observation(1, window(1), true), 3);
    assert_eq!(core.snapshot().pending, Some(new));
    core.tick(802);
    assert_eq!(core.snapshot().owner, FocusOwner::Computer);
    assert!(core.snapshot().issue.is_some());
}
#[test]
fn expired_work_and_game_disable_actions() {
    let mut core = FocusCore::default();
    core.attach(window(1));
    core.observe(observation(1, window(2), false), 0);
    core.observe(observation(2, window(1), false), 1);
    assert!(core.command(FocusCommand::TakeComputer, 2).is_err());
    let mut gone = observation(3, window(2), false);
    gone.game_alive = false;
    core.observe(gone, 3);
    assert!(core.snapshot().available_commands.is_empty());
}

#[test]
fn placement_can_be_repaired_without_claiming_game_focus() {
    let mut core = FocusCore::default();
    core.attach(window(1));
    let mut outside = observation(1, window(2), false);
    outside.game_in_space = false;
    core.observe(outside, 0);
    let effect = core.command(FocusCommand::MoveGameToSpace, 1).unwrap();
    assert_eq!(effect.target, window(1));
    core.observe(observation(2, window(2), true), 2);
    assert!(core.snapshot().pending.is_none());
    assert_eq!(core.snapshot().owner, FocusOwner::Computer);
    assert!(core.snapshot().game_in_space);
}

#[test]
fn foreground_alone_does_not_confirm_return_to_game_space() {
    let mut core = FocusCore::default();
    core.attach(window(1));
    core.observe(observation(1, window(2), false), 0);
    core.command(FocusCommand::ReturnGame, 1).unwrap();
    let mut misplaced = observation(2, window(1), true);
    misplaced.game_in_space = false;
    core.observe(misplaced, 2);
    assert!(core.snapshot().pending.is_some());
    core.tick(801);
    assert_eq!(core.snapshot().owner, FocusOwner::Game);
    assert!(core.snapshot().issue.is_some());
    assert!(
        core.snapshot()
            .available_commands
            .contains(&FocusCommand::MoveGameToSpace)
    );
}
