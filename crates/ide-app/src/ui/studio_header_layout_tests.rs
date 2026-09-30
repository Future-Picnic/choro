//! Exercise GPUI intrinsic sizing, which a web-only screenshot cannot verify.
use super::*;
use gpui::{AppContext, Context, Render, Window};

struct StudioHeaders;

impl Render for StudioHeaders {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full()
            .child(design_workspace_header(cx).debug_selector(|| "project-header".into())
                .child(design_workspace_identity()
                    .child(header_icon_button("back", IconName::ArrowLeft, cx))
                    .child(design::header::title_col(cx).child(design::header::title(
                        "Common Ground — Coworking Operations with a very long project name", cx))))
                .child(design::header::actions().ml_auto().debug_selector(|| "project-actions".into())
                    .child(stage_bar_choices(cx).debug_selector(|| "project-track".into())
                        .child(stage_bar_choice("design", "Design", true, "Design", cx))
                        .child(stage_bar_choice("prototype", "Prototype", false, "Prototype", cx)))
                    .child(div().flex_none().debug_selector(|| "compare".into())
                        .child(context_panel_action_button("compare", IconName::Replace, "Close Compare", cx)))
                    .child(implement_button("implement", "Reimplement", cx))))
            .child(stage_header_bar(cx).debug_selector(|| "screen-header".into())
                .child(stage_header_context().debug_selector(|| "screen-context".into())
                    .child(context_panel_action_button("all", IconName::LayoutDashboard, "All screens", cx).flex_none())
                    .child(stage_bar_rule(cx))
                    .child(div().flex_1().min_w(px(0.)).truncate().child("A screen with a very long name · 1440 × 900")))
                .child(h_flex().flex_none().ml_auto().items_center().gap_1().debug_selector(|| "screen-actions".into())
                    .child(header_icon_button("undo", IconName::Undo2, cx))
                    .child(header_icon_button("redo", IconName::Redo2, cx))
                    .child(stage_header_status("Unsaved", design::amber(cx), cx))
                    .child(context_panel_action_button("inspector", IconName::PanelRight, "Inspector", cx))))
            .child(div().flex_1())
            .child(stage_bar(cx)
                .child(stage_bar_choices(cx).debug_selector(|| "view-track".into())
                    .child(stage_bar_choice("canvas", "Canvas", true, "Canvas", cx))
                    .child(stage_bar_choice("grid", "Grid", false, "Grid", cx))
                    .child(stage_bar_choice("focus", "Focus", false, "Focus", cx)))
                .child(stage_bar_readout("100%", cx).debug_selector(|| "zoom".into())))
    }
}

#[gpui::test]
fn studio_header_groups_keep_intrinsic_width_and_never_overlap(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| StudioHeaders);
        gpui_component::Root::new(view, window, cx)
    });
    for width in [1400., 1000., 760., 560., 480.] {
        cx.simulate_resize(gpui::size(px(width), px(600.)));
        cx.run_until_parked();
        let project = cx.debug_bounds("project-header").unwrap();
        let actions = cx.debug_bounds("project-actions").unwrap();
        let track = cx.debug_bounds("project-track").unwrap();
        let compare = cx.debug_bounds("compare").unwrap();
        assert!(track.size.width > px(140.), "choice group collapsed: {track:?}");
        assert!(track.right() <= compare.left(), "choices overlap Compare at {width}");
        assert!(actions.right() <= project.right() && actions.bottom() <= project.bottom(), "project actions clipped at {width}");
        let header = cx.debug_bounds("screen-header").unwrap();
        let context = cx.debug_bounds("screen-context").unwrap();
        let tools = cx.debug_bounds("screen-actions").unwrap();
        assert!(context.right() <= tools.left() || context.bottom() <= tools.top(), "screen context overlaps editing tools at {width}");
        assert!(tools.right() <= header.right() && tools.bottom() <= header.bottom(), "editing controls clipped at {width}");
        if width >= 760. { assert_eq!(header.size.height, px(44.), "ordinary screen header must stay one row"); }
        let views = cx.debug_bounds("view-track").unwrap();
        let zoom = cx.debug_bounds("zoom").unwrap();
        assert!(views.size.width > px(170.) && views.right() <= zoom.left(), "view track overlaps zoom at {width}");
    }
}

struct CachedStudioStage;
impl Render for CachedStudioStage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        studio_region_root().child(
            v_flex().flex_1().min_w(px(0.)).h_full()
                .child(div().w_full().h(px(44.)).debug_selector(|| "cached-header".into())
                    .child(div().w(px(480.)).h(px(28.))))
                .child(div().flex_1().w_full().debug_selector(|| "cached-canvas".into()))
                .child(div().w_full().h(px(44.)).debug_selector(|| "cached-footer".into()))
        )
    }
}
struct StudioCachedLayout {
    stage: gpui::Entity<CachedStudioStage>,
    sidebar_width: f32,
}
impl Render for StudioCachedLayout {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h_flex().size_full().debug_selector(|| "studio-workspace".into())
            .child(div().w(px(self.sidebar_width)).h_full().flex_none())
            .child(gpui::AnyView::from(self.stage.clone()).cached(
                gpui::StyleRefinement::default().flex_1().h_full().min_w(px(0.))))
    }
}
#[gpui::test]
fn studio_cached_stage_fills_remaining_width_on_resize_and_sidebar_toggle(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let mut layout = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let stage = cx.new(|_| CachedStudioStage);
        let view = cx.new(|_| StudioCachedLayout { stage, sidebar_width: 318. });
        layout = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let layout = layout.unwrap();
    for (width, sidebar) in [(1500.,318.), (1000.,318.), (1000.,38.), (1700.,38.), (1700.,318.)] {
        layout.update(cx, |view, cx| { view.sidebar_width = sidebar; cx.notify(); });
        cx.simulate_resize(gpui::size(px(width), px(800.)));
        cx.run_until_parked();
        let workspace = cx.debug_bounds("studio-workspace").unwrap();
        for selector in ["cached-header", "cached-canvas", "cached-footer"] {
            let bounds = cx.debug_bounds(selector).unwrap();
            assert_eq!(bounds.left(), workspace.left() + px(sidebar), "{selector}: left edge");
            assert_eq!(bounds.right(), workspace.right(), "{selector}: unused horizontal space at {width}, sidebar {sidebar}");
        }
    }
}
