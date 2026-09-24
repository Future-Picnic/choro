//! Diagnostics for upstream completion ownership, including dropped handles.
//!
//! Choro uses the official Turso 0.7.2 release. It still contains the leak
//! tracked by https://github.com/tursodatabase/turso/pull/7447, so the lifetime
//! assertions remain opt-in until that fix ships. This upgrade does not claim
//! to fix the leak. Recheck a future release with:
//! `cargo test -p ide-core --test turso_completion_lifetime -- --ignored`
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Weak,
};
use std::task::{Wake, Waker};
use turso_core::io::{Completion, CompletionGroup};
use turso_core::CompletionError;

fn observed_group() -> (CompletionGroup, Weak<()>) {
    let capture = Arc::new(());
    let weak = Arc::downgrade(&capture);
    let group = CompletionGroup::new(move |_| drop(Arc::clone(&capture)));
    (group, weak)
}

#[test]
#[ignore = "upstream Turso #7447 is not fixed in 0.7.2"]
fn completed_groups_release_their_captures_repeatedly() {
    // This is the synchronous path responsible for the original 288-byte
    // retained allocation pattern. A callback capture makes release observable.
    for _ in 0..10_000 {
        let (mut group, capture) = observed_group();
        group.add(&Completion::new_yield());
        let group = group.build();
        assert!(group.succeeded());
        drop(group);
        assert!(
            capture.upgrade().is_none(),
            "finished group retained callback"
        );
    }
}

#[test]
#[ignore = "upstream Turso #7447 is not fixed in 0.7.2"]
fn groups_with_already_failed_children_are_released() {
    let child = Completion::new_write(|_| {});
    child.error(CompletionError::Aborted);
    let (mut group, capture) = observed_group();
    group.add(&child);
    let group = group.build();
    assert_eq!(group.get_error(), Some(CompletionError::Aborted));
    drop(group);
    drop(child);
    assert!(capture.upgrade().is_none());
}

#[test]
#[ignore = "upstream Turso #7447 is not fixed in 0.7.2"]
fn pending_groups_are_released_after_success_or_error() {
    for fail in [false, true] {
        let child = Completion::new_write(|_| {});
        let (mut group, capture) = observed_group();
        group.add(&child);
        let group = group.build();
        if fail {
            child.abort();
            assert_eq!(group.get_error(), Some(CompletionError::Aborted));
        } else {
            child.complete(0);
            assert!(group.succeeded());
        }
        drop(child);
        drop(group);
        assert!(capture.upgrade().is_none());
    }
}

#[test]
#[ignore = "upstream Turso #7447 is not fixed in 0.7.2"]
fn abandoned_pending_groups_do_not_retain_themselves() {
    let child = Completion::new_write(|_| {});
    let (mut group, capture) = observed_group();
    group.add(&child);
    let group = group.build();
    drop(group);
    // The outstanding child must keep its parent's callback alive.
    assert!(capture.upgrade().is_some());
    drop(child);
    assert!(capture.upgrade().is_none());
}

#[test]
#[ignore = "upstream Turso #7447 is not fixed in 0.7.2"]
fn nested_groups_finish_after_intermediate_handles_are_dropped() {
    for fail in [false, true] {
        let first = Completion::new_write(|_| {});
        let second = Completion::new_write(|_| {});
        let (mut middle, middle_capture) = observed_group();
        middle.add(&first);
        middle.add(&second);
        let middle = middle.build();
        let (mut outer, outer_capture) = observed_group();
        outer.add(&middle);
        let outer = outer.build();
        drop(middle);
        first.complete(0);
        assert!(!outer.finished());
        if fail {
            second.abort();
            assert_eq!(outer.get_error(), Some(CompletionError::Aborted));
        } else {
            second.complete(0);
            assert!(outer.succeeded());
        }
        assert!(outer.finished());
        drop(first);
        drop(second);
        drop(outer);
        assert!(middle_capture.upgrade().is_none());
        assert!(outer_capture.upgrade().is_none());
    }
}

#[test]
#[ignore = "upstream Turso #7447 is not fixed in 0.7.2"]
fn dropping_all_group_handles_still_delivers_callback_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let first = Completion::new_write(|_| {});
    let second = Completion::new_write(|_| {});
    let mut group = CompletionGroup::new(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    group.add(&first);
    group.add(&second);
    drop(group.build());
    first.complete(0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    second.complete(0);
    second.complete(0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(first);
    drop(second);
    assert_eq!(Arc::strong_count(&calls), 1);
}

#[derive(Default)]
struct WakeCounter(AtomicUsize);

impl Wake for WakeCounter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn progress_and_final_completion_wake_the_group_waiter() {
    let wake_counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(Arc::clone(&wake_counter));
    let first = Completion::new_write(|_| {});
    let second = Completion::new_write(|_| {});
    let mut group = CompletionGroup::new(|_| {});
    group.add(&first);
    group.add(&second);
    let group = group.build();

    group.set_waker(&waker);
    first.wake_progress();
    assert_eq!(wake_counter.0.load(Ordering::SeqCst), 1);
    assert!(!group.finished());

    group.set_waker(&waker);
    first.complete(0);
    assert_eq!(wake_counter.0.load(Ordering::SeqCst), 2);
    assert!(!group.finished());

    group.set_waker(&waker);
    second.complete(0);
    assert_eq!(wake_counter.0.load(Ordering::SeqCst), 3);
    assert!(group.succeeded());
}
