use super::*;

impl OnboardingTour {
    /// The step ribbon. Three tiers rather than two: done steps hold the accent
    /// quietly, the step you are on carries it at full strength, and the rest
    /// stay neutral — so the ribbon says *where you are*, not only how far.
    fn render_progress(&self, cx: &App) -> impl IntoElement {
        let current = self.phase.progress();
        h_flex().w_full().gap_1().children((1..=STEPS).map(|index| {
            div()
                .h(px(2.))
                .flex_1()
                .rounded_full()
                .bg(match index.cmp(&current) {
                    std::cmp::Ordering::Less => crate::ui::design::accent(cx).opacity(0.4),
                    std::cmp::Ordering::Equal => crate::ui::design::accent(cx),
                    std::cmp::Ordering::Greater => crate::ui::design::line_2(cx),
                })
        }))
    }

    fn render_exit(&self, cx: &mut Context<Self>) -> impl IntoElement {
        style::ghost_button_compact("onboarding-exit", "Exit tour")
            .text_color(crate::ui::design::t4(cx))
            .on_click(cx.listener(|this, _, _, cx| this.handle_event(OnboardingEvent::Exit, cx)))
    }

    /// Exit, as an actual control. In the cards it can be a ghost — it sits in a
    /// tray beside other buttons, so its shape is obvious. Loose in the sidebar
    /// with no card around it, bare text just reads as another label; the way
    /// out of the tour has to look like the way out. `chip_dropdown_variant` is
    /// the app's neutral filled control for exactly this plane: a `control_raised`
    /// fill that lifts off the panel, no resting stroke.
    fn render_exit_control(&self, cx: &mut Context<Self>) -> impl IntoElement {
        style::secondary_button_compact("onboarding-exit-sidebar", "Exit tour")
            .custom(style::chip_dropdown_variant(cx))
            .on_click(cx.listener(|this, _, _, cx| this.handle_event(OnboardingEvent::Exit, cx)))
    }

