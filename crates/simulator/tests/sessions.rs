use sofalink_core::*;
use sofalink_simulator::*;

#[test]
fn every_scenario_finishes_without_leaked_resources() {
    for scenario in Scenario::ALL {
        let report = check(scenario).unwrap();
        assert_eq!(report.final_snapshot.phase, Phase::Idle);
        assert!(report.final_snapshot.resources.clear());
    }
}

#[test]
fn cancel_at_each_startup_stage_prevents_late_playback() {
    for cancel_at in [0, 300, 650, 1_000, 1_250] {
        let mut runtime = Runtime::new(Simulator::new(Scenario::Healthy));
        runtime
            .command(
                Command::Start {
                    activity: ActivityKind::DesktopStream,
                },
                0,
            )
            .unwrap();
        for now in (50..=cancel_at).step_by(50) {
            runtime.advance(now);
        }
        runtime.command(Command::Stop, cancel_at).unwrap();
        for now in ((cancel_at + 50)..=(cancel_at + 6_000)).step_by(50) {
            runtime.advance(now);
        }
        assert_eq!(
            runtime.snapshot().phase,
            Phase::Idle,
            "cancel at {cancel_at}"
        );
        assert_eq!(runtime.backend().resources(), SimResources::default());
        assert!(runtime.snapshot().metrics.is_none());
    }
}

#[test]
fn repeated_sessions_use_new_ids_and_release_resources() {
    let mut runtime = Runtime::new(Simulator::new(Scenario::Healthy));
    for session in 1..=20 {
        let start = (session - 1) * 3_000;
        runtime
            .command(
                Command::Start {
                    activity: ActivityKind::DesktopStream,
                },
                start,
            )
            .unwrap();
        for now in ((start + 50)..=(start + 1_800)).step_by(50) {
            runtime.advance(now);
        }
        assert_eq!(runtime.snapshot().phase, Phase::Playing);
        assert_eq!(runtime.snapshot().token.session, session);
        runtime.command(Command::Stop, start + 2_000).unwrap();
        for now in ((start + 2_050)..=(start + 2_500)).step_by(50) {
            runtime.advance(now);
        }
        assert_eq!(runtime.snapshot().phase, Phase::Idle);
        assert_eq!(runtime.backend().resources(), SimResources::default());
    }
}

#[test]
fn serialized_commands_and_snapshots_share_the_desktop_contract() {
    let mut runtime = Runtime::new(Simulator::new(Scenario::Healthy));
    let command: Command =
        serde_json::from_str(r#"{"command":"start","activity":"desktop_stream"}"#).unwrap();
    runtime.command(command, 0).unwrap();
    let snapshot = serde_json::to_string(runtime.snapshot()).unwrap();
    let restored: Snapshot = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(&restored, runtime.snapshot());
    assert_eq!(restored.schema_version, 1);
    assert_eq!(restored.available_commands, vec![Command::Stop]);
}
