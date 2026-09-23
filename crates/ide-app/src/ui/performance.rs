use std::time::{Duration, Instant};

const UI_SLOW_OPERATION_THRESHOLD: Duration = Duration::from_millis(50);

/// Compile-time opt-in counters. No timers, allocation, or logging in ordinary
/// builds. Profiling builds report local five-second windows to stderr.
pub(crate) struct UiProbe {
    #[cfg(any(test, feature = "ui-performance"))]
    label: &'static str,
    #[cfg(any(test, feature = "ui-performance"))]
    start: Instant,
}

impl UiProbe {
    #[inline]
    pub(crate) fn new(_label: &'static str) -> Self {
        Self {
            #[cfg(any(test, feature = "ui-performance"))]
            label: _label,
            #[cfg(any(test, feature = "ui-performance"))]
            start: Instant::now(),
        }
    }
}

impl Drop for UiProbe {
    #[inline]
    fn drop(&mut self) {
        #[cfg(any(test, feature = "ui-performance"))]
        probes::record(self.label, self.start.elapsed());
    }
}

#[cfg(any(test, feature = "ui-performance"))]
pub(crate) mod probes {
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap};

    #[derive(Default, Clone)]
    pub(crate) struct Samples {
        pub count: u64,
        pub micros: Vec<u64>,
    }

    impl Samples {
        pub fn percentile(&self, percentile: usize) -> u64 {
            let mut sorted = self.micros.clone();
            sorted.sort_unstable();
            sorted
                .get(sorted.len().saturating_sub(1) * percentile / 100)
                .copied()
                .unwrap_or(0)
        }
    }

    thread_local! {
        static DATA: RefCell<(Instant, BTreeMap<&'static str, Samples>)> =
            RefCell::new((Instant::now(), BTreeMap::new()));
    }

    pub(crate) fn record(label: &'static str, elapsed: Duration) {
        DATA.with(|data| {
            let mut data = data.borrow_mut();
            let samples = data.1.entry(label).or_default();
            samples.count += 1;
            if samples.micros.len() < 20_000 {
                samples.micros.push(elapsed.as_micros() as u64);
            }
            #[cfg(not(test))]
            if data.0.elapsed() >= Duration::from_secs(5) {
                for (label, samples) in &data.1 {
                    eprintln!(
                        "[ui-profile] {label}: count={} p95={}us p99={}us",
                        samples.count,
                        samples.percentile(95),
                        samples.percentile(99)
                    );
                }
                data.0 = Instant::now();
                data.1.clear();
            }
        });
    }

    #[cfg(test)]
    pub(crate) fn snapshot() -> BTreeMap<&'static str, Samples> {
        DATA.with(|data| data.borrow().1.clone())
    }
}

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

#[cfg(all(test, feature = "ui-layout-tests"))]
mod tests {
    use gpui::{
        div, px, AnyView, AppContext, Context, Entity, EntityInputHandler, IntoElement,
        ParentElement, Render, StyleRefinement, Styled, Window,
    };
    use gpui_component::input::{Input, InputEvent, InputState};
    use std::{cell::Cell, rc::Rc};

    struct TranscriptFixture(Rc<Cell<usize>>);
    impl Render for TranscriptFixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0.set(self.0.get() + 1);
            div()
                .size_full()
                .child("A settled conversation stays cached while typing")
        }
    }

    struct ComposerFixture {
        input: Entity<InputState>,
        transcript: Entity<TranscriptFixture>,
        cached: bool,
        changes: usize,
        renders: Rc<Cell<usize>>,
    }
    impl Render for ComposerFixture {
        fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let transcript = AnyView::from(self.transcript.clone());
            let transcript = if self.cached {
                transcript.cached(StyleRefinement::default().flex_1().w_full().min_h(px(0.)))
            } else {
                transcript
            };
            div()
                .flex()
                .flex_col()
                .size_full()
                .child(transcript)
                // This label stands for Send/mention/preview controls: it must
                // update on InputState's notification without notifying the owner.
                .child(format!("draft changes: {}", self.changes))
                .child(Input::new(&self.input))
        }
    }

    #[gpui::test]
    fn composer_edits_repaint_controls_without_rebuilding_transcript(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);
        for cached in [false, true] {
            let transcript_renders = Rc::new(Cell::new(0));
            let composer_renders = Rc::new(Cell::new(0));
            let mut fixture = None;
            let (_, cx) = cx.add_window_view(|window, cx| {
                let view = cx.new(|cx| {
                    let input = cx.new(|cx| InputState::new(window, cx));
                    cx.subscribe(
                        &input,
                        |this: &mut ComposerFixture, _, event: &InputEvent, _| {
                            if matches!(event, InputEvent::Change) {
                                this.changes += 1;
                            }
                        },
                    )
                    .detach();
                    let owner = cx.entity();
                    let transcript = cx.new(|cx| {
                        cx.observe(&owner, |_, _, cx| cx.notify()).detach();
                        TranscriptFixture(transcript_renders.clone())
                    });
                    ComposerFixture {
                        input,
                        transcript,
                        cached,
                        changes: 0,
                        renders: composer_renders.clone(),
                    }
                });
                fixture = Some(view.clone());
                gpui_component::Root::new(view, window, cx)
            });
            let view = fixture.unwrap();
            cx.run_until_parked();
            let before_transcript = transcript_renders.get();
            let before_composer = composer_renders.get();
            for _ in 0..50 {
                cx.update(|window, cx| {
                    let input = view.read(cx).input.clone();
                    input.update(cx, |input, cx| {
                        input.replace_text_in_range(None, "a", window, cx)
                    });
                });
                cx.run_until_parked();
            }
            let renders = transcript_renders.get() - before_transcript;
            eprintln!("50 edits, cached={cached}: transcript renders={renders}");
            assert!(
                composer_renders.get() >= before_composer + 50,
                "composer controls must still redraw"
            );
            cx.update(|_, cx| assert_eq!(view.read(cx).changes, 50));
            if cached {
                assert_eq!(renders, 0);
            } else {
                assert!(renders >= 50);
            }

            // A real chat change must invalidate the transcript immediately.
            let before = transcript_renders.get();
            cx.update(|_, cx| view.update(cx, |_, cx| cx.notify()));
            cx.run_until_parked();
            assert!(transcript_renders.get() > before);

            let before = transcript_renders.get();
            cx.simulate_resize(gpui::size(px(640.), px(720.)));
            cx.run_until_parked();
            assert!(
                transcript_renders.get() > before,
                "resizing must reflow the transcript"
            );
        }
    }
}