    fn render_back(
        &self,
        id: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        style::ghost_button_compact(id, label)
            .icon(IconName::ArrowLeft)
            .on_click(cx.listener(|this, _, _, cx| this.handle_event(OnboardingEvent::Back, cx)))
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        scrim(SHADE_MODAL, cx)
            .child(
                tour_card(cx)
                    .w(px(WELCOME_W))
                    .child(
                        v_flex()
                            .gap_5()
                            .p_8()
                            // A welcome screen should greet you. The old line
                            // ("the AI product builder workspace") was a
                            // billboard tagline; the title below already says
                            // what Choro is, so this gets to say hello.
                            .child(eyebrow("WELCOME TO CHORO", cx))
                            // The brand draws itself, then the whole project
                            // streams into it. This is a welcome screen before
                            // it's a tour.
                            .child(welcome_hub(cx))
                            .child(hero_title(
                                "Build your products. Keep the whole story.",
                                cx,
                            ))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .line_height(gpui::relative(1.55))
                                    .text_color(crate::ui::design::t2(cx))
                                    .child("Agents write the code. Choro keeps everything around it — your projects, specs, tasks, Git, and the running app — in one calm place. You never lose the thread, and you always decide what ships."),
                            )
                            .child(
                                h_flex()
                                    .items_start()
                                    .gap_2()
                                    .text_size(crate::ui::design::text_ui())
                                    .line_height(gpui::relative(1.45))
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(
                                        Icon::new(IconName::FolderClosed)
                                            .size(crate::ui::design::icon_sm())
                                            .flex_none(),
                                    )
                                    // Wraps instead of clipping: this line ran
                                    // off the card's edge mid-word.
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .child("Your private Playground · completely separate from your Choro workspace"),
                                    ),
                            ),
                    )
                    .child(
                        tour_footer(cx)
                            .px_8()
                            .py_4()
                            .child(self.render_exit(cx))
                            .child(div().flex_1())
                            .child(
                                hero_cta("onboarding-start", "Build something", cx).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.handle_event(OnboardingEvent::Start, cx)
                                    }),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }

    // (stack tile helpers live at module scope, below this impl block)

    /// "What do you use?" — the tools they already juggle. Multi-select, and
    /// deliberately unexplained: it just asks, then the page they build pulls
    /// their picks into one place. Sits before the agent question, which stays
    /// for the later steps that need a concrete provider.
    fn render_stack(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let plane = crate::ui::design::focus(cx);
        let tile = |tool: StackTool| {
            let on = self.stack.contains(&tool);
            let tint = stack_tool_color(tool, cx);
            div()
                .id(("stack-tool", tool as usize))
                .relative()
                .flex()
                .items_center()
                .gap_3()
                .p_3()
                .rounded(crate::ui::design::r_md())
                .border_1()
                // Fill-first: a quiet raised fill when idle, an accent-tinted
                // fill + visible edge when picked. The check on the right is the
                // unmistakable "selected", so the whole tile doesn't have to shout.
                .bg(if on {
                    tint.opacity(0.11)
                } else {
                    crate::ui::design::control_on(plane, cx)
                })
                .border_color(if on {
                    tint.opacity(0.55)
                } else {
                    crate::ui::design::control_line(cx)
                })
                .cursor_pointer()
                .when(!on, |t| {
                    t.hover(|t| t.bg(crate::ui::design::control_on_hover(plane, cx)))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.handle_event(OnboardingEvent::StackToggled(tool), cx)
                }))
                .child(
                    div()
                        .flex_none()
                        .size(px(36.))
                        .rounded(crate::ui::design::r_sm())
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(tint.opacity(if on { 0.16 } else { 0.10 }))
                        .child(stack_tool_mark(tool, tint, px(18.))),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap_0p5()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::t1(cx))
                                .child(tool.label()),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_label())
                                .truncate()
                                .text_color(crate::ui::design::t3(cx))
                                .child(tool.examples()),
                        ),
                )
                .child(if on {
                    div()
                        .flex_none()
                        .size(px(18.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(crate::ui::design::accent(cx))
                        .child(
                            Icon::new(IconName::Check)
                                .size(px(11.))
                                .text_color(crate::ui::design::on_accent(cx)),
                        )
                        .into_any_element()
                } else {
                    // A held slot so idle and picked tiles keep the same width.
                    div().flex_none().size(px(18.)).into_any_element()
                })
        };

        scrim(SHADE_MODAL, cx)
            .child(
                tour_card(cx)
                    .w(px(520.))
                    .child(
                        v_flex()
                            .gap_4()
                            .p_6()
                            .child(eyebrow("YOUR STACK", cx))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_title())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("What do you use to build?"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .line_height(gpui::relative(1.55))
                                    .text_color(crate::ui::design::t2(cx))
                                    .child("Tap the ones you reach for. Pick as many as you like."),
                            )
                            .child(
                                div()
                                    .grid()
                                    .grid_cols(2)
                                    .gap_2()
                                    .children(StackTool::ALL.map(tile)),
                            ),
                    )
                    .child(
                        tour_footer(cx)
                            .px_6()
                            .py_3()
                            .child(self.render_exit(cx))
                            .child(div().flex_1())
                            .child(self.render_back("onboarding-stack-back", "Back", cx))
                            .child(
                                style::primary_button_compact(
                                    "onboarding-stack-continue",
                                    "Continue",
                                    cx,
                                )
                                .icon(IconName::ArrowRight)
                                .disabled(self.stack.is_empty())
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.handle_event(OnboardingEvent::StackContinue, cx)
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// One agent, as a brand-forward card: plate, name, status and the Default
    /// slot stacked and centred, with the tick in the corner. The card commits
    /// to being a card — an earlier pass laid the same parts out as a row inside
    /// this button, and since a gpui `Button` centres its children, the contents
    /// bunched in the middle of a full-width row with dead air either side.

    fn render_provider_choice(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        scrim(SHADE_MODAL, cx)
            .child(
                tour_card(cx)
                    .w(px(560.))
                    .child(
                        v_flex()
                            .gap_4()
                            .p_6()
                            .child(eyebrow("MAKE CHORO YOURS", cx))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_title())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("Which agents do you build with?"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .line_height(gpui::relative(1.55))
                                    .text_color(crate::ui::design::t2(cx))
                                    // Says what the Default badge below means, so
                                    // the badge never appears unexplained.
                                    .child("Choro wraps around the agents you already trust. Pick every one you use — the first becomes your default."),
                            )
                            .child(
                                div()
                                    .grid()
                                    .grid_cols(3)
                                    .w_full()
                                    .gap_2p5()
                                    .child(self.render_provider_card(
                                        "onboarding-provider-claude",
                                        OnboardingProviderChoice::Claude,
                                        ide_core::AgentKind::Claude,
                                        "Claude Code",
                                        cx,
                                    ))
                                    .child(self.render_provider_card(
                                        "onboarding-provider-codex",
                                        OnboardingProviderChoice::Codex,
                                        ide_core::AgentKind::Codex,
                                        "Codex",
                                        cx,
                                    ))
                                    .child(self.render_provider_card(
                                        "onboarding-provider-opencode",
                                        OnboardingProviderChoice::OpenCode,
                                        ide_core::AgentKind::OpenCode,
                                        "OpenCode",
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        tour_footer(cx)
                            .px_6()
                            .py_3()
                            .child(self.render_exit(cx))
                            .child(div().flex_1())
                            .child(self.render_back(
                                "onboarding-provider-back",
                                "Back",
                                cx,
                            ))
                            .child(
                                style::primary_button_compact(
                                    "onboarding-provider-continue",
                                    "Continue",
                                    cx,
                                )
                                .icon(IconName::ArrowRight)
                                .disabled(self.provider_choices.is_empty())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.handle_event(OnboardingEvent::ProviderContinue, cx)
                                })),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The tour's sidebar heading, in the sidebar's own voice — `project_list`'s
    /// `.sect` recipe verbatim, so it reads as a sibling of PROJECTS.
    fn render_section_header(&self, cx: &App) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap_1p5()
            .px_2p5()
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(crate::ui::design::icon_sm())
                    .text_color(crate::ui::design::t4(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_label())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t4(cx))
                    .child(format!(
                        "YOUR TOUR · {} OF {}",
                        self.phase.progress().clamp(1, STEPS),
                        STEPS
                    )),
            )
    }

    /// The chapters with their state — done steps tick and recede, the step
    /// you're on carries the accent and the live status, the rest wait quietly.
    fn render_step_list(&self, failed: bool, status: &'static str, cx: &App) -> impl IntoElement {
        let current = self.phase.progress();
        v_flex()
            .w_full()
            .gap_1p5()
            .px_2p5()
            .children(STEP_NAMES.iter().enumerate().map(|(i, name)| {
                let step = i + 1;
                let state = step.cmp(&current);
                let done = state == std::cmp::Ordering::Less;
                let here = state == std::cmp::Ordering::Equal;
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_none()
                                    .size(crate::ui::design::icon_sm())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(done, |slot| {
                                        slot.child(
                                            Icon::new(IconName::Check)
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(
                                                    crate::ui::design::accent(cx).opacity(0.55),
                                                ),
                                        )
                                    })
                                    .when(here && !failed, |slot| {
                                        slot.child(
                                            div()
                                                .size(px(6.))
                                                .rounded_full()
                                                .bg(crate::ui::design::accent(cx)),
                                        )
                                    })
                                    .when(here && failed, |slot| {
                                        slot.child(
                                            Icon::new(IconName::TriangleAlert)
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(crate::ui::design::rose(cx)),
                                        )
                                    })
                                    .when(!done && !here, |slot| {
                                        slot.child(
                                            div()
                                                .size(px(6.))
                                                .rounded_full()
                                                .border_1()
                                                .border_color(crate::ui::design::line_2(cx)),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(crate::ui::design::text_ui())
                                    .when(here, |label| {
                                        label.font_weight(gpui::FontWeight::SEMIBOLD)
                                    })
                                    .text_color(if here {
                                        crate::ui::design::t1(cx)
                                    } else if done {
                                        crate::ui::design::t3(cx)
                                    } else {
                                        crate::ui::design::t4(cx)
                                    })
                                    .child(*name),
                            ),
                    )
                    .when(here, |row| {
                        row.child(
                            div()
                                .pl(px(crate::ui::design::ICON_SM + 8.0))
                                .text_size(crate::ui::design::text_label())
                                .line_height(gpui::relative(1.4))
                                .text_color(if failed {
                                    crate::ui::design::rose(cx)
                                } else {
                                    crate::ui::design::t3(cx)
                                })
                                .child(if failed {
                                    "Needs you — review the error, then retry."
                                } else {
                                    status
                                }),
                        )
                    })
            }))
    }

    /// The final step. No scrim, no card — the finished page fills the screen,
    /// and the only chrome is a bar docked in the strip designs.rs reserved at
    /// the bottom of the preview (below the WKWebView, so gpui can paint it). The
    /// page carries its own toolkit rundown, so there is nothing a tile grid
    /// would add; the last thing anyone sees is what they just built.
    fn render_preview_finish(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(zone) = self
            .targets
            .get(&SpotlightTarget::PreviewActionZone)
            .copied()
        else {
            // The strip hasn't reported its rect yet — let the page show through
            // untouched until it does.
            return div().into_any_element();
        };
        // Everything but the live page and this bar goes dark. On its own the
        // finish step left the whole app lit — the Open / Edit / Use-in-Agent
        // header, the sidebar, the right panel, the rail — a dozen things to tap
        // instead of the one that matters. Shade it all; keep the preview and the
        // action bar. (The WKWebView paints above the shade anyway; this dims the
        // gpui chrome around it.)
        let preview = self
            .targets
            .get(&SpotlightTarget::AssetBody)
            .copied()
            .unwrap_or(zone);
        let shade = crate::ui::design::base(cx).opacity(SHADE_SPOTLIGHT);
        let margin = SPOTLIGHT_MARGIN;
        let ring_left = (f32::from(preview.left()) - margin).max(0.0);
        let ring_top = (f32::from(preview.top()) - margin).max(0.0);
        let ring_w = f32::from(preview.size.width) + margin * 2.0;
        let ring_h = f32::from(preview.size.height) + margin * 2.0;
        let bar = div()
            .absolute()
            .left(zone.left())
            .top(zone.top())
            .w(zone.size.width)
            .h(zone.size.height)
            .occlude()
            .flex()
            .items_center()
            .gap_4()
            .px_6()
            .border_t_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::focus(cx))
            .shadow(crate::ui::design::shadow())
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_0p5()
                    .child(eyebrow("YOU’RE READY", cx))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_title())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child("You built and shipped a real page. It’s live above."),
                    ),
            )
            .child(self.render_exit(cx))
            .child(
                hero_cta("onboarding-start-working", "Start working", cx).on_click(cx.listener(
                    |this, _, _, cx| this.handle_event(OnboardingEvent::StartWorking, cx),
                )),
            );

        div()
            .absolute()
            .inset_0()
            .children(shade_holes(&[preview, zone], window.bounds().size, shade))
            // A quiet accent frame so the eye lands on the page, not the shade.
            .child(
                div()
                    .absolute()
                    .left(px(ring_left))
                    .top(px(ring_top))
                    .w(px(ring_w))
                    .h(px(ring_h))
                    .rounded(crate::ui::design::r_md())
                    .border_1()
                    .border_color(crate::ui::design::accent(cx).opacity(0.35)),
            )
            .child(bar)
            .into_any_element()
    }

    /// The last thing the tour does: point, once, at Add Project. No card
    /// chrome, no exit button, no ribbon — the tour is over, this is just a
    /// hand on the shoulder toward the one thing that starts real work. Any
    /// click anywhere dismisses it; the click that lands on Add Project both
    /// dismisses and opens the real flow (wired at the sidebar).
    fn render_add_project(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(hole) = self.targets.get(&SpotlightTarget::AddProject).copied() else {
            // Row hasn't reported its rect yet — dismiss-on-any-click, no cutout.
            return div()
                .absolute()
                .inset_0()
                .id("onboarding-addproject-catch")
                .occlude()
                .bg(crate::ui::design::base(cx).opacity(SHADE_SPOTLIGHT))
                .on_click(
                    cx.listener(|this, _, _, cx| this.handle_event(OnboardingEvent::Exit, cx)),
                )
                .into_any_element();
        };
        let screen = window.bounds().size;
        let (sw, sh) = (f32::from(screen.width), f32::from(screen.height));
        let m = SPOTLIGHT_MARGIN;
        let l = (f32::from(hole.left()) - m).max(0.0);
        let t = (f32::from(hole.top()) - m).max(0.0);
        let r = (f32::from(hole.right()) + m).min(sw);
        let b = (f32::from(hole.bottom()) + m).min(sh);
        let shade = crate::ui::design::base(cx).opacity(SHADE_SPOTLIGHT);
        // Each shade panel is a click-catcher — clicking the dimmed app just
        // ends the tour, so nothing traps them here.
        let panel = |id: &'static str, x: f32, y: f32, w: f32, h: f32| {
            div()
                .absolute()
                .id(id)
                .left(px(x))
                .top(px(y))
                .w(px(w.max(0.0)))
                .h(px(h.max(0.0)))
                .occlude()
                .bg(shade)
                .on_click(
                    cx.listener(|this, _, _, cx| this.handle_event(OnboardingEvent::Exit, cx)),
                )
        };

        div()
            .absolute()
            .inset_0()
            .child(panel("op-top", 0.0, 0.0, sw, t))
            .child(panel("op-bottom", 0.0, b, sw, sh - b))
            .child(panel("op-left", 0.0, t, l, b - t))
            .child(panel("op-right", r, t, sw - r, b - t))
            // The row keeps a soft frame so it reads as the target, not a gap.
            .child(
                div()
                    .absolute()
                    .left(px(l))
                    .top(px(t))
                    .w(px(r - l))
                    .h(px(b - t))
                    .rounded(crate::ui::design::r_sm())
                    .border_1()
                    .border_color(crate::ui::design::accent(cx).opacity(0.55)),
            )
            .child(beacon((r, 0.5 * (t + b)), crate::ui::design::accent(cx)))
            // The pointer card, to the right of the row.
            .child(
                tour_card(cx)
                    .absolute()
                    .left(px(r + 18.0))
                    .top(px(t))
                    .w(px(280.))
                    .rounded(crate::ui::design::r_md())
                    .child(
                        v_flex()
                            .gap_2()
                            .p_4()
                            .child(eyebrow("ONE MORE THING", cx))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_title())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("Add your first project"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .line_height(gpui::relative(1.5))
                                    .text_color(crate::ui::design::t2(cx))
                                    .child("Point Choro at a repo on your machine, and everything you just saw is yours for real."),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// A label pinned over a region, centred in it by a lane the size of the
    /// region itself — gpui has no `translate(-50%, -50%)`, and a chip's width is
    /// its text's. `low` drops it toward the bottom, for the one region big
    /// enough that its centre is where the card goes.
    fn map_pin(
        &self,
        target: SpotlightTarget,
        label: &'static str,
        low: bool,
        cx: &App,
    ) -> Option<gpui::AnyElement> {
        let b = self.targets.get(&target).copied()?;
        Some(
            div()
                .absolute()
                .left(b.left())
                .top(b.top())
                .w(b.size.width)
                .h(b.size.height)
                .flex()
                .justify_center()
                .when(low, |lane| lane.items_end().pb_12())
                .when(!low, |lane| lane.items_center())
                .child(
                    // On a fully dimmed screen these are the only legible thing
                    // on it, so they read at body size and lift off the shade —
                    // at label size they were easy to skim straight past.
                    div()
                        .flex_none()
                        .px_3()
                        .py_1p5()
                        .rounded(crate::ui::design::r_sm())
                        .bg(crate::ui::design::focus(cx))
                        .border_1()
                        .border_color(crate::ui::design::accent(cx).opacity(0.55))
                        .shadow(crate::ui::design::shadow())
                        .text_size(crate::ui::design::text_ui())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(crate::ui::design::accent(cx))
                        .child(label),
                )
                .into_any_element(),
        )
    }

    /// The map. Three cards used to walk you past the sidebar, then the rail,
    /// then the middle — sequencing something that isn't a sequence. Left,
    /// middle, right is one shape you take in at a glance, and you can only see
    /// a layout by seeing all of it at once. So: dim everything, name each
    /// region, say it once.
    fn render_map(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let screen = window.bounds().size;
        let work = self.targets.get(&SpotlightTarget::WorkArea).copied();
        let card_left = work
            .map(|w| f32::from(w.left()) + (f32::from(w.size.width) - CARD_W) / 2.0)
            .unwrap_or((f32::from(screen.width) - CARD_W) / 2.0)
            .clamp(14.0, (f32::from(screen.width) - CARD_W - 14.0).max(14.0));
        let card_top = ((f32::from(screen.height) - MAP_CARD_H) / 2.0).max(14.0);
        scrim(SHADE_SPOTLIGHT, cx)
            .children(self.map_pin(
                SpotlightTarget::ProjectSidebar,
                "your projects + agents",
                false,
                cx,
            ))
            .children(self.map_pin(
                SpotlightTarget::ProjectTools,
                "the project’s tools",
                false,
                cx,
            ))
            .children(self.map_pin(SpotlightTarget::WorkArea, "the work happens here", true, cx))
            .child(self.render_instruction_card(px(card_left), px(card_top), CARD_W, cx))
            .into_any_element()
    }

    fn render_waiting(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (left, width, bottom) = self
            .targets
            .get(&SpotlightTarget::ProjectSidebar)
            .map(|sidebar| {
                let inset = 8.0;
                (
                    px(f32::from(sidebar.left()) + inset),
                    px((f32::from(sidebar.size.width) - inset * 2.0).max(210.0)),
                    px(
                        (f32::from(window.bounds().size.height) - f32::from(sidebar.bottom())
                            + inset)
                            .max(inset),
                    ),
                )
            })
            .unwrap_or((px(10.), px(280.), px(10.)));
        let failed = self.last_agent_status == Some(AgentChatStatus::Failed);
        // Not a card on the sidebar — a section *of* it. Nothing is being asked
        // of you while an agent works, so the tour stops presenting itself as a
        // notification and just sits in the project alongside Projects, wearing
        // the same section header the sidebar already uses.
        div()
            .absolute()
            .left(left)
            .bottom(bottom)
            .w(width)
            .occlude()
            .child(
                v_flex()
                    .gap_2()
                    // The one hairline in the tour. Everywhere else a plane step
                    // does this job, but here the tour butts straight into
                    // unrelated sidebar content with no surface of its own to
                    // separate them.
                    .child(div().h(px(1.)).mx_2().bg(crate::ui::design::line(cx)))
                    .child(self.render_section_header(cx))
                    // All six chapters: ticked where they're done, lit where you
                    // are, quiet where they're still to come. The live status
                    // hangs off the step it belongs to.
                    .child(self.render_step_list(failed, self.waiting_status(), cx))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .px_2p5()
                            .child(self.render_exit_control(cx)),
                    ),
            )
            .into_any_element()
    }

    fn render_instruction_card(
        &self,
        left: Pixels,
        top: Pixels,
        width: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (title, body, action) = self.instruction();
        let has_continue = self.phase.has_continue();
        let continue_label = match self.phase {
            Phase::Map => "Meet your first agent",
            Phase::FirstResult => "Next: Docs",
            Phase::DocsSeen => "Show me with a task",
            Phase::ShipResult => "Next: Run project",
            _ => "Next",
        };
        let continue_event = match self.phase {
            Phase::FirstResult => OnboardingEvent::FirstResultNext,
            Phase::DocsSeen => OnboardingEvent::DocsSeenNext,
            Phase::ShipResult => OnboardingEvent::ShipResultNext,
            _ => OnboardingEvent::OrientationNext,
        };
        tour_card(cx)
            .absolute()
            .left(left)
            .top(top)
            .w(px(width))
            .rounded(crate::ui::design::r_md())
            .child(
                v_flex()
                    .gap_3()
                    .p_4()
                    // Always present, empty through the orientation cards: it
                    // sets the tour's length up front, and the card no longer
                    // changes shape when the count starts.
                    .child(self.render_progress(cx))
                    // The plate leads the eyebrow and nothing else indents. The
                    // card has one left edge — ribbon, label, title, body all
                    // start on it. Stacking the title beside the plate gave the
                    // card two competing edges, which is what read as crooked.
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(step_plate(self.phase.glyph(), cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(crate::ui::design::text_label())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::accent(cx))
                                    .child(action.to_uppercase()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_title())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .line_height(gpui::relative(1.3))
                            .text_color(crate::ui::design::t1(cx))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .line_height(gpui::relative(1.5))
                            .text_color(crate::ui::design::t2(cx))
                            .child(body),
                    ),
            )
            .child(
                tour_footer(cx)
                    .px_4()
                    .py_2()
                    // Exit leads, quiet and left. It used to sit after the
                    // spacer — which put the most destructive control in the
                    // bottom-right primary slot on every step that has no Next,
                    // i.e. most of them. Someone in a test tapped it because it
                    // was the only button on the card. A way out should be
                    // findable, never the default.
                    .child(self.render_exit(cx))
                    .child(div().flex_1())
                    .when(self.phase.can_go_back(), |footer| {
                        footer.child(self.render_back(
                            "onboarding-instruction-previous",
                            "Previous",
                            cx,
                        ))
                    })
                    .when(has_continue, |footer| {
                        footer.child(
                            style::primary_button_compact(
                                "onboarding-guided-next",
                                continue_label,
                                cx,
                            )
                            .icon(IconName::ArrowRight)
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.handle_event(continue_event, cx),
                            )),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_spotlight(
        &self,
        reveals: &[Bounds<Pixels>],
        focus: Bounds<Pixels>,
        secondary_focus: Option<Bounds<Pixels>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let screen = window.bounds().size;
        let margin = SPOTLIGHT_MARGIN;
        let left = (f32::from(focus.left()) - margin).max(0.0);
        let top = (f32::from(focus.top()) - margin).max(0.0);
        let right = (f32::from(focus.right()) + margin).min(f32::from(screen.width));
        let bottom = (f32::from(focus.bottom()) + margin).min(f32::from(screen.height));
        let width = (right - left).max(1.0);
        let height = (bottom - top).max(1.0);
        let shade = crate::ui::design::base(cx).opacity(SHADE_SPOTLIGHT);
        let secondary_ring = secondary_focus.map(|secondary| {
            let secondary_left = (f32::from(secondary.left()) - margin).max(0.0);
            let secondary_top = (f32::from(secondary.top()) - margin).max(0.0);
            let secondary_right =
                (f32::from(secondary.right()) + margin).min(f32::from(screen.width));
            let secondary_bottom =
                (f32::from(secondary.bottom()) + margin).min(f32::from(screen.height));
            div()
                .absolute()
                .left(px(secondary_left))
                .top(px(secondary_top))
                .w(px((secondary_right - secondary_left).max(1.0)))
                .h(px((secondary_bottom - secondary_top).max(1.0)))
                .rounded(crate::ui::design::r_sm())
                .border_1()
                // Quieter than the primary ring — it is context for the action,
                // not the action itself.
                .border_color(crate::ui::design::accent(cx).opacity(0.45))
                .into_any_element()
        });

        let gap = 18.0;
        let screen_width = f32::from(screen.width);
        let screen_height = f32::from(screen.height);
        // A native Doc webview paints above GPUI overlays. While that webview is
        // open, keep the instruction card wholly inside the right sidebar so no
        // part of its copy or controls can disappear behind the document.
        let doc_sidebar_card = matches!(self.phase, Phase::DocsSeen | Phase::TasksNav)
            .then(|| {
                let tools = self.targets.get(&SpotlightTarget::ProjectTools)?;
                let rail_left = self
                    .targets
                    .get(&SpotlightTarget::TasksNav)
                    .map(|rail| f32::from(rail.left()))
                    .unwrap_or_else(|| f32::from(tools.right()));
                let card_left = f32::from(tools.left()) + 14.0;
                let card_right = rail_left - gap;
                (card_right > card_left + 220.0).then_some((card_left, card_right - card_left))
            })
            .flatten();
        let card_width = doc_sidebar_card
            .map(|(_, width)| width.min(CARD_W))
            .unwrap_or(CARD_W);
        let card_height = if doc_sidebar_card.is_some() {
            260.0
        } else {
            220.0
        };
        let prefer_left = self.phase == Phase::Ship;
        // The side is now part of the answer, not just the coordinates: the beak
        // has to sit on whichever edge of the card faces the target.
        let (card_left, card_top, beak_side) = if let Some((sidebar_left, _)) = doc_sidebar_card {
            (
                sidebar_left,
                top.min(screen_height - card_height - 14.0).max(14.0),
                if self.phase == Phase::DocsSeen {
                    Beak::Left
                } else {
                    Beak::Right
                },
            )
        } else if prefer_left && left >= card_width + gap + 14.0 {
            (
                left - card_width - gap,
                top.min(screen_height - card_height - 14.0),
                Beak::Right,
            )
        } else if right + gap + card_width <= screen_width - 14.0 {
            (
                right + gap,
                top.min(screen_height - card_height - 14.0),
                Beak::Left,
            )
        } else if left >= card_width + gap + 14.0 {
            (
                left - card_width - gap,
                top.min(screen_height - card_height - 14.0),
                Beak::Right,
            )
        } else if bottom + gap + card_height <= screen_height - 14.0 {
            (
                left.min(screen_width - card_width - 14.0).max(14.0),
                bottom + gap,
                Beak::Top,
            )
        } else {
            (
                left.min(screen_width - card_width - 14.0).max(14.0),
                (top - card_height - gap).max(14.0),
                Beak::Bottom,
            )
        };
        let beak_at = match beak_side {
            // Track the target's centre, but stay inside the card's own span.
            // The clamp is deliberately conservative: `card_height` above is an
            // estimate, and a beak that slid off a shorter card would read as a
            // stray triangle floating in the shade.
            Beak::Left | Beak::Right => {
                (0.5 * (top + bottom)).clamp(card_top + BEAK_INSET, card_top + BEAK_SAFE_SPAN)
            }
            Beak::Top | Beak::Bottom => (0.5 * (left + right))
                .clamp(card_left + BEAK_INSET, card_left + card_width - BEAK_INSET),
        };

        div()
            .absolute()
            .inset_0()
            .children(shade_holes(reveals, screen, shade))
            .children(secondary_ring)
            // A soft outer halo carries the ring's glow into the shade instead
            // of a hard drop shadow, which only muddied the revealed UI. It
            // breathes — the one moving thing on screen, sitting exactly where
            // the eye is supposed to go. Slow and shallow on purpose; a tour
            // that pulses hard reads as needy.
            .child({
                let glow = crate::ui::design::accent(cx);
                div()
                    .absolute()
                    .left(px(left - HALO_SPREAD))
                    .top(px(top - HALO_SPREAD))
                    .w(px(width + HALO_SPREAD * 2.0))
                    .h(px(height + HALO_SPREAD * 2.0))
                    .rounded(crate::ui::design::r_md())
                    .border_1()
                    .border_color(glow.opacity(HALO_MIN))
                    .with_animation(
                        "onboarding-halo",
                        Animation::new(HALO_CYCLE).repeat(),
                        move |halo, delta| {
                            // A sine keeps the loop seamless — it returns to
                            // exactly where it started, so the repeat has no
                            // visible seam the way a linear ramp would.
                            let wave = 0.5 + 0.5 * (delta * std::f32::consts::TAU).sin();
                            halo.border_color(glow.opacity(HALO_MIN + (HALO_MAX - HALO_MIN) * wave))
                        },
                    )
            })
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(width))
                    .h(px(height))
                    .rounded(crate::ui::design::r_sm())
                    .border_2()
                    .border_color(crate::ui::design::accent(cx)),
            )
            // On the target's edge that faces the card, never over its middle:
            // the eye travels card → target, so the beacon sits on that path.
            // A dot in the centre would cover the very word it's asking you to
            // press, and on a corner it lands wherever the eye isn't.
            .children((!self.phase.has_continue()).then(|| {
                let at = match beak_side {
                    Beak::Left => (right, top + height / 2.0),
                    Beak::Right => (left, top + height / 2.0),
                    Beak::Top => (left + width / 2.0, bottom),
                    Beak::Bottom => (left + width / 2.0, top),
                };
                beacon(at, crate::ui::design::accent(cx))
            }))
            .child(self.render_instruction_card(px(card_left), px(card_top), card_width, cx))
            // After the card, so its fill paints over the card's own border.
            .child(beak(
                beak_side, beak_at, card_left, card_top, card_width, cx,
            ))
            .into_any_element()
    }
}

impl Render for OnboardingTour {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.phase == Phase::Finished || !self.is_active_project(cx) {
            return div().into_any_element();
        }
        if self.phase == Phase::Welcome {
            return self.render_welcome(cx);
        }
        if self.phase == Phase::Stack {
            return self.render_stack(cx);
        }
        if self.phase == Phase::ProviderChoice {
            return self.render_provider_choice(cx);
        }
        if self.phase.is_waiting() {
            return self.render_waiting(window, cx);
        }
        // Before the `target()` lookup below: the map deliberately has no single
        // focus, so it must not fall through to the no-target fallback.
        if self.phase == Phase::Map {
            return self.render_map(window, cx);
        }
        if self.phase == Phase::AddProject {
            return self.render_add_project(window, cx);
        }
        // The live page shows through untouched; the only tour chrome is a bar
        // docked under it (designs.rs reserved the strip). No scrim, no card —
        // the page is the reward, not something to dim.
        if self.phase == Phase::PreviewLive {
            return self.render_preview_finish(window, cx);
        }
        let Some(focus) = self
            .phase
            .target()
            .and_then(|target| self.targets.get(&target).copied())
        else {
            return scrim(SHADE_SPOTLIGHT, cx).into_any_element();
        };
        let reveal_target = match self.phase {
            Phase::TaskSend => Some(SpotlightTarget::Composer),
            Phase::ShipGenerate | Phase::ShipCommit => Some(SpotlightTarget::ShipDialog),
            Phase::FirstSend | Phase::TaskImplement => Some(SpotlightTarget::WorkArea),
            _ => None,
        };
        let reveal = reveal_target
            .and_then(|target| self.targets.get(&target).copied())
            .unwrap_or(focus);
        // "Nothing gets lost" is the one claim the tour can *show* instead of
        // asserting: light every connected piece at once and let the shade make
        // the argument. The task and PR under the agent title, the ship result
        // in the timeline, the changed files in Git, the script still serving —
        // four corners of the window, one thread, all lit together.
        let mut reveals = vec![reveal];
        if self.phase == Phase::ShipResult {
            reveals = [
                self.targets.get(&SpotlightTarget::AgentContext).copied(),
                self.targets.get(&SpotlightTarget::ShipResult).copied(),
                self.targets.get(&SpotlightTarget::GitPanel).copied(),
                self.targets.get(&SpotlightTarget::RunScript).copied(),
            ]
            .into_iter()
            .flatten()
            .collect();
            if reveals.is_empty() {
                reveals.push(focus);
            }
        }
        let secondary_focus = match self.phase {
            Phase::PreviewLive => self.targets.get(&SpotlightTarget::RunScript).copied(),
            Phase::ShipResult => self.targets.get(&SpotlightTarget::AgentContext).copied(),
            _ => None,
        };
        self.render_spotlight(&reveals, focus, secondary_focus, window, cx)
    }
}
