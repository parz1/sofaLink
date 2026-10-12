use crate::{Scenario, SimResources, Simulator};
use serde::Serialize;
use sofalink_core::*;

#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub scenario: Scenario,
    pub phases: Vec<Phase>,
    pub recovered: bool,
    pub retried: bool,
    pub final_snapshot: Snapshot,
}

/// Uses the exact core/backend runtime used by the TUI, with a virtual clock.
pub fn check(scenario: Scenario) -> Result<CheckReport, String> {
    let mut runtime = Runtime::new(Simulator::new(scenario));
    runtime
        .command(
            Command::Start {
                activity: ActivityKind::DesktopStream,
            },
            0,
        )
        .map_err(|e| e.to_string())?;
    let mut phases = vec![Phase::Idle, Phase::Checking];
    let mut recovered = false;
    let mut retried = false;
    let mut played_since = None;
    let mut stopped = false;
    for now in (50..=20_000).step_by(50) {
        runtime.advance(now);
        let snapshot = runtime.snapshot();
        let phase = snapshot.phase;
        if phases.last() != Some(&phase) {
            phases.push(phase);
        }
        if phase == Phase::Playing {
            played_since.get_or_insert(now);
        }
        if phase == Phase::Degraded && !recovered {
            runtime
                .command(Command::Recover, now)
                .map_err(|e| e.to_string())?;
            recovered = true;
            played_since = None;
        } else if phase == Phase::Faulted {
            if !snapshot.resources.clear() && !recovered {
                runtime
                    .command(Command::Recover, now)
                    .map_err(|e| e.to_string())?;
                recovered = true;
            } else if snapshot.resources.clear() && !retried {
                runtime
                    .command(Command::Retry, now)
                    .map_err(|e| e.to_string())?;
                retried = true;
            } else {
                return Err(format!(
                    "{scenario:?}: unexpected fault: {}",
                    snapshot.message
                ));
            }
        } else if phase == Phase::Playing && !stopped {
            let wait_for_fault = matches!(
                scenario,
                Scenario::AudioStall | Scenario::VideoStall | Scenario::Disconnect
            ) && !recovered;
            if !wait_for_fault && played_since.is_some_and(|since| now - since >= 1_000) {
                runtime
                    .command(Command::Stop, now)
                    .map_err(|e| e.to_string())?;
                stopped = true;
            }
        } else if phase == Phase::Idle && stopped {
            if runtime.backend().resources() != SimResources::default() {
                return Err("simulated resource leaked".into());
            }
            if !phases.contains(&Phase::Playing) {
                return Err("playback was never proven".into());
            }
            if matches!(
                scenario,
                Scenario::AudioStall | Scenario::VideoStall | Scenario::Disconnect
            ) && !recovered
            {
                return Err("recovery was not exercised".into());
            }
            if matches!(scenario, Scenario::StartupFail | Scenario::StartupTimeout) && !retried {
                return Err("retry was not exercised".into());
            }
            if scenario == Scenario::StopFail && !recovered {
                return Err("cleanup recovery was not exercised".into());
            }
            return Ok(CheckReport {
                scenario,
                phases,
                recovered,
                retried,
                final_snapshot: runtime.snapshot().clone(),
            });
        }
        let phase = runtime.snapshot().phase;
        if phases.last() != Some(&phase) {
            phases.push(phase);
        }
    }
    Err(format!(
        "{scenario:?}: did not finish within virtual deadline"
    ))
}
