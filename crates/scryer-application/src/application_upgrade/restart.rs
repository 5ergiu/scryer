use std::sync::Arc;

/// Process-restart callback supplied by the executable host.
///
/// The application crate owns this small boundary so an upgrade can schedule
/// its restart without depending on an HTTP or GraphQL layer.
#[derive(Clone)]
pub struct ApplicationUpgradeRestartHandle {
    schedule_fn: Arc<dyn Fn() + Send + Sync>,
    exit_fn: Arc<dyn Fn() + Send + Sync>,
    bundle_relaunch_fn: Arc<dyn Fn() + Send + Sync>,
}

impl ApplicationUpgradeRestartHandle {
    pub fn new(schedule: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            schedule_fn: Arc::new(schedule),
            exit_fn: Arc::new(|| {}),
            bundle_relaunch_fn: Arc::new(|| {}),
        }
    }

    pub fn new_with_exit(
        schedule: impl Fn() + Send + Sync + 'static,
        exit: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            schedule_fn: Arc::new(schedule),
            exit_fn: Arc::new(exit),
            bundle_relaunch_fn: Arc::new(|| {}),
        }
    }

    /// Add the macOS application-bundle relaunch action.
    ///
    /// Separate from [`Self::schedule_exit`] because the two mean different
    /// things to whatever is supervising this process: an ordinary exit is a
    /// stop, and this one is "the application was replaced, start the new one".
    #[must_use]
    pub fn with_bundle_relaunch(
        mut self,
        bundle_relaunch: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        self.bundle_relaunch_fn = Arc::new(bundle_relaunch);
        self
    }

    /// Ask the supervising wrapper to relaunch the replaced application bundle.
    pub fn schedule_bundle_relaunch(&self) {
        (self.bundle_relaunch_fn)();
    }

    pub fn schedule_restart(&self) {
        (self.schedule_fn)();
    }

    /// Request a delayed exit without launching a replacement process.
    /// Windows upgrade helpers use this after they have been detached.
    pub fn schedule_exit(&self) {
        (self.exit_fn)();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn exit_only_callback_does_not_schedule_a_restart() {
        let restarted = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let handle = ApplicationUpgradeRestartHandle::new_with_exit(
            {
                let restarted = restarted.clone();
                move || restarted.store(true, Ordering::SeqCst)
            },
            {
                let exited = exited.clone();
                move || exited.store(true, Ordering::SeqCst)
            },
        );

        handle.schedule_exit();
        assert!(exited.load(Ordering::SeqCst));
        assert!(!restarted.load(Ordering::SeqCst));
    }

    /// A bundle upgrade must not also restart the process in place: the binary
    /// it would re-exec has been replaced, and the wrapper is what starts the
    /// new one.
    #[test]
    fn a_bundle_relaunch_neither_restarts_nor_plainly_exits() {
        let restarted = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let relaunched = Arc::new(AtomicBool::new(false));
        let handle = ApplicationUpgradeRestartHandle::new_with_exit(
            {
                let restarted = restarted.clone();
                move || restarted.store(true, Ordering::SeqCst)
            },
            {
                let exited = exited.clone();
                move || exited.store(true, Ordering::SeqCst)
            },
        )
        .with_bundle_relaunch({
            let relaunched = relaunched.clone();
            move || relaunched.store(true, Ordering::SeqCst)
        });

        handle.schedule_bundle_relaunch();
        assert!(relaunched.load(Ordering::SeqCst));
        assert!(!restarted.load(Ordering::SeqCst));
        assert!(!exited.load(Ordering::SeqCst));
    }
}
