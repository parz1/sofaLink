//! Versioned, platform-independent media contracts and strict capability negotiation.
use crate::{AvcCapabilities, DisplayMode, EncodingPolicy};
use serde::{Deserialize, Serialize};

pub const MEDIA_SCHEMA_VERSION: u32 = 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    Avc,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoMode {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    Sdr709,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleFormat {
    PcmS16Le,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u32,
    pub sample_format: SampleFormat,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioCapability {
    pub format: AudioFormat,
    /// None for capture-only adapters. Output adapters report the granted mode.
    pub low_latency_output: Option<bool>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamRequest {
    pub version: u32,
    pub codec: VideoCodec,
    pub video: VideoMode,
    pub bitrate_kbit: u32,
    pub color: ColorSpace,
    pub audio: AudioFormat,
    pub require_low_latency_audio: bool,
}
impl Default for StreamRequest {
    fn default() -> Self {
        Self {
            version: MEDIA_SCHEMA_VERSION,
            codec: VideoCodec::Avc,
            video: VideoMode {
                width: 1920,
                height: 1080,
                fps: 60,
            },
            bitrate_kbit: 20000,
            color: ColorSpace::Sdr709,
            audio: AudioFormat {
                sample_rate: 48000,
                channels: 2,
                sample_format: SampleFormat::PcmS16Le,
            },
            require_low_latency_audio: true,
        }
    }
}
impl StreamRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        let v = self.video;
        if self.version != MEDIA_SCHEMA_VERSION {
            return Err("Unsupported media request version");
        }
        if !(160..=4096).contains(&v.width)
            || !(160..=4096).contains(&v.height)
            || !v.width.is_multiple_of(2)
            || !v.height.is_multiple_of(2)
            || !(1..=240).contains(&v.fps)
            || !(1000..=100000).contains(&self.bitrate_kbit)
            || !(8000..=192000).contains(&self.audio.sample_rate)
            || !(1..=8).contains(&self.audio.channels)
        {
            return Err("Invalid stream request");
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoCapability {
    pub implementation: String,
    pub codec: VideoCodec,
    /// This exact mode/profile combination has been queried by the adapter.
    pub mode: VideoMode,
    pub high: bool,
    pub baseline: bool,
    pub hardware: bool,
    pub on_demand_idr: bool,
    pub color: ColorSpace,
    pub max_bitrate_kbit: u32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceCapabilities {
    pub version: u32,
    pub video: Vec<VideoCapability>,
    pub audio: Vec<AudioCapability>,
    /// Output signal, not the display panel's native resolution.
    pub display: Option<DisplayMode>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaPlan {
    pub version: u32,
    pub request: StreamRequest,
    pub encoder: String,
    pub decoder: String,
    pub profile: String,
    pub gop_frames: i32,
    pub vbv_kbit: u32,
    pub display: Option<DisplayMode>,
    pub preserve_display_mode: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaNegotiation {
    pub host: DeviceCapabilities,
    pub client: DeviceCapabilities,
    pub plan: MediaPlan,
}
pub fn negotiate_media(
    request: &StreamRequest,
    host: &DeviceCapabilities,
    client: &DeviceCapabilities,
) -> Result<MediaPlan, &'static str> {
    request.validate()?;
    if host.version != MEDIA_SCHEMA_VERSION || client.version != MEDIA_SCHEMA_VERSION {
        return Err("Unsupported device capability version");
    }
    if !host.audio.iter().any(|cap| cap.format == request.audio)
        || !client.audio.iter().any(|cap| cap.format == request.audio)
    {
        return Err("No common requested audio format");
    }
    if request.require_low_latency_audio
        && !client
            .audio
            .iter()
            .any(|cap| cap.format == request.audio && cap.low_latency_output == Some(true))
    {
        return Err("Requested audio format has no granted low-latency output path");
    }
    let display = client
        .display
        .ok_or("Client display capability is unknown")?;
    display.check_target(request.video.fps)?;
    for encoder in &host.video {
        for decoder in &client.video {
            if encoder.implementation.is_empty()
                || decoder.implementation.is_empty()
                || !encoder.hardware
                || !decoder.hardware
                || encoder.codec != request.codec
                || decoder.codec != request.codec
                || encoder.mode != request.video
                || decoder.mode != request.video
                || encoder.color != request.color
                || decoder.color != request.color
                || encoder.max_bitrate_kbit < request.bitrate_kbit
                || decoder.max_bitrate_kbit < request.bitrate_kbit
            {
                continue;
            }
            let caps = |v: &VideoCapability| AvcCapabilities {
                high: v.high,
                baseline: v.baseline,
                on_demand_idr: v.on_demand_idr,
            };
            if let Ok(policy) = EncodingPolicy::negotiate(
                caps(encoder),
                caps(decoder),
                request.bitrate_kbit,
                request.video.fps,
            ) {
                return Ok(MediaPlan {
                    version: MEDIA_SCHEMA_VERSION,
                    request: request.clone(),
                    encoder: encoder.implementation.clone(),
                    decoder: decoder.implementation.clone(),
                    profile: policy.profile.into(),
                    gop_frames: policy.gop_frames,
                    vbv_kbit: policy.vbv_kbit,
                    display: Some(display),
                    preserve_display_mode: true,
                });
            }
        }
    }
    Err("No common hardware AVC mode/profile with SDR709 and on-demand IDR recovery")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn caps(request: &StreamRequest) -> DeviceCapabilities {
        DeviceCapabilities {
            version: MEDIA_SCHEMA_VERSION,
            video: vec![VideoCapability {
                implementation: "test-adapter".into(),
                codec: VideoCodec::Avc,
                mode: request.video,
                high: true,
                baseline: true,
                hardware: true,
                on_demand_idr: true,
                color: request.color,
                max_bitrate_kbit: 100000,
            }],
            audio: vec![AudioCapability {
                format: request.audio,
                low_latency_output: Some(true),
            }],
            display: Some(DisplayMode {
                width: 3840,
                height: 2160,
                refresh_hz: 59.94,
            }),
        }
    }
    #[test]
    fn negotiates_exact_non_default_request_and_preserves_output() {
        let r = StreamRequest {
            video: VideoMode {
                width: 1280,
                height: 720,
                fps: 60,
            },
            bitrate_kbit: 12000,
            audio: AudioFormat {
                sample_rate: 44100,
                ..StreamRequest::default().audio
            },
            ..StreamRequest::default()
        };
        let host = caps(&r);
        let mut client = caps(&r);
        client.video[0].high = false;
        let plan = negotiate_media(&r, &host, &client).unwrap();
        assert_eq!(plan.request, r);
        assert_eq!(plan.profile, "baseline");
        assert_eq!(plan.vbv_kbit, 200);
        assert_eq!(plan.display.unwrap().width, 3840);
    }
    #[test]
    fn never_silently_downgrades_unknown_or_unsupported_capabilities() {
        let r = StreamRequest::default();
        let host = caps(&r);
        let original = caps(&r);
        let mut bad = original.clone();
        bad.video[0].mode.width = 1280;
        assert!(negotiate_media(&r, &host, &bad).is_err());
        bad = original.clone();
        bad.audio.clear();
        assert!(negotiate_media(&r, &host, &bad).is_err());
        bad = original.clone();
        bad.video[0].on_demand_idr = false;
        assert!(negotiate_media(&r, &host, &bad).is_err());
        bad = original.clone();
        bad.display = None;
        assert!(negotiate_media(&r, &host, &bad).is_err());
        bad = original.clone();
        bad.display.as_mut().unwrap().refresh_hz = 30.0;
        assert!(negotiate_media(&r, &host, &bad).is_err());
        bad = original.clone();
        bad.version += 1;
        assert!(negotiate_media(&r, &host, &bad).is_err());
        bad = original.clone();
        bad.video[0].max_bitrate_kbit = 1000;
        assert!(negotiate_media(&r, &host, &bad).is_err());
        bad = original;
        bad.video[0].hardware = false;
        assert!(negotiate_media(&r, &host, &bad).is_err());
    }
    #[test]
    fn compatible_audio_requires_explicit_opt_in_and_never_changes_format() {
        let mut request = StreamRequest::default();
        let host = caps(&request);
        let mut client = caps(&request);
        client.audio[0].low_latency_output = Some(false);
        assert!(negotiate_media(&request, &host, &client).is_err());
        client.audio[0].low_latency_output = None;
        assert!(negotiate_media(&request, &host, &client).is_err());
        request.require_low_latency_audio = false;
        let plan = negotiate_media(&request, &host, &client).unwrap();
        assert_eq!(plan.request.audio, request.audio);
        assert!(!plan.request.require_low_latency_audio);
    }
}
