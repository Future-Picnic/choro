use super::*;

impl RootView {
    pub(super) fn resize_handle(
        &self,
        side: SidebarResizeSide,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = match side {
            SidebarResizeSide::Left => "left-sidebar-resize",
            SidebarResizeSide::Right => "right-sidebar-resize",
        };
        // Keep the interactive hitbox on the handle itself. An absolutely
        // positioned child does not enlarge its parent's GPUI hitbox, so making
        // the parent only 1px wide leaves the sidebar effectively impossible to
        // drag. The extra width is painted as part of the adjacent sidebar and
        // the center-facing edge remains a single hairline seam.
        let line_color = style::hairline(cx);

        div()
            .id(id)
            .flex_none()
            .w(px(5.))
            .h_full()
            .bg(crate::ui::design::nav(cx))
            .cursor_ew_resize()
            .child(
                div()
                    .when(side == SidebarResizeSide::Left, |line| line.ml_auto())
                    .w(px(1.))
                    .h_full()
                    .bg(line_color),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    let panels = &this.workspace.read(cx).panels;
                    this.sidebar_resize = Some(SidebarResizeState {
                        side,
                        start_x: event.position.x.as_f32(),
                        start_width: match side {
                            SidebarResizeSide::Left => panels.left,
                            SidebarResizeSide::Right => {
                                panels.right.max(crate::ui::design::RIGHT_SIDEBAR_MIN_W)
                            }
                        },
                    });
                }),
            )
            .on_drag(SidebarResizeHandle(side), |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
            .on_drag_move(cx.listener(
                move |this, event: &DragMoveEvent<SidebarResizeHandle>, _, cx| {
                    let side = event.drag(cx).0;
                    let Some(resize) = &this.sidebar_resize else {
                        return;
                    };
                    if resize.side != side {
                        return;
                    }

                    let delta = event.event.position.x.as_f32() - resize.start_x;
                    let width = match side {
                        SidebarResizeSide::Left => {
                            (resize.start_width + delta).clamp(LEFT_PANEL_MIN, LEFT_PANEL_MAX)
                        }
                        SidebarResizeSide::Right => (resize.start_width - delta)
                            .clamp(crate::ui::design::RIGHT_SIDEBAR_MIN_W, RIGHT_PANEL_MAX),
                    };

                    this.workspace.update(cx, |workspace, cx| match side {
                        SidebarResizeSide::Left => workspace.set_left_panel_size(width, cx),
                        SidebarResizeSide::Right => workspace.set_right_panel_size(width, cx),
                    });
                    cx.notify();
                },
            ))
    }

    /// Open or close the dedicated full-screen Settings route.
    pub(super) fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_view.is_some() {
            self.close_settings(window, cx);
        } else {
            let view = SettingsView::new(
                self.workspace.clone(),
                self.penpot.clone(),
                self.voice.clone(),
                self.orbit.clone(),
                self.remote_auth.clone(),
                self.remote_relay_identity.clone(),
                self.remote_relay_control.clone(),
                window,
                cx,
            );
            self.open_settings_view(view, window, cx);
        }
    }

    /// Open Settings directly on the Remote access section from the status chip
    /// beside the Run control.
    pub(super) fn open_remote_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_section(SettingsSection::Remote, window, cx);
    }

    pub(super) fn open_orbit_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_section(SettingsSection::Orbit, window, cx);
    }

    fn open_settings_section(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = SettingsView::new_in_section(
            self.workspace.clone(),
            self.penpot.clone(),
            self.voice.clone(),
            self.orbit.clone(),
            self.remote_auth.clone(),
            self.remote_relay_identity.clone(),
            self.remote_relay_control.clone(),
            section,
            window,
            cx,
        );
        self.open_settings_view(view, window, cx);
    }

    fn open_settings_view(
        &mut self,
        view: Entity<SettingsView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.center.update(cx, |center, cx| {
            center.set_web_preview_suspended(true, window, cx)
        });
        self.settings_view = Some(view);
        cx.notify();
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_view = None;
        self.center.update(cx, |center, cx| {
            center.set_web_preview_suspended(false, window, cx)
        });
        cx.notify();
    }

    /// The Settings screen as a full-screen route over the app. Its title bar
    /// mirrors the main workspace header: the same canonical 40px band, type tier,
    /// traffic-light clearance, and understated divider.
    pub(super) fn render_settings_screen(
        &self,
        view: Entity<SettingsView>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(crate::ui::design::base(cx))
            .child(
                v_flex()
                    .size_full()
                    .child(
                        h_flex()
                            .id("settings-header")
                            .w_full()
                            .h(crate::ui::design::header_h())
                            .flex_none()
                            .items_center()
                            .px_2()
                            .pl(px(76.))
                            .gap_2()
                            .window_control_area(gpui::WindowControlArea::Drag)
                            .border_b_1()
                            .border_color(crate::ui::design::line(cx))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_head())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("Settings"),
                            )
                            .child(div().flex_1())
                            .child(
                                style::header_icon_button(
                                    "settings-screen-close",
                                    IconName::Close,
                                    cx,
                                )
                                .tooltip("Close settings")
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.close_settings(window, cx);
                                    },
                                )),
                            ),
                    )
                    .child(div().flex_1().min_h(px(0.)).child(view)),
            )
            .into_any_element()
    }
}
