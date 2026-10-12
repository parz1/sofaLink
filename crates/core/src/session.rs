use crate::*;

pub const STARTUP_TIMEOUT_MS: TimeMs = 5_000;
pub const FEEDBACK_TIMEOUT_MS: TimeMs = 1_500;
pub const RECOVERY_TIMEOUT_MS: TimeMs = 4_000;
pub const CLEANUP_TIMEOUT_MS: TimeMs = 3_000;

/// Host monotonic time budgets; frontends do not decide timeout outcomes.
#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    pub startup_ms: TimeMs,
    pub feedback_ms: TimeMs,
    pub recovery_ms: TimeMs,
    pub cleanup_ms: TimeMs,
}
impl Default for Timeouts {
    fn default() -> Self {
        Self {
            startup_ms: STARTUP_TIMEOUT_MS,
            feedback_ms: FEEDBACK_TIMEOUT_MS,
            recovery_ms: RECOVERY_TIMEOUT_MS,
            cleanup_ms: CLEANUP_TIMEOUT_MS,
        }
    }
}

pub struct Core {
    timeouts: Timeouts,
    state: Snapshot,
    clock: TimeMs,
    deadline: Option<TimeMs>,
    has_played: bool,
    failure_after_cleanup: Option<Issue>,
    host_mute_wanted: bool,
    host_audio_request: u64,
}

impl Default for Core {
    fn default() -> Self {
        let mut value = Self {
            timeouts: Timeouts::default(),
            state: Snapshot {
                schema_version: 1,
                revision: 0,
                token: Token::default(),
                activity: ActivityKind::DesktopStream,
                phase: Phase::Idle,
                message: "准备就绪".into(),
                video: SignalState::Stopped,
                audio: SignalState::Stopped,
                host_audio: HostAudioState::default(),
                input: InputState::default(),
                performance: PerformanceState::default(),
                stream_request: StreamRequest::default(),
                media_negotiation: None,
                resources: Resources::default(),
                metrics: None,
                last_feedback_ms: None,
                issue: None,
                available_commands: vec![],
                supported_activities: vec![ActivityKind::DesktopStream],
            },
            clock: 0,
            deadline: None,
            has_played: false,
            failure_after_cleanup: None,
            host_mute_wanted: false,
            host_audio_request: 0,
        };
        value.actions();
        value
    }
}

impl Core {
    pub fn with_timeouts(timeouts: Timeouts) -> Self {
        Self {
            timeouts,
            ..Self::default()
        }
    }
    pub fn snapshot(&self) -> &Snapshot {
        &self.state
    }
    pub fn with_stream_request(mut self, request: StreamRequest) -> Self {
        self.state.stream_request = request;
        self
    }
    pub fn with_host_mute(timeouts: Timeouts, enabled: bool) -> Self {
        let mut core = Self::with_timeouts(timeouts);
        if enabled {
            core.state.host_audio = HostAudioState {
                enabled: true,
                status: HostAudioStatus::Original,
                message: "保持电脑原状态，等待电视播放".into(),
            };
        }
        core
    }
    fn reconcile_host_audio(&mut self, effects: &mut Vec<Effect>) {
        if !self.state.host_audio.enabled {
            return;
        }
        let wanted = self.state.phase == Phase::Playing;
        if wanted == self.host_mute_wanted {
            return;
        }
        self.host_mute_wanted = wanted;
        self.host_audio_request += 1;
        self.state.host_audio.status = if wanted {
            HostAudioStatus::Muting
        } else {
            HostAudioStatus::Restoring
        };
        self.state.host_audio.message = if wanted {
            "电视已开始播放，正在静音电脑"
        } else {
            "正在恢复电脑原静音状态"
        }
        .into();
        // Cleanup owns restoration and cannot succeed while it remains pending.
        if !matches!(self.state.phase, Phase::Stopping | Phase::Faulted) {
            effects.push(self.effect(EffectKind::HostAudio {
                mute: wanted,
                request: self.host_audio_request,
            }));
        }
    }

