use crate::{Worker, command};
use sofalink_core::*;
use std::{
    io::Read,
    process::Stdio,
    thread,
    time::{Duration, Instant},
};
impl Worker {
    fn steam_helper(
        &self,
        action: &str,
        window: isize,
        token: Option<Token>,
    ) -> Result<serde_json::Value, String> {
        let path = self.config.root.join("scripts/steam-session.ps1");
        let mut child = command("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(path)
            .args(["-Action", action, "-Window", &window.to_string()])
            .arg("-StateFile")
            .arg(self.config.root.join("local/steam-session.json"))
            .args(["-OwnerPid", &std::process::id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(if action == "Start" { 28 } else { 7 });
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                let mut out = Vec::new();
                let mut err = Vec::new();
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_end(&mut out);
                }
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_end(&mut err);
                }
                if !status.success() {
                    let _ =
                        std::fs::write(self.config.root.join("local/steam-session.stderr"), &err);
                    return Err(if action == "Start" {
                        "无法进入 Steam 大屏幕，请在电脑确认 Steam 已安装并登录"
                    } else {
                        "Steam 大屏幕退出未确认，请处理电脑上的提示后重试清理"
                    }
                    .into());
                }
                return serde_json::from_str(String::from_utf8_lossy(&out).trim())
                    .map_err(|e| format!("Steam evidence invalid: {e}"));
            }
            if Instant::now() >= deadline
                || (action == "Start" && token.is_some_and(|t| !self.active(t)))
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Steam operation cancelled or timed out".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn application(&self, token: Token, status: ApplicationStatus, message: &str) {
        self.emit(
            token,
            EventKind::Application(ApplicationState {
                target: ApplicationTarget::SteamBigPicture,
                status,
                message: message.into(),
            }),
        );
    }
    pub(super) fn start_steam(&mut self, token: Token) -> Result<(), String> {
        self.application(token, ApplicationStatus::Starting, "正在进入 Steam 大屏幕");
        let result = match self.steam_helper("Start", 0, Some(token)) {
            Ok(result) => result,
            Err(error) => {
                self.load_steam_ownership()?;
                return Err(error);
            }
        };
        self.steam_window = Some(result["window"].as_i64().ok_or("Steam window missing")? as isize);
        self.steam_window_owner = Some(
            result["window_pid"]
                .as_u64()
                .ok_or("Steam window owner missing")? as u32,
        );
        self.config.input_target = self.steam_window;
        self.application(token, ApplicationStatus::Ready, "Steam 大屏幕已就绪");
        Ok(())
    }
    pub(super) fn stop_steam(&mut self, token: Option<Token>) -> Result<(), String> {
        if !self.config.steam_big_picture {
            return Ok(());
        }
        let owned = self.steam_window.is_some();
        if let Some(window) = self.steam_window {
            if let Some(t) = token {
                self.application(t, ApplicationStatus::Stopping, "正在退出 Steam 大屏幕");
            }
            if let Err(error) = self.steam_helper("Stop", window, None) {
                if let Some(t) = token {
                    self.application(t, ApplicationStatus::Error, &error);
                }
                return Err(error);
            }
            self.steam_window = None;
            self.steam_window_owner = None;
            self.config.input_target = None;
        }
        if let Some(t) = token {
            self.application(
                t,
                ApplicationStatus::Inactive,
                if owned {
                    "Steam 大屏幕已退出"
                } else {
                    "本次未启动 Steam 大屏幕"
                },
            );
        }
        Ok(())
    }
    fn load_steam_ownership(&mut self) -> Result<(), String> {
        let file = self.config.root.join("local/steam-session.json");
        if file.is_file() {
            let raw = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
            let value: serde_json::Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
                .map_err(|e| e.to_string())?;
            self.steam_window = Some(
                value["window"]
                    .as_i64()
                    .ok_or("Invalid Steam ownership record")? as isize,
            );
        }
        Ok(())
    }
    pub(super) fn recover_steam(&mut self, token: Token) -> Result<(), String> {
        self.load_steam_ownership()?;
        self.stop_steam(Some(token))
    }
}
