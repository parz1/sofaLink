//! Authenticated TV control transport supplies evidence; core retains lifecycle decisions.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sofalink_core::{DeviceCapabilities, StreamRequest};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteAction {
    Probe { id: String, request: StreamRequest },
    Start { session: String, extras: Value },
    Stop { session: String },
}

#[derive(Clone, Default)]
pub struct RemoteBridge(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    probe_complete: String,
    probe_error: String,
    action: Option<RemoteAction>,
    capabilities: Option<(String, DeviceCapabilities)>,
    client_session: String,
    client_running: bool,
    stopped_session: String,
    seen: Option<Instant>,
}
impl RemoteBridge {
    pub fn action(&self) -> Option<RemoteAction> {
        self.0.lock().expect("remote lock").action.clone()
    }
    pub fn publish(&self, action: RemoteAction) {
        let mut s = self.0.lock().expect("remote lock");
        s.capabilities = None;
        s.action = Some(action);
    }
    pub fn report(
        &self,
        session: &str,
        running: bool,
        stopped_session: &str,
        probe_complete: &str,
        probe_error: &str,
        probe: Option<(String, DeviceCapabilities)>,
    ) {
        let mut s = self.0.lock().expect("remote lock");
        s.seen = Some(Instant::now());
        s.client_session = session.into();
        s.client_running = running;
        s.stopped_session = stopped_session.into();
        if matches!(&s.action,Some(RemoteAction::Probe{id,..}) if id==probe_complete) {
            s.probe_complete = probe_complete.into();
            s.probe_error = probe_error.into();
        }
        if let Some((id, caps)) = probe
            && matches!(&s.action,Some(RemoteAction::Probe{id:expected,..}) if *expected==id)
        {
            s.capabilities = Some((id, caps));
        }
    }
    pub fn capabilities(&self, id: &str) -> Option<DeviceCapabilities> {
        let s = self.0.lock().expect("remote lock");
        s.capabilities
            .as_ref()
            .filter(|(key, _)| key == id)
            .map(|(_, caps)| caps.clone())
    }
    pub fn probe_finished(&self, id: &str) -> bool {
        self.0.lock().expect("remote lock").probe_complete == id
    }
    pub fn probe_error(&self, id: &str) -> Option<String> {
        let s = self.0.lock().expect("remote lock");
        if s.probe_complete == id && !s.probe_error.is_empty() {
            Some(s.probe_error.clone())
        } else {
            None
        }
    }
    pub fn running(&self, session: &str) -> bool {
        let s = self.0.lock().expect("remote lock");
        s.client_session == session
            && s.client_running
            && s.seen.is_some_and(|t| t.elapsed() < Duration::from_secs(3))
    }
    pub fn stopped(&self, session: &str) -> bool {
        let s = self.0.lock().expect("remote lock");
        s.stopped_session == session
            && !s.client_running
            && s.seen.is_some_and(|t| t.elapsed() < Duration::from_secs(3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_probe_ack_cannot_confirm_new_probe_cleanup() {
        let bridge = RemoteBridge::default();
        bridge.publish(RemoteAction::Probe {
            id: "new".into(),
            request: StreamRequest::default(),
        });
        bridge.report("", false, "", "old", "", None);
        assert!(!bridge.probe_finished("new"));
        bridge.report("", false, "", "new", "audio probe failed", None);
        assert!(bridge.probe_finished("new"));
        assert_eq!(
            bridge.probe_error("new").as_deref(),
            Some("audio probe failed")
        );
        bridge.publish(RemoteAction::Probe {
            id: "next".into(),
            request: StreamRequest::default(),
        });
        assert!(!bridge.probe_finished("next"));
        assert!(bridge.probe_error("next").is_none());
    }
    #[test]
    fn stop_requires_explicit_same_session_ack_not_merely_no_media() {
        let bridge = RemoteBridge::default();
        bridge.publish(RemoteAction::Start {
            session: "new".into(),
            extras: serde_json::json!({}),
        });
        bridge.report("new", false, "", "", "", None);
        assert!(!bridge.stopped("new"));
        bridge.publish(RemoteAction::Stop {
            session: "new".into(),
        });
        bridge.report("new", false, "old", "", "", None);
        assert!(!bridge.stopped("new"));
        bridge.report("new", false, "new", "", "", None);
        assert!(bridge.stopped("new"));
        bridge.report("new", true, "new", "", "", None);
        assert!(!bridge.stopped("new"));
    }
}