    pub fn command(&mut self, command: Command, now: TimeMs) -> Result<Vec<Effect>, CommandError> {
        if now < self.clock {
            return Err(CommandError::ClockWentBackward);
        }
        if matches!(
            command,
            Command::Start {
                activity: ActivityKind::MediaPlayback
            }
        ) {
            return Err(CommandError::UnsupportedActivity);
        }
        if !self.state.can(command) {
            return Err(CommandError::NotAvailable);
        }
        if matches!(command, Command::Start { .. } | Command::Retry)
            && self.state.stream_request.validate().is_err()
        {
            return Err(CommandError::InvalidStreamRequest);
        }
        self.clock = now;
        let kind = match command {
            Command::Start { activity } => {
                self.state.activity = activity;
                self.start(now);
                EffectKind::Check
            }
            Command::Retry => {
                self.start(now);
                EffectKind::Check
            }
            Command::Stop => {
                self.begin_cleanup(now, None);
                EffectKind::Cleanup
            }
            Command::Recover if self.state.phase == Phase::Faulted => {
                self.begin_cleanup(now, None);
                EffectKind::Cleanup
            }
            Command::Recover => {
                self.state.token.generation += 1;
                self.reset_media();
                self.state.media_negotiation = None;
                self.state.issue = None;
                self.set_phase(Phase::Recovering, "重新建立音画连接");
                self.deadline = Some(now + self.timeouts.recovery_ms);
                EffectKind::RestartMedia
            }
        };
        let mut effects = vec![self.effect(kind)];
        self.reconcile_host_audio(&mut effects);
        self.changed();
        Ok(effects)
    }

    fn start(&mut self, now: TimeMs) {
        self.state.media_negotiation = None;
        self.state.token = Token {
            session: self.state.token.session + 1,
            generation: 0,
        };
        self.reset_media();
        self.has_played = false;
        self.state.issue = None;
        self.failure_after_cleanup = None;
        self.deadline = Some(now + self.timeouts.startup_ms);
        self.set_phase(Phase::Checking, "检查设备与串流能力");
    }

    fn reset_media(&mut self) {
        self.state.performance = PerformanceState::default();
        self.state.input = InputState::default();
        self.state.metrics = None;
        self.state.last_feedback_ms = None;
        self.state.video = SignalState::Waiting;
        self.state.audio = SignalState::Waiting;
    }

    fn begin_cleanup(&mut self, now: TimeMs, failure: Option<Issue>) {
        // Invalidate all pending start/recovery/feedback events before cleanup begins.
        self.state.token.generation += 1;
        self.failure_after_cleanup = failure;
        self.state.issue = self.failure_after_cleanup.clone();
        if self.state.resources.host != ResourceState::Stopped {
            self.state.resources.host = ResourceState::Unknown;
        }
        if self.state.resources.client != ResourceState::Stopped {
            self.state.resources.client = ResourceState::Unknown;
        }
        self.state.video = SignalState::Stopped;
        self.state.audio = SignalState::Stopped;
        self.state.performance = PerformanceState::default();
        self.state.input = InputState::default();
        self.state.metrics = None;
        self.state.last_feedback_ms = None;
        self.set_phase(Phase::Stopping, "停止会话，等待资源释放确认");
        self.deadline = Some(now + self.timeouts.cleanup_ms);
    }

