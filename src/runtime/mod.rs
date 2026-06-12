//! Process-wide shutdown coordination (Ctrl+C / SIGTERM).

pub mod budget;
pub mod profile;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct Shutdown {
    triggered: AtomicBool,
}

impl Shutdown {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            triggered: AtomicBool::new(false),
        })
    }

    pub fn trigger(&self) {
        self.triggered.store(true, Ordering::SeqCst);
    }

    pub fn is_triggered(&self) -> bool {
        self.triggered.load(Ordering::SeqCst)
    }

    pub fn install_ctrlc_handler(self: &Arc<Self>) {
        let flag = self.clone();
        let _ = ctrlc::set_handler(move || {
            tracing::info!("shutdown signal received");
            flag.trigger();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::budget::MemoryBudget;

    #[test]
    fn memory_budget_reserves_and_releases() {
        let budget = MemoryBudget::with_limit_mb(1);
        let full = 1024 * 1024;
        assert!(budget.try_reserve(full));
        assert!(!budget.try_reserve(1));
        budget.release(full);
        assert!(budget.try_reserve(1));
    }

    #[test]
    fn shutdown_starts_clear_and_triggers_once() {
        let shutdown = Shutdown::new();
        assert!(!shutdown.is_triggered());
        shutdown.trigger();
        assert!(shutdown.is_triggered());
        shutdown.trigger();
        assert!(shutdown.is_triggered());
    }
}
