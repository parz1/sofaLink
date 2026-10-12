//! Display capability validation. Stream dimensions do not dictate output mode.
//! Mode changes belong to the platform adapter and are disabled by default.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f64,
}

impl DisplayMode {
    /// Validate capacity, not frame pacing or the TV's native panel resolution.
    /// Allow fractional broadcast rates such as 59.94 for a 60 fps target.
    pub fn check_target(self, target_fps: u32) -> Result<(), &'static str> {
        if self.width == 0
            || self.height == 0
            || !self.refresh_hz.is_finite()
            || self.refresh_hz <= 0.0
            || !(1..=240).contains(&target_fps)
        {
            return Err("Invalid display mode or target frame rate");
        }
        let target = f64::from(target_fps);
        if self.refresh_hz < target * (1000.0 / 1001.0) - 0.001 {
            return Err(
                "Current display refresh rate is below the streaming target; choose a suitable system display mode before starting",
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_output_resolution_independent_of_stream_resolution() {
        for (width, height, refresh_hz) in [
            (1920, 1080, 60.0),
            (3840, 2160, 59.94),
            (3840, 2160, 120.0),
            (1280, 720, 60.0),
        ] {
            assert!(
                DisplayMode {
                    width,
                    height,
                    refresh_hz
                }
                .check_target(60)
                .is_ok()
            );
        }
    }

    #[test]
    fn rejects_insufficient_or_unknown_output_capacity() {
        for refresh_hz in [30.0, 50.0, 59.0, 0.0, f64::NAN, f64::INFINITY] {
            assert!(
                DisplayMode {
                    width: 3840,
                    height: 2160,
                    refresh_hz
                }
                .check_target(60)
                .is_err()
            );
        }
        let mode = DisplayMode {
            width: 1920,
            height: 1080,
            refresh_hz: 60.0,
        };
        assert!(mode.check_target(120).is_err());
        assert!(mode.check_target(0).is_err());
        assert!(DisplayMode { width: 0, ..mode }.check_target(60).is_err());
        assert!(
            DisplayMode {
                refresh_hz: 29.97,
                ..mode
            }
            .check_target(30)
            .is_ok()
        );
    }
}
