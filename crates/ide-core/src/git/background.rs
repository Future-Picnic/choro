//! One process-wide limit for background Git readers and mutations.
use std::{
    cell::Cell,
    marker::PhantomData,
    rc::Rc,
    sync::{Condvar, Mutex, OnceLock},
};

thread_local! { static DEPTH: Cell<usize> = const { Cell::new(0) }; }
fn slots() -> &'static (Mutex<usize>, Condvar) {
    static SLOTS: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
    SLOTS.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

/// Nested synchronous Git helpers share their caller's slot. This guard cannot
/// cross an await or move to another thread.
pub struct BackgroundGitPermit(PhantomData<Rc<()>>);
impl BackgroundGitPermit {
    pub fn acquire() -> Self {
        crate::blocking_guard::debug_warn_if_ui_thread("BackgroundGitPermit::acquire");
        DEPTH.with(|depth| {
            if depth.get() == 0 {
                let (lock, wake) = slots();
                let mut active = lock.lock().unwrap_or_else(|e| e.into_inner());
                while *active >= 2 {
                    active = wake.wait(active).unwrap_or_else(|e| e.into_inner());
                }
                *active += 1;
            }
            depth.set(depth.get() + 1);
        });
        Self(PhantomData)
    }
}
impl Drop for BackgroundGitPermit {
    fn drop(&mut self) {
        DEPTH.with(|depth| {
            depth.set(depth.get() - 1);
            if depth.get() == 0 {
                let (lock, wake) = slots();
                *lock.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
                wake.notify_one();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    #[test]
    fn all_callers_share_two_slots_and_nested_helpers_do_not_deadlock() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let (active, peak) = (active.clone(), peak.clone());
                scope.spawn(move || {
                    let _outer = BackgroundGitPermit::acquire();
                    let _inner = BackgroundGitPermit::acquire();
                    let n = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(n, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        assert!(peak.load(Ordering::SeqCst) <= 2);
    }
}