    pub fn event(&mut self, event: Event, now: TimeMs) -> Vec<Effect> {
        if now < self.clock || event.token != self.state.token {
            return vec![];
        }
        self.clock = now;
        let mut effects = vec![];
        match (self.state.phase, event.kind) {
            (phase @ (Phase::Checking | Phase::Recovering), EventKind::Negotiated(result)) => {
                match negotiate_media(&self.state.stream_request, &result.host, &result.client) {
                    Ok(plan) if plan == result.plan => {
                        self.state.media_negotiation = Some(result);
                        if phase == Phase::Checking {
                            self.state.resources.host = ResourceState::Starting;
                            self.set_phase(Phase::StartingHost, "能力协商完成，准备主机");
                            self.deadline = Some(now + self.timeouts.startup_ms);
                            effects.push(self.effect(EffectKind::StartHost));
                        }
                    }
                    _ => {
                        self.begin_cleanup(
                            now,
                            Some(Issue::new(
                                IssueCode::StartupFailed,
                                "设备能力或协商结果与请求不匹配",
                            )),
                        );
                        effects.push(self.effect(EffectKind::Cleanup));
                    }
                }
            }
            (
                phase,
                EventKind::HostAudio {
                    request,
                    status,
                    message,
                },
            ) if phase.active()
                && self.state.host_audio.enabled
                && request == self.host_audio_request =>
            {
                let expected = if self.host_mute_wanted {
                    HostAudioStatus::Muted
                } else {
                    HostAudioStatus::Original
                };
                if status != expected && status != HostAudioStatus::Error {
                    return vec![];
                }
                self.state.host_audio.status = status;
                self.state.host_audio.message = message.clone();
                if status == HostAudioStatus::Error && phase != Phase::Stopping {
                    self.begin_cleanup(now, Some(Issue::new(IssueCode::HostAudioFailed, message)));
                    effects.push(self.effect(EffectKind::Cleanup));
                }
            }
            (
                Phase::StartingHost
                | Phase::StartingClient
                | Phase::WaitingMedia
                | Phase::Playing
                | Phase::Degraded
                | Phase::Recovering,
                EventKind::ClientPerformance(performance),
            ) => {
                if !performance.valid()
                    || self
                        .state
                        .performance
                        .client
                        .as_ref()
                        .is_some_and(|p| performance.sequence <= p.sequence)
                {
                    return vec![];
                }
                self.state.performance.client = Some(performance);
                self.state.performance.client_updated_ms = Some(now);
                self.state
                    .performance
                    .refresh(now, self.state.stream_request.video.fps);
            }
            (
                Phase::StartingHost
                | Phase::StartingClient
                | Phase::WaitingMedia
                | Phase::Playing
                | Phase::Degraded
                | Phase::Recovering,
                EventKind::HostPerformance(performance),
            ) => {
                if !performance.encoded_fps.is_finite()
                    || performance.encoded_fps < 0.0
                    || performance
                        .encoder_p95_ms
                        .is_some_and(|n| !n.is_finite() || n < 0.0)
                    || self
                        .state
                        .performance
                        .host
                        .as_ref()
                        .is_some_and(|p| performance.sequence <= p.sequence)
                {
                    return vec![];
                }
                self.state.performance.host = Some(performance);
                self.state.performance.host_updated_ms = Some(now);
                self.state
                    .performance
                    .refresh(now, self.state.stream_request.video.fps);
            }
            (
                Phase::StartingHost
                | Phase::StartingClient
                | Phase::WaitingMedia
                | Phase::Playing
                | Phase::Degraded
                | Phase::Recovering,
                EventKind::Input(input),
            ) => {
                self.state.input = input;
            }
            (Phase::Checking, EventKind::Checked) => {
                self.state.resources.host = ResourceState::Starting;
                self.set_phase(Phase::StartingHost, "准备捕获、编码与声音");
                effects.push(self.effect(EffectKind::StartHost));
            }
            (Phase::StartingHost, EventKind::HostReady) => {
                self.state.resources.host = ResourceState::Running;
                self.state.resources.client = ResourceState::Starting;
                self.set_phase(Phase::StartingClient, "启动客户端");
                effects.push(self.effect(EffectKind::StartClient));
            }
            (Phase::StartingClient, EventKind::ClientReady) => {
                self.state.resources.client = ResourceState::Running;
                self.set_phase(Phase::WaitingMedia, "等待实际音画播放进度");
            }
            (Phase::Recovering, EventKind::Recovered) => {
                if self.state.host_audio.enabled {
                    self.state.host_audio.status = HostAudioStatus::Original;
                    self.state.host_audio.message = "已恢复电脑原静音状态，等待电视播放".into();
                }
                self.state.resources.host = ResourceState::Running;
                self.state.resources.client = ResourceState::Running;
                self.set_phase(Phase::WaitingMedia, "连接已恢复，等待音画播放确认");
                self.deadline = Some(now + self.timeouts.recovery_ms);
            }
            (Phase::WaitingMedia | Phase::Playing | Phase::Degraded, EventKind::Media(sample)) => {
                if self.state.resources.host == ResourceState::Stopped
                    || self.state.resources.client == ResourceState::Stopped
                {
                    return vec![];
                }
                if !sample.valid()
                    || self
                        .state
                        .metrics
                        .as_ref()
                        .is_some_and(|m| sample.sequence <= m.sequence)
                {
                    return vec![];
                }
                self.media(sample, now);
            }
            (
                Phase::WaitingMedia | Phase::Playing | Phase::Degraded,
                kind @ (EventKind::HostExited | EventKind::ClientExited),
            ) => {
                let host = matches!(kind, EventKind::HostExited);
                if host {
                    self.state.resources.host = ResourceState::Stopped;
                } else {
                    self.state.resources.client = ResourceState::Stopped;
                }
                self.state.video = SignalState::Missing;
                self.state.audio = SignalState::Missing;
                self.state.input = InputState::new(
                    InputStatus::Disconnected,
                    "会话进程退出，输入已停用",
                    self.state.input.applied_events,
                );
                let issue = Issue::new(
                    if host {
                        IssueCode::HostExited
                    } else {
                        IssueCode::ClientExited
                    },
                    if host {
                        "媒体主机已退出，请恢复连接或结束"
                    } else {
                        "客户端进程已退出，请恢复连接或结束"
                    },
                );
                self.set_phase(Phase::Degraded, &issue.message);
                self.state.issue = Some(issue);
                self.deadline = None;
            }
            (
                Phase::Stopping,
                EventKind::CleanupResult {
                    host_stopped,
                    client_stopped,
                },
            ) => {
                self.state.resources.host = if host_stopped {
                    ResourceState::Stopped
                } else {
                    ResourceState::Unknown
                };
                self.state.resources.client = if client_stopped {
                    ResourceState::Stopped
                } else {
                    ResourceState::Unknown
                };
                self.deadline = None;
                if self.state.resources.clear() {
                    if self.state.host_audio.enabled {
                        self.state.host_audio.status = HostAudioStatus::Original;
                        self.state.host_audio.message = "已恢复电脑原静音状态".into();
                    }
                    if let Some(issue) = self.failure_after_cleanup.take() {
                        self.state.issue = Some(issue.clone());
                        self.set_phase(Phase::Faulted, &issue.message);
                    } else {
                        self.state.issue = None;
                        self.set_phase(Phase::Idle, "会话已结束，资源已释放");
                    }
                } else {
                    if self.state.host_audio.enabled {
                        self.state.host_audio.status = HostAudioStatus::Error;
                        self.state.host_audio.message = "资源清理或电脑声音恢复尚未确认".into();
                    }
                    let issue = Issue::new(IssueCode::CleanupFailed, "资源释放未确认，请重试清理");
                    self.state.issue = Some(issue.clone());
                    self.set_phase(Phase::Faulted, &issue.message);
                }
            }
            (phase, EventKind::Failed(issue)) if phase.active() && phase != Phase::Stopping => {
                self.begin_cleanup(now, Some(issue));
                effects.push(self.effect(EffectKind::Cleanup));
            }
            _ => return vec![],
        }
        self.reconcile_host_audio(&mut effects);
        self.changed();
        effects
    }

