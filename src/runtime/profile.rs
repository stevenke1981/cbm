//! Optional phase timing when `CBRLM_PROFILE=1`.

use std::time::Instant;
use tracing::info;

pub fn profiling_enabled() -> bool {
    matches!(
        std::env::var("CBRLM_PROFILE")
            .or_else(|_| std::env::var("CBM_PROFILE"))
            .as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    )
}

pub struct PhaseTimer {
    label: &'static str,
    start: Instant,
    enabled: bool,
}

impl PhaseTimer {
    pub fn start(label: &'static str) -> Self {
        Self {
            label,
            start: Instant::now(),
            enabled: profiling_enabled(),
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

impl Drop for PhaseTimer {
    fn drop(&mut self) {
        if self.enabled {
            info!(
                phase = self.label,
                duration_ms = self.elapsed_ms(),
                "profile"
            );
        }
    }
}
