//! Windows Android TV backend. Device I/O stays on a worker, outside core/UI.
mod media_adapter;
mod telemetry;
use sofalink_core::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command as ProcessCommand, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub use telemetry::ClientMetrics;

pub const SERIAL: &str = "emulator-5580";
const AVD: &str = "Livingroom_TV_API_36";
const PACKAGE: &str = "dev.livingroom.streamlab";

#[derive(Clone)]
pub struct Config {
    pub reference_video: bool,
    pub presentation_mode_override: Option<String>,
    pub stream_request: StreamRequest,
    pub serial: String,
    pub device_ip: String,
    pub host_ip: String,
    pub fire_tv: bool,
    pub root: PathBuf,
    pub adb: PathBuf,
    pub python: PathBuf,
    pub test_source: bool,
    pub test_audio: bool,
    pub exclude_audio_pid: Option<u32>,
    pub input_target: Option<isize>,
    pub diagnostics: bool,
    pub sync_video: bool,
    pub texture_video: bool,
    pub encoder_gop_override: Option<i32>,
    pub encoder_vbv_override: Option<u32>,
    pub audio_backend_override: Option<String>,
    pub virtual_audio: bool,
    pub mute_host: bool,
}
impl Config {
    pub fn local(root: PathBuf, test_source: bool) -> Result<Self, String> {
        let sdk = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA unavailable")?;
        let runtime_file = root.join("local/emulator-runtime.json");
        let mut exclude_audio_pid = None;
        let local_python = if runtime_file.is_file() {
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&runtime_file).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("Invalid emulator runtime config: {e}"))?;
            exclude_audio_pid = value["emulatorPid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
                .filter(|pid| *pid > 0);
            Some(PathBuf::from(
                value["python"]
                    .as_str()
                    .ok_or("emulator-runtime.json requires python path")?,
            ))
        } else {
            None
        };
        Ok(Self {
            stream_request: StreamRequest::default(),
            reference_video: false,
            presentation_mode_override: None,
            serial: SERIAL.into(),
            device_ip: "127.0.0.1".into(),
            host_ip: "10.0.2.2".into(),
            fire_tv: false,
            root,
            adb: PathBuf::from(sdk).join("Android/Sdk/platform-tools/adb.exe"),
            python: std::env::var_os("LIVINGROOM_PYTHON")
                .map(PathBuf::from)
                .or(local_python)
                .unwrap_or_else(|| "python.exe".into()),
            test_source,
            test_audio: false,
            exclude_audio_pid,
            input_target: std::env::var("LIVINGROOM_INPUT_WINDOW")
                .ok()
                .map(|v| v.parse::<isize>())
                .transpose()
                .map_err(|e| format!("Invalid input target: {e}"))?,
            diagnostics: false,
            sync_video: true,
            encoder_gop_override: None,
            encoder_vbv_override: None,
            audio_backend_override: None,
            // Physical display A/B only; direct SurfaceView remains the default.
            texture_video: match std::env::var("LIVINGROOM_TEXTURE_VIDEO").as_deref() {
                Ok("1") => true,
                Ok("0") | Err(_) => false,
                _ => return Err("LIVINGROOM_TEXTURE_VIDEO must be 0 or 1".into()),
            },
            virtual_audio: match std::env::var("LIVINGROOM_VIRTUAL_AUDIO").as_deref() {
                Ok("1") => true,
                Ok("0") | Err(_) => false,
                _ => return Err("LIVINGROOM_VIRTUAL_AUDIO must be 0 or 1".into()),
            },
            mute_host: !test_source
                && std::env::var("LIVINGROOM_HOST_MUTE").as_deref() != Ok("0")
                && std::env::var("LIVINGROOM_VIRTUAL_AUDIO").as_deref() != Ok("1"),
        })
    }
    pub fn fire_tv(root: PathBuf, device: &str, test_source: bool) -> Result<Self, String> {
        let ip: std::net::Ipv4Addr = device
            .strip_suffix(":5555")
            .unwrap_or(device)
            .parse()
            .map_err(|_| "Fire TV requires an IPv4 address")?;
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
        socket.connect((ip, 5004)).map_err(|e| e.to_string())?;
        let mut config = Self::local(root, test_source)?;
        config.serial = format!("{ip}:5555");
        config.device_ip = ip.to_string();
        config.host_ip = socket
            .local_addr()
            .map_err(|e| e.to_string())?
            .ip()
            .to_string();
        config.fire_tv = true;
        // Process loopback avoids driver-dependent endpoint loopback. Exclude
        // our controller when there is no emulator process to exclude.
        config.exclude_audio_pid.get_or_insert(std::process::id());
        Ok(config)
    }
    pub fn timeouts() -> Timeouts {
        Timeouts {
            startup_ms: 20_000,
            feedback_ms: 2_000,
            recovery_ms: 20_000,
            cleanup_ms: 8_000,
        }
    }
}

