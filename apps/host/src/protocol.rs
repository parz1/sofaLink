use serde::{Deserialize, Serialize};
use sofalink_core::{Command, DeviceCapabilities, Token};
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u8,
    pub op: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub pair_code: String,
    #[serde(default)]
    pub client_session: String,
    #[serde(default)]
    pub client_running: bool,
    #[serde(default)]
    pub client_stopped_session: String,
    #[serde(default)]
    pub probe: Option<(String, DeviceCapabilities)>,
    #[serde(default)]
    pub probe_complete: String,
    #[serde(default)]
    pub probe_error: String,
    #[serde(default)]
    pub command: Option<Command>,
    #[serde(default)]
    pub expected_token: Option<Token>,
    #[serde(default)]
    pub expected_instance: String,
}
impl Request {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || !matches!(self.op.as_str(), "pair" | "poll" | "command")
            || self.device_id.is_empty()
            || self.device_id.len() > 64
            || self.key.len() > 64
            || self.client_session.len() > 128
        {
            return Err("Invalid control request".into());
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
struct Credential {
    version: u8,
    device_id: String,
    key: String,
}
pub struct Pairing {
    path: PathBuf,
    credential: Option<Credential>,
    code: String,
    issued: Instant,
    attempts: HashMap<String, u8>,
}
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}
impl Pairing {
    pub fn load(path: PathBuf, reset: bool) -> Result<Self, String> {
        let credential = if path.is_file() && !reset {
            Some(
                serde_json::from_slice::<Credential>(
                    &std::fs::read(&path).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        if credential.as_ref().is_some_and(|c| {
            c.version != 1
                || c.key.len() != 64
                || !c.key.bytes().all(|b| b.is_ascii_hexdigit())
                || c.device_id.is_empty()
                || c.device_id.len() > 64
        }) {
            return Err("Invalid saved pairing; reset pairing explicitly".into());
        }
        if reset && path.is_file() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        let secret = sofalink_input::random_secret()?;
        let code = format!(
            "{:06}",
            u64::from_str_radix(&secret[..12], 16).map_err(|e| e.to_string())? % 1000000
        );
        Ok(Self {
            path,
            credential,
            code,
            issued: Instant::now(),
            attempts: HashMap::new(),
        })
    }
    pub fn is_unpaired(&self) -> bool {
        self.credential.is_none()
    }
    pub fn code(&self) -> &str {
        &self.code
    }
    pub fn authorize(&self, r: &Request) -> Result<(), String> {
        if self
            .credential
            .as_ref()
            .is_some_and(|c| c.version == 1 && c.device_id == r.device_id && same(&c.key, &r.key))
        {
            Ok(())
        } else {
            Err("Pairing required".into())
        }
    }
    pub fn pair(&mut self, r: &Request, peer: &str) -> Result<String, String> {
        if self.credential.is_some() || self.issued.elapsed() > Duration::from_secs(300) {
            return Err("Pairing closed; restart host with --reset-pairing if needed".into());
        }
        if self.attempts.len() >= 32 && !self.attempts.contains_key(peer) {
            return Err("Pairing attempt limit".into());
        }
        let count = self.attempts.entry(peer.into()).or_default();
        if *count >= 5 {
            return Err("Pairing attempt limit".into());
        }
        *count += 1;
        if !same(&self.code, &r.pair_code) {
            return Err("Incorrect pairing code".into());
        }
        let key = sofalink_input::random_secret()?;
        let c = Credential {
            version: 1,
            device_id: r.device_id.clone(),
            key: key.clone(),
        };
        let temporary = self.path.with_extension("tmp");
        std::fs::write(
            &temporary,
            serde_json::to_vec(&c).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(temporary, &self.path).map_err(|e| e.to_string())?;
        self.credential = Some(c);
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        serde_json::from_value(serde_json::json!({"version":1,"op":"pair","device_id":"test-tv"}))
            .unwrap()
    }
    #[test]
    fn pairing_is_bounded_and_survives_restart_without_plaintext_code() {
        let path = std::env::temp_dir().join(format!(
            "sofalink-pairing-{}.json",
            sofalink_input::random_secret().unwrap()
        ));
        let mut pair = Pairing::load(path.clone(), false).unwrap();
        let mut r = request();
        r.pair_code = "wrong".into();
        for _ in 0..5 {
            assert!(pair.pair(&r, "attacker").is_err());
        }
        r.pair_code = pair.code().into();
        assert!(pair.pair(&r, "attacker").is_err());
        r.key = pair.pair(&r, "tv").unwrap();
        assert_eq!(r.key.len(), 64);
        pair.authorize(&r).unwrap();
        assert!(pair.pair(&r, "tv").is_err());
        let loaded = Pairing::load(path.clone(), false).unwrap();
        loaded.authorize(&r).unwrap();
        r.device_id = "other".into();
        assert!(loaded.authorize(&r).is_err());
        r.device_id = "test-tv".into();
        r.key = "forged".into();
        assert!(loaded.authorize(&r).is_err());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn expired_or_unknown_contract_cannot_pair() {
        let path = std::env::temp_dir().join(format!(
            "sofalink-expired-{}.json",
            sofalink_input::random_secret().unwrap()
        ));
        let mut pair = Pairing::load(path, false).unwrap();
        pair.issued = Instant::now() - Duration::from_secs(301);
        let mut r = request();
        r.pair_code = pair.code().into();
        assert!(pair.pair(&r, "tv").is_err());
        r.version = 2;
        assert!(r.validate().is_err());
        assert!(
            serde_json::from_value::<Request>(
                serde_json::json!({"version":1,"op":"poll","device_id":"tv","execute":"arbitrary"})
            )
            .is_err()
        );
    }
}
