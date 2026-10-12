use serde::Deserialize;
use sofalink_core::{ClientPerformance, MediaSample};

/// Counters are reported by Surface/AAudio playback, never inferred from connection.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientMetrics {
    pub session_id: String,
    pub sequence: u64,
    pub rendered: u64,
    pub audio_played_frames: u64,
    pub fps: f64,
    pub receive_to_render_p50_ms: f64,
    pub audio_buffered_ms: f64,
    pub video_healthy: bool,
    pub audio_healthy: bool,
    #[serde(default)]
    pub performance: Option<ClientPerformance>,
}
impl ClientMetrics {
    pub fn sample(&self) -> MediaSample {
        MediaSample {
            sequence: self.sequence,
            rendered_frames: self.rendered,
            audio_played_frames: if self.audio_healthy {
                self.audio_played_frames
            } else {
                0
            },
            video_fps: if self.video_healthy { self.fps } else { 0.0 },
            receive_to_render_ms: self.receive_to_render_p50_ms,
            audio_buffer_ms: self.audio_buffered_ms,
        }
    }
}
pub fn parse(
    value: &serde_json::Value,
    session: &str,
) -> Result<Option<ClientMetrics>, serde_json::Error> {
    if value["type"] != "client" || value["session_id"].as_str() != Some(session) {
        return Ok(None);
    }
    let metrics: ClientMetrics = serde_json::from_value(value["metrics"].clone())?;
    if metrics.session_id != session || !metrics.sample().valid() {
        return Ok(None);
    }
    Ok(Some(metrics))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report() -> serde_json::Value {
        serde_json::json!({"type":"client","session_id":"current","metrics":{
            "sessionId":"current","sequence":4,"rendered":180,"audioPlayedFrames":144000,
            "fps":60.0,"receiveToRenderP50Ms":140.0,"audioBufferedMs":150.0,
            "videoHealthy":true,"audioHealthy":true}})
    }
    #[test]
    fn accepts_matching_session_and_actual_playback_counters() {
        let sample = parse(&report(), "current").unwrap().unwrap().sample();
        assert_eq!(sample.rendered_frames, 180);
        assert_eq!(sample.audio_played_frames, 144000);
    }
    #[test]
    fn rejects_old_outer_or_inner_session_and_missing_sequence() {
        let mut value = report();
        value["session_id"] = "old".into();
        assert!(parse(&value, "current").unwrap().is_none());
        value = report();
        value["metrics"]["sessionId"] = "old".into();
        assert!(parse(&value, "current").unwrap().is_none());
        value = report();
        value["metrics"].as_object_mut().unwrap().remove("sequence");
        assert!(parse(&value, "current").is_err());
    }
    #[test]
    fn invalid_metrics_or_failed_output_cannot_prove_playback() {
        let mut value = report();
        value["metrics"]["audioBufferedMs"] = (-1.0).into();
        assert!(parse(&value, "current").unwrap().is_none());
        value = report();
        value["metrics"]["audioHealthy"] = false.into();
        value["metrics"]["videoHealthy"] = false.into();
        let sample = parse(&value, "current").unwrap().unwrap().sample();
        assert_eq!(sample.audio_played_frames, 0);
        assert_eq!(sample.video_fps, 0.0);
    }
}
