//! Ordered virtual-display/session ownership. UI and platform code cannot skip cleanup stages.
use crate::ResourceState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpacePhase {
    #[default]
    Inspecting,
    Idle,
    Creating,
    Starting,
    Streaming,
    Stopping,
    Restoring,
    Reclaiming,
    Uninstalling,
    Faulted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceCommand {
    Start,
    Stop,
    Recover,
    Uninstall,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceOperation {
    Inspect,
    CreateDisplay,
    StartMedia,
    StopMedia,
    RestoreGame,
    RemoveDisplay,
    Uninstall,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceEffect {
    pub request: u64,
    pub operation: SpaceOperation,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceResources {
    pub display: ResourceState,
    pub media: ResourceState,
    pub game_window: ResourceState,
    pub driver_installed: bool,
}
impl Default for SpaceResources {
    fn default() -> Self {
        Self {
            display: ResourceState::Unknown,
            media: ResourceState::Unknown,
            game_window: ResourceState::Unknown,
            driver_installed: false,
        }
    }
}
impl SpaceResources {
    pub fn clear(&self) -> bool {
        self.display == ResourceState::Stopped
            && self.media == ResourceState::Stopped
            && self.game_window == ResourceState::Stopped
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceSnapshot {
    pub revision: u64,
    pub phase: SpacePhase,
    pub resources: SpaceResources,
    pub pending: Option<SpaceEffect>,
    pub issue: Option<String>,
    pub message: String,
    pub available_commands: Vec<SpaceCommand>,
}
#[derive(Default)]
pub struct GameSpaceCore {
    state: SpaceSnapshot,
    request: u64,
    cancel: bool,
}
impl GameSpaceCore {
    pub fn shutdown(&mut self) -> Option<SpaceEffect> {
        self.cancel = true;
        if self.state.pending.is_some() {
            self.publish();
            None
        } else {
            self.cleanup()
        }
    }
    pub fn allowed_focus_commands(&self, focus: &crate::FocusSnapshot) -> Vec<crate::FocusCommand> {
        focus
            .available_commands
            .iter()
            .copied()
            .filter(|c| self.focus_allowed() || *c == crate::FocusCommand::TakeComputer)
            .collect()
    }
    pub fn focus_allowed(&self) -> bool {
        self.state.phase == SpacePhase::Streaming
    }
    pub fn focus_message(&self, focus: &crate::FocusSnapshot) -> String {
        if self.state.phase == SpacePhase::Idle {
            "电脑就绪 · 开始后可切换焦点".into()
        } else {
            focus.message.clone()
        }
    }
    pub fn snapshot(&self) -> &SpaceSnapshot {
        &self.state
    }
    pub fn initialize(&mut self) -> SpaceEffect {
        self.effect(SpaceOperation::Inspect)
    }
    pub fn command(&mut self, command: SpaceCommand) -> Result<Option<SpaceEffect>, String> {
        if !self.state.available_commands.contains(&command) {
            return Err("当前状态不允许此操作".into());
        }
        match command {
            SpaceCommand::Start => {
                self.cancel = false;
                self.state.issue = None;
                Ok(Some(self.effect(SpaceOperation::CreateDisplay)))
            }
            SpaceCommand::Uninstall => {
                self.state.issue = None;
                Ok(Some(self.effect(SpaceOperation::Uninstall)))
            }
            SpaceCommand::Stop | SpaceCommand::Recover => {
                if command == SpaceCommand::Recover {
                    self.state.issue = None;
                }
                self.cancel = true;
                if self.state.pending.is_some() {
                    self.publish();
                    Ok(None)
                } else {
                    Ok(self.cleanup())
                }
            }
        }
    }
    pub fn complete(
        &mut self,
        effect: SpaceEffect,
        resources: SpaceResources,
        error: Option<String>,
    ) -> Option<SpaceEffect> {
        if self.state.pending != Some(effect) {
            return None;
        }
        self.state.pending = None;
        self.state.resources = resources;
        if let Some(error) = error {
            self.state.issue = Some(error);
            if matches!(
                effect.operation,
                SpaceOperation::CreateDisplay | SpaceOperation::StartMedia
            ) {
                self.cancel = true;
                return self.cleanup();
            }
            self.state.phase = SpacePhase::Faulted;
            self.publish();
            return None;
        }
        if self.cancel {
            return self.cleanup();
        }
        match effect.operation {
            SpaceOperation::Inspect => {
                if self.state.resources.clear() {
                    self.state.phase = SpacePhase::Idle;
                } else {
                    self.state.phase = SpacePhase::Faulted;
                    self.state.issue = Some("发现遗留资源，请先恢复并回收".into());
                }
                self.publish();
                None
            }
            SpaceOperation::CreateDisplay => {
                if self.state.resources.display != ResourceState::Running {
                    return self.bad_evidence("虚拟屏幕就绪未确认");
                }
                Some(self.effect(SpaceOperation::StartMedia))
            }
            SpaceOperation::StartMedia => {
                if self.state.resources.media != ResourceState::Running
                    || self.state.resources.game_window != ResourceState::Running
                {
                    return self.bad_evidence("音视频播放或游戏位置未确认");
                }
                self.state.phase = SpacePhase::Streaming;
                self.publish();
                None
            }
            SpaceOperation::Uninstall => {
                if self.state.resources.driver_installed || !self.state.resources.clear() {
                    return self.bad_evidence("驱动卸载未确认");
                }
                self.state.phase = SpacePhase::Idle;
                self.publish();
                None
            }
            _ => self.cleanup(),
        }
    }
    pub fn lost(&mut self, message: String) -> Option<SpaceEffect> {
        if self.state.phase != SpacePhase::Streaming {
            return None;
        }
        self.state.issue = Some(message);
        self.cancel = true;
        self.cleanup()
    }
    fn bad_evidence(&mut self, message: &str) -> Option<SpaceEffect> {
        self.state.issue = Some(message.into());
        self.cancel = true;
        self.cleanup()
    }
    fn cleanup(&mut self) -> Option<SpaceEffect> {
        let operation = if self.state.resources.media != ResourceState::Stopped {
            Some(SpaceOperation::StopMedia)
        } else if self.state.resources.game_window != ResourceState::Stopped {
            Some(SpaceOperation::RestoreGame)
        } else if self.state.resources.display != ResourceState::Stopped {
            Some(SpaceOperation::RemoveDisplay)
        } else {
            None
        };
        if let Some(operation) = operation {
            Some(self.effect(operation))
        } else {
            self.cancel = false;
            self.state.phase = if self.state.issue.is_some() {
                SpacePhase::Faulted
            } else {
                SpacePhase::Idle
            };
            self.publish();
            None
        }
    }
    fn effect(&mut self, operation: SpaceOperation) -> SpaceEffect {
        self.request += 1;
        let effect = SpaceEffect {
            request: self.request,
            operation,
        };
        self.state.pending = Some(effect);
        self.state.phase = match operation {
            SpaceOperation::Inspect => SpacePhase::Inspecting,
            SpaceOperation::CreateDisplay => SpacePhase::Creating,
            SpaceOperation::StartMedia => SpacePhase::Starting,
            SpaceOperation::StopMedia => SpacePhase::Stopping,
            SpaceOperation::RestoreGame => SpacePhase::Restoring,
            SpaceOperation::RemoveDisplay => SpacePhase::Reclaiming,
            SpaceOperation::Uninstall => SpacePhase::Uninstalling,
        };
        self.publish();
        effect
    }
    fn publish(&mut self) {
        self.state.revision += 1;
        self.state.message = if self.cancel
            && matches!(
                self.state.phase,
                SpacePhase::Creating | SpacePhase::Starting
            ) {
            "已请求结束，等待当前操作完成后回收"
        } else {
            match self.state.phase {
                SpacePhase::Inspecting => "正在检查屏幕与遗留会话",
                SpacePhase::Idle => "准备就绪 · 开始时创建电视屏幕",
                SpacePhase::Creating => "正在创建电视屏幕 · 请完成 Windows 授权",
                SpacePhase::Starting => "正在启动游戏并确认电视音画",
                SpacePhase::Streaming => "正在串流 · 电视屏幕已创建",
                SpacePhase::Stopping => "正在停止串流",
                SpacePhase::Restoring => "正在把游戏移回主屏",
                SpacePhase::Reclaiming => "正在回收电视屏幕 · 请完成 Windows 授权",
                SpacePhase::Uninstalling => "正在彻底卸载驱动 · 请完成 Windows 授权",
                SpacePhase::Faulted => "操作未完成 · 查看问题并恢复",
            }
        }
        .into();
        self.state.available_commands.clear();
        match self.state.phase {
            SpacePhase::Idle | SpacePhase::Faulted if self.state.pending.is_none() => {
                if self.state.resources.clear() {
                    self.state.available_commands.push(SpaceCommand::Start);
                    if self.state.resources.driver_installed {
                        self.state.available_commands.push(SpaceCommand::Uninstall);
                    }
                } else {
                    self.state.available_commands.push(SpaceCommand::Recover);
                }
            }
            SpacePhase::Creating
            | SpacePhase::Starting
            | SpacePhase::Streaming
            | SpacePhase::Inspecting
                if !self.cancel =>
            {
                self.state.available_commands.push(SpaceCommand::Stop)
            }
            _ => {}
        }
    }
}
