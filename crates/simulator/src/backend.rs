use crate::Scenario;
use sofalink_core::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SimResources {
    pub host: bool,
    pub client: bool,
}

pub struct Simulator {
    scenario: Scenario,
    token: Token,
    pending: Vec<(TimeMs, Event)>,
    resources: SimResources,
    streaming_since: Option<TimeMs>,
    next_feedback: TimeMs,
    sample: MediaSample,
    fault_consumed: bool,
    stalled: bool,
}

impl Simulator {
    pub fn new(scenario: Scenario) -> Self {
        Self {
            scenario,
            token: Token::default(),
            pending: vec![],
            resources: SimResources::default(),
            streaming_since: None,
            next_feedback: 0,
            sample: MediaSample {
                sequence: 0,
                rendered_frames: 0,
                audio_played_frames: 0,
                video_fps: 60.0,
                receive_to_render_ms: 65.0,
                audio_buffer_ms: 24.0,
            },
            fault_consumed: false,
            stalled: false,
        }
    }
    pub fn resources(&self) -> SimResources {
        self.resources
    }
    fn schedule(&mut self, now: TimeMs, delay: TimeMs, kind: EventKind) {
        self.pending.push((
            now + delay,
            Event {
                token: self.token,
                kind,
            },
        ));
    }
    fn feedback_start(&mut self, now: TimeMs) {
        self.streaming_since = Some(now);
        self.next_feedback = now + 250;
        self.sample = MediaSample {
            sequence: 0,
            rendered_frames: 0,
            audio_played_frames: 0,
            video_fps: 60.0,
            receive_to_render_ms: 65.0,
            audio_buffer_ms: 24.0,
        };
    }
}

impl Backend for Simulator {
    fn execute(&mut self, effect: Effect, now: TimeMs) {
        if effect.token != self.token {
            self.pending.clear();
            self.streaming_since = None;
            self.token = effect.token;
        }
        match effect.kind {
            EffectKind::HostAudio { mute, request } => {
                self.schedule(
                    now,
                    50,
                    EventKind::HostAudio {
                        request,
                        status: if mute {
                            HostAudioStatus::Muted
                        } else {
                            HostAudioStatus::Original
                        },
                        message: if mute {
                            "电脑已静音"
                        } else {
                            "已恢复电脑原静音状态"
                        }
                        .into(),
                    },
                );
            }
            EffectKind::Check => {
                self.stalled = false;
                self.schedule(now, 300, EventKind::Checked);
            }
            EffectKind::StartHost => {
                // Model a partially acquired resource even when startup fails.
                self.resources.host = true;
                if self.scenario == Scenario::StartupFail && !self.fault_consumed {
                    self.fault_consumed = true;
                    self.schedule(
                        now,
                        350,
                        EventKind::Failed(Issue::new(IssueCode::StartupFailed, "模拟主机启动失败")),
                    );
                } else {
                    self.schedule(now, 350, EventKind::HostReady);
                }
            }
            EffectKind::StartClient => {
                self.resources.client = true;
                if self.scenario == Scenario::StartupTimeout && !self.fault_consumed {
                    self.fault_consumed = true;
                } else {
                    self.schedule(now, 350, EventKind::ClientReady);
                }
            }
            EffectKind::RestartMedia => {
                self.stalled = false;
                self.schedule(now, 600, EventKind::Recovered);
            }
            EffectKind::Cleanup => {
                self.streaming_since = None;
                let fail = self.scenario == Scenario::StopFail && !self.fault_consumed;
                if fail {
                    self.fault_consumed = true;
                }
                self.schedule(
                    now,
                    400,
                    EventKind::CleanupResult {
                        host_stopped: !fail,
                        client_stopped: true,
                    },
                );
            }
        }
    }

    fn poll(&mut self, now: TimeMs) -> Vec<Event> {
        let mut events = vec![];
        let mut remaining = vec![];
        for (at, event) in std::mem::take(&mut self.pending) {
            if at > now {
                remaining.push((at, event));
                continue;
            }
            match event.kind {
                EventKind::ClientReady | EventKind::Recovered => self.feedback_start(now),
                EventKind::CleanupResult {
                    host_stopped,
                    client_stopped,
                } => {
                    if host_stopped {
                        self.resources.host = false;
                    }
                    if client_stopped {
                        self.resources.client = false;
                    }
                }
                _ => {}
            }
            events.push(event);
        }
        self.pending = remaining;
        if let Some(since) = self.streaming_since
            && now >= self.next_feedback
        {
            if now.saturating_sub(since) >= 3_000
                && !self.fault_consumed
                && matches!(
                    self.scenario,
                    Scenario::AudioStall | Scenario::VideoStall | Scenario::Disconnect
                )
            {
                self.fault_consumed = true;
                self.stalled = true;
            }
            if !(self.stalled && self.scenario == Scenario::Disconnect) {
                let intervals = (now - self.next_feedback) / 250 + 1;
                self.sample.sequence += 1;
                if !(self.stalled && self.scenario == Scenario::VideoStall) {
                    self.sample.rendered_frames += 15 * intervals;
                }
                if !(self.stalled && self.scenario == Scenario::AudioStall) {
                    self.sample.audio_played_frames += 12_000 * intervals;
                }
                self.sample.video_fps = if self.stalled && self.scenario == Scenario::VideoStall {
                    0.0
                } else {
                    60.0
                };
                events.push(Event {
                    token: self.token,
                    kind: EventKind::Media(self.sample.clone()),
                });
            }
            self.next_feedback = now + 250;
        }
        events
    }
}
