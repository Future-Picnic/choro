use std::time::{Duration, Instant};

const UI_SLOW_OPERATION_THRESHOLD: Duration = Duration::from_millis(50);

/// Emits a local diagnostic when work performed during a GPUI callback takes
/// long enough to be perceptible. This never sends data off the machine.
pub(crate) struct UiOperationTimer {
    label: &'static str,
    started_at: Instant,
}

impl UiOperationTimer {
    pub(crate) fn start(label: &'static str) -> Self {
        Self {
            label,
            started_at: Instant::now(),
        }
    }
}

impl Drop for UiOperationTimer {
    fn drop(&mut self) {
        let elapsed = self.started_at.elapsed();
        if elapsed >= UI_SLOW_OPERATION_THRESHOLD {
            eprintln!(
                "[performance] slow UI operation `{}` took {:.1} ms",
                self.label,
                elapsed.as_secs_f64() * 1_000.0
            );
        }
    }
}
