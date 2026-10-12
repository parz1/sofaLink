use crate::TimeMs;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientPerformance {
    #[serde(default)]
    pub sequence: u64,
    pub frames: f64,
    pub render_gap_p95_ms: f64,
    pub render_gap_p99_ms: f64,
    pub render_gap_max_ms: f64,
    pub receive_gap_p95_ms: f64,
    pub queue_p50_ms: f64,
    pub queue_p95_ms: f64,
    pub decode_p50_ms: f64,
    pub decode_p95_ms: f64,
    pub presentation_p50_ms: f64,
    pub presentation_p95_ms: f64,
    pub sync_wait_p50_ms: f64,
    pub sync_wait_p95_ms: f64,
    pub surface_late_p95_ms: f64,
    pub callback_lag_p95_ms: f64,
    pub queue_depth_p95: f64,
    pub in_flight_p95: f64,
    #[serde(default)]
    pub audio_output_low_latency: Option<bool>,
    #[serde(default)]
    pub audio_output_buffer_ms: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_measurement_does_not_present_current_diagnosis() {
        let c = ClientPerformance {
            frames: 120.0,
            sync_wait_p50_ms: 90.0,
            ..Default::default()
        };
        let mut state = PerformanceState {
            client: Some(c),
            client_updated_ms: Some(100),
            ..Default::default()
        };
        state.refresh(101, 60);
        assert_eq!(state.message, "主要等待：音画同步调度");
        state.refresh(3101, 60);
        assert!(state.stale);
        assert_eq!(state.message, "逐帧反馈已过期");
    }
    #[test]
    fn invalid_timings_are_rejected() {
        let mut c = ClientPerformance {
            decode_p95_ms: f64::NAN,
            ..Default::default()
        };
        assert!(!c.valid());
        c.decode_p95_ms = -1.0;
        assert!(!c.valid());
    }
    #[test]
    fn frame_diagnostics_follow_target_budget() {
        let mut state = PerformanceState {
            client: Some(ClientPerformance {
                frames: 90.0,
                render_gap_p95_ms: 33.4,
                ..Default::default()
            }),
            client_updated_ms: Some(0),
            ..Default::default()
        };
        state.refresh(1, 30);
        assert_eq!(state.message, "当前窗口未见明显帧间隔异常");
        state.refresh(1, 60);
        assert_eq!(state.message, "呈现帧间隔波动，需查看逐帧报告");
        state.client.as_mut().unwrap().render_gap_p95_ms = 0.0;
        state.host = Some(HostPerformance {
            sequence: 1,
            encoded_fps: 30.0,
            encoder_p95_ms: Some(20.0),
        });
        state.host_updated_ms = Some(0);
        state.refresh(1, 30);
        assert_eq!(state.message, "当前窗口未见明显帧间隔异常");
        state.refresh(1, 60);
        assert_eq!(state.message, "编码耗时超过目标帧预算");
    }
}
impl ClientPerformance {
    pub fn valid(&self) -> bool {
        [
            self.frames,
            self.render_gap_p95_ms,
            self.render_gap_p99_ms,
            self.render_gap_max_ms,
            self.receive_gap_p95_ms,
            self.queue_p50_ms,
            self.queue_p95_ms,
            self.decode_p50_ms,
            self.decode_p95_ms,
            self.presentation_p50_ms,
            self.presentation_p95_ms,
            self.sync_wait_p50_ms,
            self.sync_wait_p95_ms,
            self.surface_late_p95_ms,
            self.callback_lag_p95_ms,
            self.queue_depth_p95,
            self.in_flight_p95,
        ]
        .iter()
        .all(|n| n.is_finite() && *n >= 0.0)
            && self
                .audio_output_buffer_ms
                .is_none_or(|n| n.is_finite() && n >= 0.0)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostPerformance {
    pub sequence: u64,
    pub encoded_fps: f64,
    pub encoder_p95_ms: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PerformanceState {
    pub client: Option<ClientPerformance>,
    pub host: Option<HostPerformance>,
    pub message: String,
    pub client_updated_ms: Option<TimeMs>,
    pub host_updated_ms: Option<TimeMs>,
    pub stale: bool,
    #[serde(default)]
    pub host_stale: bool,
}
impl Default for PerformanceState {
    fn default() -> Self {
        Self {
            client: None,
            host: None,
            message: "等待逐帧测量".into(),
            client_updated_ms: None,
            host_updated_ms: None,
            stale: false,
            host_stale: false,
        }
    }
}
impl PerformanceState {
    pub(crate) fn refresh(&mut self, now: TimeMs, target_fps: u32) {
        let frame_budget_ms = 1000.0 / f64::from(target_fps.max(1));
        self.host_stale = self
            .host_updated_ms
            .is_some_and(|t| now.saturating_sub(t) > 3000);
        self.stale = self
            .client_updated_ms
            .is_some_and(|t| now.saturating_sub(t) > 3000);
        self.message = if self.stale {
            "逐帧反馈已过期"
        } else if let Some(c) = &self.client {
            if c.frames < 30.0 {
                "逐帧测量采样中"
            } else if c.sync_wait_p50_ms > 30.0
                && c.sync_wait_p50_ms > c.queue_p50_ms + c.decode_p50_ms
            {
                if c.audio_output_low_latency == Some(false) {
                    "音频未启用低延迟 · 音画等待较长"
                } else if c.render_gap_p95_ms > frame_budget_ms * 1.5 {
                    "音画同步等待较长 · 呈现存在长帧"
                } else {
                    "主要等待：音画同步调度"
                }
            } else if c.queue_p95_ms > 30.0 {
                "卡顿线索：客户端排队"
            } else if c.decode_p95_ms > 30.0 {
                "卡顿线索：解码输出等待"
            } else if c.surface_late_p95_ms > 25.0 {
                "平台呈现通知与同步目标偏差（需合成器核对）"
            } else if c.callback_lag_p95_ms > 30.0 {
                "统计回调延迟较大"
            } else if c.render_gap_p95_ms > frame_budget_ms * 1.5 {
                "呈现帧间隔波动，需查看逐帧报告"
            } else if self
                .host_updated_ms
                .is_some_and(|t| now.saturating_sub(t) <= 3000)
                && self
                    .host
                    .as_ref()
                    .and_then(|h| h.encoder_p95_ms)
                    .is_some_and(|n| n > frame_budget_ms)
            {
                "编码耗时超过目标帧预算"
            } else {
                "当前窗口未见明显帧间隔异常"
            }
        } else {
            "等待逐帧测量"
        }
        .into();
    }
}
