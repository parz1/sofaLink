//! Focus ownership is evidence-driven and independent of media clocks/buffers.
use crate::TimeMs;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowRef {
    pub handle: u64,
    pub process_id: u32,
    /// OS process creation identity; prevents HWND/PID reuse from selecting another app.
    pub process_created: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusOwner {
    #[default]
    Unknown,
    Computer,
    Game,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusCommand {
    TakeComputer,
    ReturnGame,
    MoveGameToSpace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusEffect {
    pub request: u64,
    pub target: WindowRef,
    pub command: FocusCommand,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusSnapshot {
    pub revision: u64,
    pub owner: FocusOwner,
    pub game_in_space: bool,
    pub game: Option<WindowRef>,
    pub work: Option<WindowRef>,
    pub pending: Option<FocusEffect>,
    pub message: String,
    pub issue: Option<String>,
    pub available_commands: Vec<FocusCommand>,
}

pub struct FocusObservation {
    pub sequence: u64,
    pub foreground: Option<WindowRef>,
    pub game_alive: bool,
    pub game_in_space: bool,
    pub work_alive: bool,
    /// Adapter excludes its own UI, desktop/shell and session-owned game windows.
    pub work_candidate: Option<WindowRef>,
}

/// One owner per focus session. Frontends render this snapshot and submit commands.
#[derive(Default)]
pub struct FocusCore {
    state: FocusSnapshot,
    sequence: u64,
    request: u64,
    deadline: Option<TimeMs>,
    clock: TimeMs,
}

impl FocusCore {
    pub fn attach_with_work(&mut self, game: WindowRef, work: Option<WindowRef>) {
        self.attach(game);
        self.state.work = work.filter(|w| *w != game);
        self.publish();
    }
    pub fn snapshot(&self) -> &FocusSnapshot {
        &self.state
    }
    pub fn attach(&mut self, game: WindowRef) {
        self.state.game = Some(game);
        self.state.owner = FocusOwner::Unknown;
        self.state.game_in_space = false;
        self.state.work = None;
        self.state.pending = None;
        self.state.issue = None;
        self.deadline = None;
        self.publish();
    }
    pub fn observe(&mut self, observation: FocusObservation, now: TimeMs) {
        if now < self.clock || observation.sequence <= self.sequence {
            return;
        }
        self.clock = now;
        self.sequence = observation.sequence;
        let before = self.state.clone();
        self.state.game_in_space = observation.game_alive && observation.game_in_space;
        if !observation.game_alive {
            self.state.game = None;
            self.state.pending = None;
            self.deadline = None;
        }
        if !observation.work_alive {
            self.state.work = None;
        }
        if self.state.game.is_some() && observation.foreground == self.state.game {
            self.state.owner = FocusOwner::Game;
        } else {
            self.state.owner = if observation.foreground.is_some() {
                FocusOwner::Computer
            } else {
                FocusOwner::Unknown
            };
            if let Some(candidate) = observation.work_candidate
                && Some(candidate) != self.state.game
            {
                self.state.work = Some(candidate);
            }
        }
        if self.state.owner != before.owner || self.state.game_in_space != before.game_in_space {
            self.state.issue = None;
        }
        if let Some(pending) = self.state.pending
            && (pending.command == FocusCommand::MoveGameToSpace
                || observation.foreground == Some(pending.target))
            && (Some(pending.target) != self.state.game || self.state.game_in_space)
        {
            self.state.pending = None;
            self.state.issue = None;
            self.deadline = None;
        }
        if self.state != before {
            self.publish();
        }
        self.tick(now);
    }
    pub fn command(&mut self, command: FocusCommand, now: TimeMs) -> Result<FocusEffect, String> {
        if now < self.clock || !self.state.available_commands.contains(&command) {
            return Err("焦点操作当前不可用".into());
        }
        self.clock = now;
        let target = match command {
            FocusCommand::TakeComputer => self.state.work,
            FocusCommand::ReturnGame | FocusCommand::MoveGameToSpace => self.state.game,
        }
        .ok_or("目标窗口已不可用")?;
        self.request += 1;
        let effect = FocusEffect {
            request: self.request,
            target,
            command,
        };
        self.state.pending = Some(effect);
        self.state.issue = None;
        self.deadline = Some(now.saturating_add(800));
        self.publish();
        Ok(effect)
    }
    pub fn failed(&mut self, request: u64, message: String) {
        if self.state.pending.is_some_and(|p| p.request == request) {
            self.state.pending = None;
            self.deadline = None;
            self.state.issue = Some(message);
            self.publish();
        }
    }
    pub fn tick(&mut self, now: TimeMs) {
        if now < self.clock {
            return;
        }
        self.clock = now;
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            let message = if !self.state.game_in_space {
                "游戏位置未确认，请在游戏内选择显示器或使用窗口模式"
            } else {
                "Windows 未确认切换，请用 Alt+Tab 选择目标窗口"
            };
            self.state.pending = None;
            self.deadline = None;
            self.state.issue = Some(message.into());
            self.publish();
        }
    }
    fn publish(&mut self) {
        self.state.revision += 1;
        self.state.available_commands.clear();
        self.state.message = if self.state.game.is_none() {
            "游戏窗口已不可用"
        } else if self
            .state
            .pending
            .is_some_and(|p| p.command == FocusCommand::MoveGameToSpace)
        {
            "正在移动游戏窗口…"
        } else if self.state.pending.is_some() {
            "正在切换焦点…"
        } else if !self.state.game_in_space {
            "游戏离开串流屏幕 · 请移回电视"
        } else {
            match self.state.owner {
                FocusOwner::Game => "游戏获得焦点",
                FocusOwner::Computer => "电脑正在使用 · 游戏未获得焦点",
                FocusOwner::Unknown => "等待真实焦点状态",
            }
        }
        .into();
        if self.state.game.is_some() && self.state.pending.is_none() {
            if self.state.owner != FocusOwner::Game || !self.state.game_in_space {
                self.state.available_commands.push(FocusCommand::ReturnGame);
            }
            if !self.state.game_in_space {
                self.state
                    .available_commands
                    .push(FocusCommand::MoveGameToSpace);
            }
            if self.state.work.is_some() && self.state.owner == FocusOwner::Game {
                self.state
                    .available_commands
                    .push(FocusCommand::TakeComputer);
            }
        }
    }
}
