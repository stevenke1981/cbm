//! Cooperative memory budget for indexing and scans.

use std::sync::atomic::{AtomicUsize, Ordering};

pub struct MemoryBudget {
    limit_bytes: usize,
    used: AtomicUsize,
}

impl MemoryBudget {
    pub fn from_env() -> Self {
        let mb: usize = std::env::var("CBRLM_MEMORY_BUDGET_MB")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(512);
        Self {
            limit_bytes: mb.saturating_mul(1024 * 1024),
            used: AtomicUsize::new(0),
        }
    }

    pub fn with_limit_mb(mb: usize) -> Self {
        Self {
            limit_bytes: mb.saturating_mul(1024 * 1024),
            used: AtomicUsize::new(0),
        }
    }

    pub fn limit_bytes(&self) -> usize {
        self.limit_bytes
    }

    pub fn used_bytes(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    pub fn try_reserve(&self, bytes: usize) -> bool {
        loop {
            let current = self.used.load(Ordering::Relaxed);
            if current.saturating_add(bytes) > self.limit_bytes {
                return false;
            }
            if self
                .used
                .compare_exchange_weak(
                    current,
                    current + bytes,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return true;
            }
        }
    }

    pub fn release(&self, bytes: usize) {
        self.used.fetch_sub(bytes, Ordering::Relaxed);
    }
}