    fn media(&mut self, sample: MediaSample, now: TimeMs) {
        let previous = self.state.metrics.as_ref();
        let initial_sample = previous.is_none();
        self.state.video = match previous {
            None => SignalState::Waiting,
            Some(p)
                if sample.rendered_frames > p.rendered_frames
                    && sample.video_fps
                        >= f64::from(self.state.stream_request.video.fps) * 0.75 =>
            {
                SignalState::Healthy
            }
            Some(_) => SignalState::Stalled,
        };
        self.state.audio = match previous {
            None => SignalState::Waiting,
            Some(p) if sample.audio_played_frames > p.audio_played_frames => SignalState::Healthy,
            Some(_) => SignalState::Stalled,
        };
        self.state.metrics = Some(sample);
        self.state.last_feedback_ms = Some(now);
        if self.state.video == SignalState::Healthy && self.state.audio == SignalState::Healthy {
            self.has_played = true;
            self.deadline = None;
            self.state.issue = None;
            self.set_phase(Phase::Playing, "音画播放进度正常");
        } else if self.has_played && !initial_sample {
            let issue = if self.state.video != SignalState::Healthy {
                Issue::new(IssueCode::VideoStalled, "画面呈现停止或帧率不足")
            } else {
                Issue::new(IssueCode::AudioStalled, "声音播放进度停止")
            };
            self.set_phase(Phase::Degraded, &issue.message);
            self.state.issue = Some(issue);
        }
    }

