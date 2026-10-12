use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scenario {
    #[default]
    Healthy,
    StartupFail,
    StartupTimeout,
    AudioStall,
    VideoStall,
    Disconnect,
    StopFail,
}

impl Scenario {
    pub const ALL: [Self; 7] = [
        Self::Healthy,
        Self::StartupFail,
        Self::StartupTimeout,
        Self::AudioStall,
        Self::VideoStall,
        Self::Disconnect,
        Self::StopFail,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::StartupFail => "startup-fail",
            Self::StartupTimeout => "startup-timeout",
            Self::AudioStall => "audio-stall",
            Self::VideoStall => "video-stall",
            Self::Disconnect => "disconnect",
            Self::StopFail => "stop-fail",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "正常播放",
            Self::StartupFail => "启动失败",
            Self::StartupTimeout => "启动超时",
            Self::AudioStall => "声音停止",
            Self::VideoStall => "画面停止",
            Self::Disconnect => "连接中断",
            Self::StopFail => "结束失败",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Healthy => "开始后音画正常；结束时确认资源释放。",
            Self::StartupFail => "首次主机启动失败并清理；重试可成功。",
            Self::StartupTimeout => "首次客户端无响应；超时清理后可重试。",
            Self::AudioStall => "播放三秒后声音停止；恢复后重新确认音画。",
            Self::VideoStall => "播放三秒后画面停止；恢复后重新确认音画。",
            Self::Disconnect => "播放三秒后反馈中断；不会继续显示播放正常。",
            Self::StopFail => "首次结束无法释放主机；重试清理后回到就绪。",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == value)
    }
}
