//! Windows + Android capability I/O. Platform names never enter core negotiation rules.
use crate::{Worker, command};
use sofalink_core::*;
use std::{
    process::Stdio,
    time::{Duration, Instant},
};

impl Worker {
    pub(super) fn probe_media(&mut self, token: Token) -> Result<MediaNegotiation, String> {
        let request = &self.config.stream_request;
        request.validate().map_err(str::to_owned)?;
        let json = serde_json::to_string(request).map_err(|e| e.to_string())?;
        // Temporary output allocation is owned even when discovery fails or is cancelled.
        self.probe_client_owned = true;
        let client: DeviceCapabilities = if let Some(remote) = self.config.remote.clone() {
            let id = self.session(token);
            remote.publish(crate::remote::RemoteAction::Probe {
                id: id.clone(),
                request: request.clone(),
            });
            let deadline = Instant::now() + Duration::from_secs(8);
            loop {
                if let Some(error) = remote.probe_error(&id) {
                    return Err(format!("电视能力探测失败：{error}"));
                }
                if let Some(caps) = remote.capabilities(&id) {
                    break caps;
                }
                if !self.active(token) || Instant::now() >= deadline {
                    return Err("电视能力探测超时或取消".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        } else {
            let (ok, reply) = self.adb(
                &[
                    "shell",
                    "content",
                    "call",
                    "--uri",
                    "content://dev.livingroom.streamlab.capabilities",
                    "--method",
                    "media",
                    "--arg",
                    &format!("'{json}'"),
                ],
                Some(token),
            )?;
            let json = reply
                .trim()
                .strip_prefix("Result: Bundle[{capabilities=")
                .and_then(|s| s.strip_suffix("}]"))
                .filter(|_| ok)
                .ok_or("客户端能力合同不兼容，请更新 APK")?;
            let client: DeviceCapabilities = serde_json::from_str(json)
                .map_err(|e| format!("Invalid client capabilities: {e}"))?;
            client
        };
        let directory = self.config.root.join("local/sessions");
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let prefix = directory.join(format!("{}-probe", self.session(token)));
        let request_path = prefix.with_extension("request.json");
        let output_path = prefix.with_extension("host.json");
        let error_path = prefix.with_extension("stderr");
        std::fs::write(
            &request_path,
            serde_json::to_vec(request).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let mut child = command(&self.config.python)
            .current_dir(&self.config.root)
            .env("PYTHONIOENCODING", "utf-8")
            .arg(self.config.root.join("experiments/stream/probe-host.py"))
            .arg("--request")
            .arg(&request_path)
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(&output_path).map_err(|e| e.to_string())?)
            .stderr(std::fs::File::create(&error_path).map_err(|e| e.to_string())?)
            .spawn()
            .map_err(|e| format!("Windows capability probe failed to start: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                if !status.success() {
                    return Err(format!(
                        "Windows capability probe failed: {}",
                        std::fs::read_to_string(&error_path).unwrap_or_default()
                    ));
                }
                break;
            }
            if !self.active(token) || Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Windows capability probe cancelled or timed out".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let host: DeviceCapabilities =
            serde_json::from_slice(&std::fs::read(output_path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Invalid host capabilities: {e}"))?;
        let plan =
            negotiate_media(request, &host, &client).map_err(|e| format!("音视频协商失败：{e}"))?;
        Ok(MediaNegotiation { host, client, plan })
    }
}