enum Work {
    Effect(Effect),
    TerminateHost(Token),
    Shutdown,
}
pub struct EmulatorBackend {
    request: StreamRequest,
    pub serial: String,
    tx: Sender<Work>,
    events: Receiver<Event>,
    latest: Arc<Mutex<Option<(Instant, Event)>>>,
    current: Arc<Mutex<Token>>,
    worker: Option<JoinHandle<()>>,
}
impl EmulatorBackend {
    /// Diagnostic fault injection terminates the owned child; core observes its real exit.
    pub fn terminate_owned_host(&self) -> Result<(), String> {
        let token = *self.current.lock().expect("token lock");
        self.tx
            .send(Work::TerminateHost(token))
            .map_err(|e| e.to_string())
    }
    pub fn new(config: Config) -> Result<Self, String> {
        config.stream_request.validate().map_err(str::to_owned)?;
        if !config.fire_tv && config.stream_request != StreamRequest::default() {
            return Err("模拟器兼容适配器暂时只验证了默认请求；非默认参数请用真实设备".into());
        }
        let request = config.stream_request.clone();
        let serial = config.serial.clone();
        if !cfg!(windows) {
            return Err("Android TV backend currently requires Windows".into());
        }
        let (tx, rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        let latest = Arc::new(Mutex::new(None));
        let current = Arc::new(Mutex::new(Token::default()));
        let mut worker = Worker {
            config,
            rx,
            events: event_tx,
            latest: latest.clone(),
            current: current.clone(),
            host: None,
            input: None,
            client_owned: false,
            probe_client_owned: false,
            ownership: None,
            client_missing: false,
            next_client_check: Instant::now(),
            reader: None,
            host_ready: Arc::new(AtomicBool::new(false)),
            nonce: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
                .to_string(),
            audio_session: None,
            mute_session: None,
            media_plan: None,
        };
        let handle = thread::spawn(move || worker.run());
        Ok(Self {
            serial,
            request,
            tx,
            events,
            latest,
            current,
            worker: Some(handle),
        })
    }
}
impl Backend for EmulatorBackend {
    fn stream_request(&self) -> StreamRequest {
        self.request.clone()
    }
    fn execute(&mut self, effect: Effect, _: TimeMs) {
        *self.current.lock().expect("token lock") = effect.token;
        self.latest.lock().expect("media lock").take();
        let _ = self.tx.send(Work::Effect(effect));
    }
    fn poll(&mut self, _: TimeMs) -> Vec<Event> {
        let token = *self.current.lock().expect("token lock");
        let mut events: Vec<_> = self
            .events
            .try_iter()
            .filter(|e| e.token == token)
            .collect();
        if let Some((received, event)) = self.latest.lock().expect("media lock").take()
            && event.token == token
            && received.elapsed() < Duration::from_millis(Config::timeouts().feedback_ms)
        {
            events.push(event);
        }
        events
    }
}
impl Drop for EmulatorBackend {
    fn drop(&mut self) {
        // Invalidate readers and any in-progress ADB command before final cleanup.
        self.current.lock().expect("token lock").generation += 1;
        let _ = self.tx.send(Work::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn command(exe: impl AsRef<std::ffi::OsStr>) -> ProcessCommand {
    let mut cmd = ProcessCommand::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}
struct Worker {
    config: Config,
    rx: Receiver<Work>,
    events: Sender<Event>,
    latest: Arc<Mutex<Option<(Instant, Event)>>>,
    current: Arc<Mutex<Token>>,
    host: Option<Child>,
    input: Option<sofalink_input::InputServer>,
    client_owned: bool,
    probe_client_owned: bool,
    ownership: Option<std::fs::File>,
    client_missing: bool,
    next_client_check: Instant,
    reader: Option<JoinHandle<()>>,
    host_ready: Arc<AtomicBool>,
    nonce: String,
    audio_session: Option<String>,
    mute_session: Option<String>,
    media_plan: Option<MediaPlan>,
}
impl Worker {
    fn audio_route(&self, action: &str, session: &str) -> Result<(), String> {
        self.audio_control("audio-route.ps1", action, session)
            .map(|_| ())
    }
    fn audio_control(
        &self,
        script_name: &str,
        action: &str,
        session: &str,
    ) -> Result<String, String> {
        // Rust canonicalize adds a verbatim prefix; Windows PowerShell 5 cannot
        // resolve $PSScriptRoot / Join-Path reliably for that spelling.
        let script = self.config.root.join("scripts").join(script_name);
        let script = script.to_string_lossy();
        let script = if let Some(unc) = script.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else {
            script.strip_prefix(r"\\?\").unwrap_or(&script).to_owned()
        };
        // The recovery guardian outlives this helper and can inherit pipe handles.
        // Files avoid waiting for EOF from that guardian after the helper exits.
        let log_path = self.config.root.join("local").join(format!(
            "audio-control-{script_name}-{session}-{action}.log"
        ));
        let log = std::fs::File::create(&log_path)
            .map_err(|e| format!("无法创建电脑声音操作日志：{e}"))?;
        let mut child = command("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(script)
            .args([
                "-Action",
                action,
                "-OwnerPid",
                &std::process::id().to_string(),
                "-Session",
                session,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|e| format!("音频设备切换工具启动失败：{e}"))?;
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                let output = std::fs::read(&log_path).unwrap_or_default();
                let output = String::from_utf8_lossy(&output).into_owned();
                return if status.success() {
                    Ok(output)
                } else {
                    Err(format!("电脑声音操作失败（{status}）：{output}"))
                };
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("电脑声音操作超时；请检查 local 中的恢复记录".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn active(&self, token: Token) -> bool {
        *self.current.lock().expect("token lock") == token
    }
    fn emit(&self, token: Token, kind: EventKind) {
        if self.active(token) {
            let _ = self.events.send(Event { token, kind });
        }
    }
    fn run(&mut self) {
        loop {
            match self.rx.recv_timeout(Duration::from_millis(20)) {
                Ok(Work::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Work::TerminateHost(token)) => {
                    if self.active(token)
                        && let Some(host) = self.host.as_mut()
                    {
                        let _ = host.kill();
                    }
                }
                Ok(Work::Effect(effect)) => {
                    if !self.active(effect.token) {
                        continue;
                    }
                    if let Err(error) = self.effect(effect) {
                        let code = if effect.kind == EffectKind::RestartMedia {
                            IssueCode::RecoveryFailed
                        } else {
                            IssueCode::StartupFailed
                        };
                        self.emit(effect.token, EventKind::Failed(Issue::new(code, error)));
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Some(host) = self.host.as_mut()
                && let Ok(Some(status)) = host.try_wait()
            {
                self.host.take();
                if let Some(mut input) = self.input.take() {
                    let _ = input.shutdown();
                }
                let token = *self.current.lock().expect("token lock");
                let _ = status;
                self.latest.lock().expect("media lock").take();
                self.emit(token, EventKind::HostExited);
            }
            if self.client_owned && !self.client_missing && Instant::now() >= self.next_client_check
            {
                self.next_client_check = Instant::now() + Duration::from_secs(1);
                let token = *self.current.lock().expect("token lock");
                if let Ok((false, pid)) = self.adb(&["shell", "pidof", PACKAGE], Some(token))
                    && pid.trim().is_empty()
                {
                    self.client_missing = true;
                    if let Some(mut input) = self.input.take() {
                        let _ = input.shutdown();
                    }
                    self.latest.lock().expect("media lock").take();
                    self.emit(token, EventKind::ClientExited);
                }
            }
        }
        let _ = self.cleanup(None);
    }
    fn adb(&self, args: &[&str], token: Option<Token>) -> Result<(bool, String), String> {
        let mut child = command(&self.config.adb)
            .args(["-s", &self.config.serial])
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("ADB 启动失败：{e}"))?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                let mut text = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut text);
                }
                if let Some(mut out) = child.stderr.take() {
                    let _ = out.read_to_string(&mut text);
                }
                return Ok((status.success(), text));
            }
            if Instant::now() >= deadline || token.is_some_and(|t| !self.active(t)) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("ADB 超时或操作已取消".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn confirm_tv(&self, token: Option<Token>) -> Result<(), String> {
        if self.config.fire_tv {
            let (ok, manufacturer) =
                self.adb(&["shell", "getprop", "ro.product.manufacturer"], token)?;
            let (model_ok, model) = self.adb(&["shell", "getprop", "ro.product.model"], token)?;
            if !ok
                || !model_ok
                || manufacturer.trim() != "Amazon"
                || !model.trim().starts_with("AFT")
            {
                return Err(format!("{} 未确认是 Amazon Fire TV", self.config.serial));
            }
            return Ok(());
        }
        let (ok, name) = self.adb(&["emu", "avd", "name"], token)?;
        if !ok || !name.lines().any(|line| line.trim() == AVD) {
            return Err(format!("{SERIAL} 不是项目的 Android TV 模拟器"));
        }
        Ok(())
    }
    fn client_start(&mut self, token: Token, session: &str) -> Result<(), String> {
        if !self.active(token) {
            return Err("操作已取消".into());
        }
        self.client_owned = true;
        let request = if self.config.fire_tv {
            &self
                .media_plan
                .as_ref()
                .ok_or("Media negotiation missing")?
                .request
        } else {
            &self.config.stream_request
        };
        self.client_missing = false;
        let (ok, error) = self.adb(&["shell", "am", "force-stop", PACKAGE], Some(token))?;
        if !ok {
            return Err(error);
        }
        let (ok, error) = self.adb(
            &[
                "shell",
                "am",
                "start",
                "-W",
                "-n",
                "dev.livingroom.streamlab/.MainActivity",
                "--es",
                "host",
                &self.config.host_ip,
                "--es",
                "transport",
                "tcp",
                "--es",
                "audioBackend",
                self.config
                    .audio_backend_override
                    .as_deref()
                    .unwrap_or("auto"),
                "--es",
                "presentationMode",
                self.config
                    .presentation_mode_override
                    .as_deref()
                    .unwrap_or("vsync"),
                "--es",
                if self.config.fire_tv {
                    "deviceMode"
                } else {
                    "decoder"
                },
                if self.config.fire_tv {
                    "fire-tv"
                } else {
                    "c2.goldfish.h264.decoder"
                },
                "--ez",
                "syncVideo",
                if self.config.sync_video {
                    "true"
                } else {
                    "false"
                },
                "--ez",
                "diagnostics",
                if self.config.diagnostics {
                    "true"
                } else {
                    "false"
                },
                "--ei",
                "videoQueueFrames",
                if self.config.fire_tv { "4" } else { "16" },
                "--es",
                "decoder",
                self.media_plan
                    .as_ref()
                    .map(|plan| plan.decoder.as_str())
                    .unwrap_or(if self.config.fire_tv {
                        ""
                    } else {
                        "c2.goldfish.h264.decoder"
                    }),
                "--ez",
                "textureVideo",
                if self.config.texture_video {
                    "true"
                } else {
                    "false"
                },
                "--ei",
                "videoWidth",
                &request.video.width.to_string(),
                "--ei",
                "videoHeight",
                &request.video.height.to_string(),
                "--ei",
                "videoFps",
                &request.video.fps.to_string(),
                "--ei",
                "audioRate",
                &request.audio.sample_rate.to_string(),
                "--ez",
                "requireLowLatencyAudio",
                if request.require_low_latency_audio
                    && self.config.audio_backend_override.is_none()
                    && self.config.fire_tv
                {
                    "true"
                } else {
                    "false"
                },
                "--es",
                "sessionId",
                session,
                "--ei",
                "inputPort",
                &self
                    .input
                    .as_ref()
                    .ok_or("Input server unavailable")?
                    .port
                    .to_string(),
                "--es",
                "inputKey",
                &self.input.as_ref().ok_or("Input server unavailable")?.key,
            ],
            Some(token),
        )?;
        if !ok || !error.contains("Status: ok") {
            return Err(format!("客户端启动失败：{error}"));
        }
        let (ok, pid) = self.adb(&["shell", "pidof", PACKAGE], Some(token))?;
        if !ok || pid.trim().is_empty() {
            return Err("客户端进程未确认".into());
        }
        self.next_client_check = Instant::now() + Duration::from_secs(1);
        Ok(())
    }
    fn session(&self, token: Token) -> String {
        format!("{}-{}-{}", self.nonce, token.session, token.generation)
    }
    fn host_start(&mut self, token: Token, session: &str) -> Result<(), String> {
        if self.host.is_some() {
            return Err("旧媒体主机尚未释放".into());
        }
        if !self.active(token) {
            return Err("操作已取消".into());
        }
        let events = self.events.clone();
        let current = self.current.clone();
        self.input = Some(sofalink_input::InputServer::start_on(
            if self.config.fire_tv {
                &self.config.host_ip
            } else {
                "127.0.0.1"
            },
            session.into(),
            self.config.input_target,
            move || *current.lock().expect("token lock") == token,
            move |input| {
                let _ = events.send(Event {
                    token,
                    kind: EventKind::Input(input),
                });
            },
        )?);
        std::fs::create_dir_all(self.config.root.join("local/sessions"))
            .map_err(|e| e.to_string())?;
        let error_file = std::fs::File::create(
            self.config
                .root
                .join(format!("local/sessions/{session}.stderr")),
        )
        .map_err(|e| e.to_string())?;
        let mut cmd = command(&self.config.python);
        cmd.current_dir(&self.config.root)
            .env("PYTHONIOENCODING", "utf-8")
            .arg("-u")
            .arg(self.config.root.join("experiments/stream/host.py"))
            // Goldfish alone needs its measured color compatibility profile;
            // real Fire TV uses normal SDR709 and hardware decoder discovery.
            .args([
                "--color-profile",
                if self.config.fire_tv {
                    "sdr709"
                } else {
                    "goldfish-compat"
                },
            ])
            .args([
                "--device",
                &self.config.device_ip,
                "--transport",
                "tcp",
                "--session-id",
                session,
                "--control-stdin",
                "--log",
            ])
            .arg(
                self.config
                    .root
                    .join(format!("local/sessions/{session}.jsonl")),
            );
        if self.config.test_source {
            cmd.args(["--test-pattern", "--test-tone"]);
        } else if self.config.test_audio {
            cmd.arg("--test-tone");
        } else if let Some(pid) = self.config.exclude_audio_pid {
            cmd.arg("--audio-exclude-pid").arg(pid.to_string());
        } else {
            return Err(
                "请先运行 scripts/tv-emulator.ps1，确认模拟器音频排除进程，避免回采客户端声音"
                    .into(),
            );
        }
        if self.config.reference_video {
            cmd.arg("--test-pattern");
        }
        if self.config.diagnostics {
            cmd.arg("--diagnostics");
        }
        let request = self
            .media_plan
            .as_ref()
            .map(|plan| &plan.request)
            .unwrap_or(&self.config.stream_request);
        cmd.arg("--width")
            .arg(request.video.width.to_string())
            .arg("--height")
            .arg(request.video.height.to_string())
            .arg("--fps")
            .arg(request.video.fps.to_string())
            .arg("--bitrate")
            .arg(request.bitrate_kbit.to_string())
            .arg("--audio-rate")
            .arg(request.audio.sample_rate.to_string());
        if self.config.fire_tv {
            let policy = self
                .media_plan
                .as_ref()
                .ok_or("Video capabilities have not been checked")?;
            cmd.args(["--encoder-profile", &policy.profile, "--gop-size"])
                .arg(
                    self.config
                        .encoder_gop_override
                        .unwrap_or(policy.gop_frames)
                        .to_string(),
                )
                .arg("--vbv-frames")
                .arg(self.config.encoder_vbv_override.unwrap_or(1).to_string());
        } else {
            // Keep the independently verified software emulator unchanged.
            cmd.args([
                "--encoder-profile",
                "baseline",
                "--gop-size",
                "15",
                "--vbv-frames",
                "1",
            ]);
        }
        let mut host = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(error_file)
            .spawn()
            .map_err(|e| format!("媒体主机启动失败：{e}"))?;
        let stdout = host.stdout.take().ok_or("主机输出不可用")?;
        self.host_ready = Arc::new(AtomicBool::new(false));
        let ready = self.host_ready.clone();
        let current = self.current.clone();
        let latest = self.latest.clone();
        let event_tx = self.events.clone();
        let session = session.to_owned();
        self.reader = Some(thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if *current.lock().expect("token lock") != token {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                    if value["session_id"].as_str() != Some(&session) {
                        continue;
                    }
                    if value["type"] == "host_ready" {
                        ready.store(true, Ordering::Release);
                    }
                    if let Ok(Some(metrics)) = telemetry::parse(&value, &session) {
                        if let Some(mut performance) = metrics.performance.clone() {
                            performance.sequence = metrics.sequence;
                            let _ = event_tx.send(Event {
                                token,
                                kind: EventKind::ClientPerformance(performance),
                            });
                        }
                        *latest.lock().expect("media lock") = Some((
                            Instant::now(),
                            Event {
                                token,
                                kind: EventKind::Media(metrics.sample()),
                            },
                        ));
                    }
                    if value["type"] == "host"
                        && let (Some(sequence), Some(encoded_fps)) = (
                            value["host_monotonic_ns"].as_u64(),
                            value["encoded_fps"].as_f64(),
                        )
                    {
                        let _ = event_tx.send(Event {
                            token,
                            kind: EventKind::HostPerformance(HostPerformance {
                                sequence,
                                encoded_fps,
                                encoder_p95_ms: value["encoder_ms_p95"].as_f64(),
                            }),
                        });
                    }
                }
            }
        }));
        self.host = Some(host);
        // Native plugin / driver cold start can exceed eight seconds on Windows.
        // Confirm host_ready within the core's 20s startup budget, without sleeps
        // being interpreted as readiness or extending the wait indefinitely.
        let deadline = Instant::now() + Duration::from_secs(15);
        while !self.host_ready.load(Ordering::Acquire) {
            if !self.active(token) {
                return Err("操作已取消".into());
            }
            if let Some(host) = self.host.as_mut()
                && host.try_wait().map_err(|e| e.to_string())?.is_some()
            {
                return Err("媒体主机未就绪便退出，请查看 local/sessions 日志".into());
            }
            if Instant::now() >= deadline {
                return Err("媒体主机就绪超时".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }
    fn cleanup(&mut self, token: Option<Token>) -> Result<(bool, bool), String> {
        let mute = self.restore_host_mute();
        let media = self.cleanup_media(token);
        // Restore even when device cleanup fails. Never report complete cleanup
        // while an owned audio route has not been released.
        if let Some(session) = self.audio_session.as_ref() {
            self.audio_route("Release", session)?;
            self.audio_session = None;
        }
        mute?;
        media
    }
    fn restore_host_mute(&mut self) -> Result<(), String> {
        if let Some(session) = self.mute_session.as_ref() {
            self.audio_control("audio-mute.ps1", "Release", session)?;
            self.mute_session = None;
        }
        Ok(())
    }
    fn cleanup_media(&mut self, token: Option<Token>) -> Result<(bool, bool), String> {
        if let Some(input) = self.input.as_mut() {
            input.shutdown()?;
        }
        self.input.take();
        if let Some(host) = self.host.as_mut() {
            if let Some(mut stdin) = host.stdin.take() {
                let _ = writeln!(stdin, "stop");
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if host.try_wait().map_err(|e| e.to_string())?.is_some() {
                    break;
                }
                if Instant::now() >= deadline {
                    host.kill().map_err(|e| e.to_string())?;
                    host.wait().map_err(|e| e.to_string())?;
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            self.host.take();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.latest.lock().expect("media lock").take();
        if !self.client_owned && !self.probe_client_owned {
            return Ok((true, true));
        }
        // The AVD identity check also prevents cleaning up an unrelated emulator.
        if self.confirm_tv(token).is_err() {
            return Ok((true, false));
        }
        let (stopped, _) = self.adb(&["shell", "am", "force-stop", PACKAGE], token)?;
        let (has_pid, pid) = self.adb(&["shell", "pidof", PACKAGE], token)?;
        let client_stopped = stopped && !has_pid && pid.trim().is_empty();
        if client_stopped {
            self.client_owned = false;
            self.probe_client_owned = false;
        }
        Ok((true, client_stopped))
    }
    fn effect(&mut self, effect: Effect) -> Result<(), String> {
        let token = effect.token;
        let session = self.session(token);
        match effect.kind {
            EffectKind::HostAudio { mute, request } => {
                let result = if mute {
                    if self.mute_session.is_some() {
                        Ok(())
                    } else {
                        self.mute_session = Some(session.clone());
                        self.audio_control("audio-mute.ps1", "Acquire", &session)
                            .map(|_| ())
                    }
                } else {
                    self.restore_host_mute()
                };
                let (status, message) = match result {
                    Ok(()) => (
                        if mute {
                            HostAudioStatus::Muted
                        } else {
                            HostAudioStatus::Original
                        },
                        if mute {
                            "电脑已静音，电视继续播放".into()
                        } else {
                            "已恢复电脑原静音状态".into()
                        },
                    ),
                    Err(error) => (HostAudioStatus::Error, error),
                };
                self.emit(
                    token,
                    EventKind::HostAudio {
                        request,
                        status,
                        message,
                    },
                );
            }
            EffectKind::Check => {
                if self.ownership.is_none() {
                    std::fs::create_dir_all(self.config.root.join("local"))
                        .map_err(|e| e.to_string())?;
                    let mut options = std::fs::OpenOptions::new();
                    options.read(true).write(true).create(true).truncate(false);
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::OpenOptionsExt;
                        options.share_mode(0);
                    }
                    self.ownership = Some(
                        options
                            .open(self.config.root.join("local/emulator-session.lock"))
                            .map_err(|e| {
                                format!("无法取得模拟器会话控制权，可能已有会话运行：{e}")
                            })?,
                    );
                }
                if self.config.mute_host {
                    self.audio_control("audio-mute.ps1", "Recover", &session)?;
                }
                self.confirm_tv(Some(token))?;
                let (ok, boot) =
                    self.adb(&["shell", "getprop", "sys.boot_completed"], Some(token))?;
                if !ok || boot.trim() != "1" {
                    return Err("Android TV 设备尚未启动完成".into());
                }
                let (ok, apk) = self.adb(&["shell", "pm", "path", PACKAGE], Some(token))?;
                if !ok || !apk.contains("package:") {
                    return Err("请先安装实验客户端".into());
                }
                let negotiation = if self.config.fire_tv {
                    let result = self.probe_media(token)?;
                    self.media_plan = Some(result.plan.clone());
                    Some(result)
                } else {
                    None
                };
                if !self
                    .config
                    .root
                    .join("experiments/stream/host.py")
                    .is_file()
                {
                    return Err("媒体主机路径不存在".into());
                }
                if let Some(result) = negotiation {
                    self.emit(token, EventKind::Negotiated(result));
                } else {
                    self.emit(token, EventKind::Checked);
                }
            }
            EffectKind::StartHost => {
                if self.config.virtual_audio && !self.config.test_source {
                    self.audio_session = Some(session.clone());
                    self.audio_route("Acquire", &session)?;
                }
                self.host_start(token, &session)?;
                self.emit(token, EventKind::HostReady);
            }
            EffectKind::StartClient => {
                self.client_start(token, &session)?;
                self.emit(token, EventKind::ClientReady);
            }
            EffectKind::RestartMedia => {
                let (host, client) = self.cleanup(Some(token))?;
                if !host || !client {
                    return Err("旧媒体资源尚未确认释放".into());
                }
                if self.config.fire_tv {
                    self.media_plan = None;
                    let result = self.probe_media(token)?;
                    self.media_plan = Some(result.plan.clone());
                    self.emit(token, EventKind::Negotiated(result));
                }
                if self.config.virtual_audio && !self.config.test_source {
                    self.audio_session = Some(session.clone());
                    self.audio_route("Acquire", &session)?;
                }
                self.host_start(token, &session)?;
                self.client_start(token, &session)?;
                self.emit(token, EventKind::Recovered);
            }
            EffectKind::Cleanup => {
                let result = self.cleanup(Some(token));
                let (host_stopped, client_stopped) = result.unwrap_or((false, false));
                if host_stopped && client_stopped {
                    self.ownership.take();
                }
                self.emit(
                    token,
                    EventKind::CleanupResult {
                        host_stopped,
                        client_stopped,
                    },
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn media_contract_roundtrips_for_frontends_and_adapters() {
        let request = StreamRequest {
            video: VideoMode {
                width: 1280,
                height: 720,
                fps: 30,
            },
            ..Default::default()
        };
        let encoded = serde_json::to_string(&request).unwrap();
        assert!(encoded.contains("pcm_s16_le"));
        assert_eq!(
            serde_json::from_str::<StreamRequest>(&encoded).unwrap(),
            request
        );
        let mut malformed = serde_json::to_value(&request).unwrap();
        malformed["unknown"] = true.into();
        assert!(serde_json::from_value::<StreamRequest>(malformed).is_err());
    }
    #[test]
    fn poll_rejects_stale_generation_and_expired_feedback() {
        let (tx, _rx) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let current = Token {
            session: 1,
            generation: 2,
        };
        let latest = Arc::new(Mutex::new(Some((
            Instant::now() - Duration::from_secs(3),
            Event {
                token: current,
                kind: EventKind::ClientReady,
            },
        ))));
        let mut backend = EmulatorBackend {
            request: StreamRequest::default(),
            serial: SERIAL.into(),
            tx,
            events,
            latest: latest.clone(),
            current: Arc::new(Mutex::new(current)),
            worker: None,
        };
        events_tx
            .send(Event {
                token: Token {
                    session: 1,
                    generation: 1,
                },
                kind: EventKind::HostReady,
            })
            .unwrap();
        assert!(backend.poll(0).is_empty());
        events_tx
            .send(Event {
                token: current,
                kind: EventKind::ClientReady,
            })
            .unwrap();
        assert_eq!(backend.poll(1).len(), 1);
    }
}