    pub fn tick(&mut self, now: TimeMs) -> Vec<Effect> {
        if now < self.clock {
            return vec![];
        }
        self.clock = now;
        let old = self.state.performance.clone();
        self.state
            .performance
            .refresh(now, self.state.stream_request.video.fps);
        if old != self.state.performance {
            self.changed();
        }
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            if self.state.phase == Phase::Stopping {
                let issue = Issue::new(IssueCode::CleanupFailed, "清理超时，尚未确认资源释放");
                self.state.issue = Some(issue.clone());
                self.deadline = None;
                self.set_phase(Phase::Faulted, &issue.message);
                self.changed();
                return vec![];
            }
            let code = if self.has_played {
                IssueCode::RecoveryFailed
            } else {
                IssueCode::StartupTimeout
            };
            let issue = Issue::new(code, "等待音画就绪超时，会话正在清理");
            self.begin_cleanup(now, Some(issue));
            let mut effects = vec![self.effect(EffectKind::Cleanup)];
            self.reconcile_host_audio(&mut effects);
            self.changed();
            return effects;
        }
        if matches!(self.state.phase, Phase::Playing | Phase::Degraded)
            && self
                .state
                .last_feedback_ms
                .is_some_and(|last| now.saturating_sub(last) >= self.timeouts.feedback_ms)
            && self.state.issue.as_ref().is_none_or(|i| {
                !matches!(
                    i.code,
                    IssueCode::FeedbackExpired | IssueCode::HostExited | IssueCode::ClientExited
                )
            })
        {
            self.state.video = SignalState::Missing;
            self.state.audio = SignalState::Missing;
            let issue = Issue::new(
                IssueCode::FeedbackExpired,
                "客户端反馈中断，请恢复连接或结束",
            );
            self.set_phase(Phase::Degraded, &issue.message);
            self.state.issue = Some(issue);
            let mut effects = vec![];
            self.reconcile_host_audio(&mut effects);
            self.changed();
            return effects;
        }
        vec![]
    }

    fn effect(&self, kind: EffectKind) -> Effect {
        Effect {
            token: self.state.token,
            kind,
        }
    }
    fn set_phase(&mut self, phase: Phase, message: &str) {
        self.state.phase = phase;
        self.state.message = message.into();
    }
    fn changed(&mut self) {
        self.state.revision += 1;
        self.actions();
    }
    fn actions(&mut self) {
        self.state.available_commands = match self.state.phase {
            Phase::Idle => vec![Command::Start {
                activity: ActivityKind::DesktopStream,
            }],
            Phase::Faulted if self.state.resources.clear() => vec![Command::Retry],
            Phase::Faulted => vec![Command::Recover],
            Phase::Degraded => vec![Command::Recover, Command::Stop],
            Phase::Playing
                if matches!(
                    self.state.input.status,
                    InputStatus::Disconnected | InputStatus::Error
                ) =>
            {
                vec![Command::Recover, Command::Stop]
            }
            Phase::Stopping => vec![],
            _ => vec![Command::Stop],
        };
    }
}
