//! Animated paint is a sibling of cached sidebar content. Its clock must never
//! notify ProjectList or rebuild the row data.
use gpui::{
    canvas, px, App, AppContext, Bounds, ContentMask, Context, Entity, Hsla, IntoElement, Pixels,
    Render, Styled, Window,
};
use std::{cell::RefCell, rc::Rc, time::Instant};

#[derive(Clone)]
struct Anchor {
    bounds: Bounds<Pixels>,
    mask: ContentMask<Pixels>,
    color: Option<Hsla>,
}

#[derive(Clone, Default)]
pub(super) struct ActivityAnchors {
    anchors: Rc<RefCell<Vec<Anchor>>>,
    groups: Rc<RefCell<Vec<ActivityAnchors>>>,
}

impl ActivityAnchors {
    pub fn clear(&self) {
        self.anchors.borrow_mut().clear();
        self.groups.borrow_mut().clear();
    }

    pub fn visible_group(&self, group: &Self) -> impl IntoElement {
        let groups = self.groups.clone();
        let group = group.clone();
        canvas(
            move |_, _, _| groups.borrow_mut().push(group.clone()),
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_0()
    }

    fn visible_anchors(&self) -> Vec<Anchor> {
        let mut anchors = self.anchors.borrow().clone();
        for group in self.groups.borrow().iter() {
            anchors.extend(group.anchors.borrow().iter().cloned());
        }
        anchors
    }
    pub fn begin_prepaint(&self) -> impl IntoElement {
        let anchors = self.clone();
        canvas(move |_, _, _| anchors.clear(), |_, _, _, _| {})
            .absolute()
            .top_0()
            .left_0()
            .size_0()
    }

    pub fn placeholder(&self, diameter: f32, color: Option<Hsla>) -> gpui::AnyElement {
        let anchors = self.clone();
        canvas(
            move |bounds, window, _| {
                let mask = window.content_mask();
                let visible = bounds.intersect(&mask.bounds);
                if visible.size.width > px(0.) && visible.size.height > px(0.) {
                    anchors.anchors.borrow_mut().push(Anchor {
                        bounds,
                        mask,
                        color,
                    });
                }
            },
            |_, _, _, _| {},
        )
        .size(px(diameter))
        .flex_shrink_0()
        .into_any_element()
    }
}

// Use an OS event, not a polling clock, to restart after Reduce Motion is
// disabled. The Objective-C callback only sends a thread-safe wake hint.
#[cfg(target_os = "macos")]
struct MotionObserver {
    center: objc2::rc::Retained<objc2_foundation::NSNotificationCenter>,
    token:
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>,
}
#[cfg(target_os = "macos")]
impl MotionObserver {
    fn new(wake: async_channel::Sender<()>) -> Self {
        let center = objc2_app_kit::NSWorkspace::sharedWorkspace().notificationCenter();
        let callback = block2::RcBlock::new(
            move |_: std::ptr::NonNull<objc2_foundation::NSNotification>| {
                let _ = wake.try_send(());
            },
        );
        // No notification payload is dereferenced and the captured sender is Send.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(objc2_app_kit::NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                None,
                None,
                &callback,
            )
        };
        Self { center, token }
    }
}
#[cfg(target_os = "macos")]
impl Drop for MotionObserver {
    fn drop(&mut self) {
        unsafe {
            self.center.removeObserver((*self.token).as_ref());
        }
    }
}

pub(crate) struct SidebarActivityLayer {
    anchors: ActivityAnchors,
    #[cfg(target_os = "macos")]
    _motion_observer: MotionObserver,
    started: Instant,
    #[cfg(feature = "ui-performance")]
    profile_static: bool,
    #[cfg(any(test, feature = "ui-performance"))]
    last_frame: Rc<std::cell::Cell<Option<Instant>>>,
}

impl SidebarActivityLayer {
    pub(super) fn pause(&mut self) {
        #[cfg(any(test, feature = "ui-performance"))]
        self.last_frame.set(None);
    }

    pub(super) fn new(anchors: ActivityAnchors, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            #[cfg(target_os = "macos")]
            let observer = {
                let (sender, receiver) = async_channel::bounded(1);
                cx.spawn(async move |this, cx| {
                    while receiver.recv().await.is_ok() {
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                })
                .detach();
                MotionObserver::new(sender)
            };
            #[cfg(not(target_os = "macos"))]
            let _ = cx;
            Self {
                anchors,
                #[cfg(target_os = "macos")]
                _motion_observer: observer,
                started: Instant::now(),
                #[cfg(feature = "ui-performance")]
                profile_static: std::env::var_os("CHORO_PROFILE_STATIC_SIDEBAR")
                    .is_some_and(|v| v == "1"),
                #[cfg(any(test, feature = "ui-performance"))]
                last_frame: Rc::new(std::cell::Cell::new(None)),
            }
        })
    }
}

impl Render for SidebarActivityLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let anchors = self.anchors.clone();
        let rotation = std::f32::consts::TAU * (self.started.elapsed().as_secs_f32() / 1.6 % 1.);
        #[cfg(target_os = "macos")]
        let reduced =
            objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion();
        #[cfg(not(target_os = "macos"))]
        let reduced = false;
        #[cfg(feature = "ui-performance")]
        let reduced = reduced || self.profile_static;
        #[cfg(any(test, feature = "ui-performance"))]
        let last_frame = self.last_frame.clone();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let anchors = anchors.visible_anchors();
                if anchors.is_empty() {
                    #[cfg(any(test, feature = "ui-performance"))]
                    last_frame.set(None);
                    return;
                }
                let _probe = crate::ui::performance::UiProbe::new("sidebar.animation");
                for anchor in anchors.iter() {
                    window.with_content_mask(Some(anchor.mask.clone()), |window| {
                        crate::ui::logo_spinner::paint_sidebar_spinner(
                            anchor.bounds,
                            if reduced { 0. } else { rotation },
                            anchor.color,
                            window,
                        );
                    });
                }
                #[cfg(any(test, feature = "ui-performance"))]
                {
                    let now = Instant::now();
                    if !reduced {
                        if let Some(previous) = last_frame.replace(Some(now)) {
                            crate::ui::performance::probes::record(
                                "sidebar.frame_interval",
                                now.duration_since(previous),
                            );
                        }
                    } else {
                        last_frame.set(None);
                    }
                }
                if !reduced {
                    window.request_animation_frame();
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}
