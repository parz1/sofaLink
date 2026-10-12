use sofalink_core::*;
use sofalink_emulator::{Config, EmulatorBackend};
use sofalink_simulator::{Scenario, Simulator};
use std::collections::VecDeque;

pub enum AppBackend {
    Simulated(Simulator),
    Emulator(EmulatorBackend),
}
impl Backend for AppBackend {
    fn application_target(&self) -> ApplicationTarget {
        match self {
            Self::Simulated(b) => b.application_target(),
            Self::Emulator(b) => b.application_target(),
        }
    }
    fn stream_request(&self) -> StreamRequest {
        match self {
            Self::Simulated(b) => b.stream_request(),
            Self::Emulator(b) => b.stream_request(),
        }
    }
    fn execute(&mut self, effect: Effect, now: TimeMs) {
        match self {
            Self::Simulated(b) => b.execute(effect, now),
            Self::Emulator(b) => b.execute(effect, now),
        }
    }
    fn poll(&mut self, now: TimeMs) -> Vec<Event> {
        match self {
            Self::Simulated(b) => b.poll(now),
            Self::Emulator(b) => b.poll(now),
        }
    }
}
pub struct App {
    pub runtime: Runtime<AppBackend>,
    pub real_device: bool,
    pub test_source: bool,
    pub reference_video: bool,
    pub scenario: Scenario,
    pub details: bool,
    pub quitting: bool,
    pub notice: Option<String>,
    pub quit_started: Option<TimeMs>,
    pub(crate) history: VecDeque<Phase>,
}

impl App {
    pub fn new(scenario: Scenario) -> Self {
        Self::with_simulated_host_mute(scenario, false)
    }
    pub fn with_simulated_host_mute(scenario: Scenario, enabled: bool) -> Self {
        Self {
            runtime: Runtime::with_host_mute(
                AppBackend::Simulated(Simulator::new(scenario)),
                Timeouts::default(),
                enabled,
            ),
            real_device: false,
            test_source: false,
            reference_video: false,
            scenario,
            details: false,
            quitting: false,
            notice: None,
            quit_started: None,
            history: VecDeque::from([Phase::Idle]),
        }
    }
    pub fn emulator(config: Config) -> Result<Self, String> {
        let test_source = config.test_source;
        let reference_video = config.reference_video;
        let mute_host = config.mute_host;
        let backend = EmulatorBackend::new(config)?;
        let mut app = Self::new(Scenario::Healthy);
        app.runtime =
            Runtime::with_host_mute(AppBackend::Emulator(backend), Config::timeouts(), mute_host);
        app.real_device = true;
        app.test_source = test_source;
        app.reference_video = reference_video;
        Ok(app)
    }
    pub fn advance(&mut self, now: TimeMs) {
        self.runtime.advance(now);
        if self.quitting
            && self.runtime.snapshot().phase == Phase::Faulted
            && self.runtime.snapshot().can(Command::Recover)
        {
            self.command(Command::Recover, now);
        }
        let phase = self.runtime.snapshot().phase;
        if self.history.back() != Some(&phase) {
            self.history.push_back(phase);
            if self.history.len() > 8 {
                self.history.pop_front();
            }
        }
    }
    pub fn command(&mut self, command: Command, now: TimeMs) {
        if let Err(error) = self.runtime.command(command, now) {
            self.notice = Some(error.to_string());
        } else {
            self.notice = None;
        }
    }
    pub fn primary(&mut self, now: TimeMs) {
        if let Some(command) = self.runtime.snapshot().primary_command() {
            self.command(command, now);
        }
    }
    pub fn select_scenario(&mut self, index: usize) {
        if self.real_device || self.runtime.snapshot().phase != Phase::Idle {
            return;
        }
        if let Some(scenario) = Scenario::ALL.get(index).copied() {
            *self = Self::new(scenario);
        }
    }
    pub fn request_exit(&mut self, now: TimeMs) {
        self.quitting = true;
        self.quit_started.get_or_insert(now);
        if self.runtime.snapshot().can(Command::Stop) {
            self.command(Command::Stop, now);
        } else if self.runtime.snapshot().can(Command::Recover) {
            self.command(Command::Recover, now);
        }
    }
    pub fn can_exit(&self) -> bool {
        self.quitting
            && (self.runtime.snapshot().phase == Phase::Idle
                || (self.runtime.snapshot().phase == Phase::Faulted
                    && self.runtime.snapshot().resources.clear()))
    }
}
