//! Capability-based low-latency AVC policy; no device names or I/O.
#[derive(Debug, Clone, Copy)]
pub struct AvcCapabilities {
    pub high: bool,
    pub baseline: bool,
    pub on_demand_idr: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodingPolicy {
    pub profile: &'static str,
    pub gop_frames: i32,
    pub vbv_kbit: u32,
}

impl EncodingPolicy {
    pub fn negotiate(
        encoder: AvcCapabilities,
        decoder: AvcCapabilities,
        bitrate_kbit: u32,
        fps: u32,
    ) -> Result<Self, &'static str> {
        if !(1000..=100000).contains(&bitrate_kbit) || !(1..=240).contains(&fps) {
            return Err("Invalid video bitrate or frame rate");
        }
        if !encoder.on_demand_idr || !decoder.on_demand_idr {
            return Err("Low-latency streaming requires on-demand IDR recovery");
        }
        let profile = if encoder.high && decoder.high {
            "high"
        } else if encoder.baseline && decoder.baseline {
            "baseline"
        } else {
            return Err("No common AVC profile");
        };
        Ok(Self {
            profile,
            // Avoid periodic I/P reconstruction pumping. Recover explicitly.
            gop_frames: -1,
            vbv_kbit: (bitrate_kbit / fps).max(1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FULL: AvcCapabilities = AvcCapabilities {
        high: true,
        baseline: true,
        on_demand_idr: true,
    };
    #[test]
    fn selects_common_profile_and_scales_one_frame_budget() {
        let plan = EncodingPolicy::negotiate(FULL, FULL, 20000, 60).unwrap();
        assert_eq!(plan.profile, "high");
        assert_eq!(plan.gop_frames, -1);
        assert_eq!(plan.vbv_kbit, 333);
        let baseline_only = AvcCapabilities {
            high: false,
            ..FULL
        };
        assert_eq!(
            EncodingPolicy::negotiate(FULL, baseline_only, 20000, 120)
                .unwrap()
                .profile,
            "baseline"
        );
        assert_eq!(
            EncodingPolicy::negotiate(FULL, FULL, 20000, 120)
                .unwrap()
                .vbv_kbit,
            166
        );
    }
    #[test]
    fn rejects_missing_recovery_or_incompatible_profiles() {
        assert!(
            EncodingPolicy::negotiate(
                FULL,
                AvcCapabilities {
                    on_demand_idr: false,
                    ..FULL
                },
                20000,
                60
            )
            .is_err()
        );
        assert!(
            EncodingPolicy::negotiate(
                FULL,
                AvcCapabilities {
                    high: false,
                    baseline: false,
                    ..FULL
                },
                20000,
                60
            )
            .is_err()
        );
        assert!(EncodingPolicy::negotiate(FULL, FULL, 20000, 0).is_err());
    }
}
