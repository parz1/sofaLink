use serde::{Deserialize, Serialize};

/// Milliseconds on the host's monotonic clock, never a remote device's clock.
pub type TimeMs = u64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub session: u64,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    DesktopStream,
    /// Reserved for a future local/media-server provider. Currently unsupported.
    MediaPlayback,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    StartingHost,
    StartingClient,
    WaitingMedia,
    Playing,
    Degraded,
    Recovering,
    Stopping,
    Faulted,
}

impl Phase {
    pub fn active(self) -> bool {
        !matches!(self, Self::Idle | Self::Faulted)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceState {
    #[default]
    Stopped,
    Starting,
    Running,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resources {
    pub host: ResourceState,
    pub client: ResourceState,
}

impl Resources {
    pub fn clear(self) -> bool {
        self.host == ResourceState::Stopped && self.client == ResourceState::Stopped
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalState {
    #[default]
    Waiting,
    Healthy,
    Stalled,
    Missing,
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaSample {
    pub sequence: u64,
    pub rendered_frames: u64,
    pub audio_played_frames: u64,
    pub video_fps: f64,
    pub receive_to_render_ms: f64,
    pub audio_buffer_ms: f64,
}

impl MediaSample {
    pub fn valid(&self) -> bool {
        [
            self.video_fps,
            self.receive_to_render_ms,
            self.audio_buffer_ms,
        ]
        .into_iter()
        .all(|n| n.is_finite() && n >= 0.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputStatus {
    #[default]
    Unavailable,
    Waiting,
    Ready,
    Disconnected,
    Error,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputState {
    pub status: InputStatus,
    pub message: String,
    pub applied_events: u64,
}
impl InputState {
    pub fn new(status: InputStatus, message: impl Into<String>, applied_events: u64) -> Self {
        Self {
            status,
            message: message.into(),
            applied_events,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    StartupFailed,
    StartupTimeout,
    VideoStalled,
    AudioStalled,
    FeedbackExpired,
    HostExited,
    ClientExited,
    RecoveryFailed,
    CleanupFailed,
    HostAudioFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub code: IssueCode,
    pub message: String,
}

impl Issue {
    pub fn new(code: IssueCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    Start { activity: ActivityKind },
    Stop,
    Retry,
    Recover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandError {
    NotAvailable,
    UnsupportedActivity,
    ClockWentBackward,
    InvalidStreamRequest,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CommandError {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub token: Token,
    pub activity: ActivityKind,
    pub phase: Phase,
    pub message: String,
    pub video: SignalState,
    pub audio: SignalState,
    #[serde(default)]
    pub host_audio: HostAudioState,
    #[serde(default)]
    pub input: InputState,
    #[serde(default)]
    pub application: ApplicationState,
    #[serde(default)]
    pub performance: crate::PerformanceState,
    #[serde(default)]
    pub stream_request: crate::StreamRequest,
    #[serde(default)]
    pub media_negotiation: Option<crate::MediaNegotiation>,
    pub resources: Resources,
    pub metrics: Option<MediaSample>,
    pub last_feedback_ms: Option<TimeMs>,
    pub issue: Option<Issue>,
    /// These commands are decided by core, not inferred by a frontend.
    pub available_commands: Vec<Command>,
    pub supported_activities: Vec<ActivityKind>,
}

impl Snapshot {
    pub fn primary_command(&self) -> Option<Command> {
        self.available_commands.first().copied()
    }
    pub fn can(&self, command: Command) -> bool {
        self.available_commands.contains(&command)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectKind {
    Check,
    StartHost,
    StartClient,
    RestartMedia,
    Cleanup,
    HostAudio { mute: bool, request: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    pub token: Token,
    pub kind: EffectKind,
}

#[derive(Clone, Debug)]
pub enum EventKind {
    Application(ApplicationState),
    Checked,
    Negotiated(crate::MediaNegotiation),
    HostReady,
    ClientReady,
    Media(MediaSample),
    Input(InputState),
    ClientPerformance(crate::ClientPerformance),
    HostPerformance(crate::HostPerformance),
    Recovered,
    HostExited,
    ClientExited,
    Failed(Issue),
    HostAudio {
        request: u64,
        status: HostAudioStatus,
        message: String,
    },
    CleanupResult {
        host_stopped: bool,
        client_stopped: bool,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAudioStatus {
    #[default]
    Disabled,
    Original,
    Muting,
    Muted,
    Restoring,
    Error,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostAudioState {
    pub enabled: bool,
    pub status: HostAudioStatus,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub token: Token,
    pub kind: EventKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationTarget {
    #[default]
    Desktop,
    SteamBigPicture,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationStatus {
    #[default]
    Inactive,
    Starting,
    Ready,
    Stopping,
    Error,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationState {
    pub target: ApplicationTarget,
    pub status: ApplicationStatus,
    pub message: String,
}
