//! Scope guard for RAII cleanup.

/// A guard that runs a closure when dropped.
pub struct ScopeGuard<F: FnOnce()> {
    f: Option<F>,
}

impl<F: FnOnce()> ScopeGuard<F> {
    /// Create a new scope guard.
    pub fn new(f: F) -> Self {
        Self { f: Some(f) }
    }

    /// Dismiss the guard, preventing the closure from running.
    pub fn dismiss(mut self) {
        self.f = None;
    }
}

impl<F: FnOnce()> Drop for ScopeGuard<F> {
    fn drop(&mut self) {
        if let Some(f) = self.f.take() {
            f();
        }
    }
}

/// Create a scope guard that runs the closure on drop.
pub fn scope_guard<F: FnOnce()>(f: F) -> ScopeGuard<F> {
    ScopeGuard::new(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_scope_guard_runs_on_drop() {
        let ran = AtomicBool::new(false);
        {
            let _guard = scope_guard(|| {
                ran.store(true, Ordering::SeqCst);
            });
            assert!(!ran.load(Ordering::SeqCst));
        }
        assert!(ran.load(Ordering::SeqCst));
    }

    #[test]
    fn test_scope_guard_dismiss() {
        let ran = AtomicBool::new(false);
        {
            let guard = scope_guard(|| {
                ran.store(true, Ordering::SeqCst);
            });
            guard.dismiss();
        }
        assert!(!ran.load(Ordering::SeqCst));
    }
}
