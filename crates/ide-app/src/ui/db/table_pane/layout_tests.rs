use super::*;
use ide_core::{TableColumn, TableRow};

fn fixture(window: &mut Window, cx: &mut Context<TablePane>, draft: &str) -> TablePane {
    let columns = (0..16)
        .map(|index| TableColumn {
            name: format!("column_{index}"),
            data_type: "TEXT".into(),
            nullable: true,
            editable: false,
            primary_key: false,
        })
        .collect::<Vec<_>>();
    let rows = (0..50)
        .map(|index| TableRow {
            json: serde_json::json!({"column_0": format!("Row {index} with a long value")})
                .to_string(),
        })
        .collect::<Vec<_>>();
    let filter_input = cx.new(|cx| {
        let mut input = InputState::new(window, cx);
        input.set_value(draft.to_owned(), window, cx);
        input
    });
    TablePane {
        // Missing-file SQLite reads fail without creating a file or contacting
        // a service. The fixture starts with a previously displayed page.
        handle: SqlHandle::new(
            DbProvider::SQLite,
            "/__choro_missing_layout_fixture__/db.sqlite",
        )
        .unwrap(),
        provider: DbProvider::SQLite,
        namespace: "main".into(),
        table: "people".into(),
        object_kind: DbObjectKind::Table,
        prod: false,
        filter_input,
        grid: GridModel::build(&columns, &rows),
        page: 1,
        has_more: true,
        table_editable: false,
        loading: false,
        loaded_once: true,
        error: None,
        filter_error: None,
        filters: vec![TableFilter {
            column: "column_0".into(),
            value: "committed".into(),
        }],
        pending_request: None,
        selected_row: None,
        inspector_open: true,
        fetch_ms: Some(12),
        load_seq: 0,
        h_scroll: ScrollHandle::new(),
        v_scroll: ScrollHandle::new(),
    }
}

#[gpui::test]
fn grid_header_body_and_footer_remain_aligned_during_resize_and_scroll(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(gpui_component::init);
    let mut pane = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| fixture(window, cx, "column_0=draft"));
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    pane.update(cx, |pane, cx| {
        pane.prod = true;
        pane.namespace = "an_unusually_long_schema_name_for_reporting_and_analytics".into();
        pane.table = "an_unusually_long_table_name_for_customer_event_analytics".into();
        cx.notify();
    });
    for (width, height) in [(1100., 600.), (700., 350.), (560., 350.), (1100., 600.)] {
        pane.update(cx, |pane, cx| {
            pane.h_scroll.set_offset(gpui::point(px(0.), px(0.)));
            pane.v_scroll.set_offset(gpui::point(px(0.), px(0.)));
            cx.notify();
        });
        cx.simulate_resize(gpui::size(px(width), px(height)));
        cx.run_until_parked();
        let root = cx.debug_bounds("db-grid-pane").unwrap();
        let toolbar = cx.debug_bounds("db-grid-toolbar").unwrap();
        let header = cx.debug_bounds("db-grid-header").unwrap();
        let body = cx.debug_bounds("db-grid-body").unwrap();
        let footer = cx.debug_bounds("db-grid-footer").unwrap();
        let controls = cx.debug_bounds("db-grid-filter-controls").unwrap();
        let pagination = cx.debug_bounds("db-grid-pagination").unwrap();
        let first_row = cx.debug_bounds("db-grid-first-row").unwrap();
        assert_eq!(header.size.height, px(HEADER_H));
        assert_eq!(header.left(), body.left());
        assert_eq!(header.size.width, body.size.width);
        assert!(toolbar.bottom() <= header.top());
        assert!(header.bottom() <= body.top());
        assert!(body.bottom() <= footer.top());
        assert!(
            controls.right() <= toolbar.right() && controls.bottom() <= toolbar.bottom(),
            "filter controls clipped at {width}x{height}"
        );
        assert!(
            pagination.right() <= footer.right() && pagination.bottom() <= footer.bottom(),
            "paging controls clipped at {width}x{height}"
        );
        assert!(
            footer.bottom() <= root.bottom(),
            "footer clipped at {width}x{height}"
        );
        pane.update(cx, |pane, cx| {
            pane.h_scroll.set_offset(gpui::point(px(-100.), px(0.)));
            pane.v_scroll.set_offset(gpui::point(px(0.), px(-100.)));
            cx.notify();
        });
        cx.run_until_parked();
        let scrolled_header = cx.debug_bounds("db-grid-header").unwrap();
        let scrolled_body = cx.debug_bounds("db-grid-body").unwrap();
        assert_eq!(scrolled_header.left(), scrolled_body.left());
        assert_eq!(
            scrolled_header.top(),
            header.top(),
            "header must stay above vertical scrolling"
        );
        assert!(
            scrolled_header.left() < root.left(),
            "horizontal scroll did not move the columns"
        );
        assert!(
            cx.debug_bounds("db-grid-first-row")
                .is_none_or(|row| row.top() < first_row.top()),
            "vertical scroll did not move the rows"
        );
    }
}

#[gpui::test]
fn row_inspector_shares_the_pane_without_hiding_grid_or_status(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let mut pane = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| fixture(window, cx, ""));
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    pane.update(cx, |pane, cx| {
        pane.selected_row = Some(3);
        cx.notify();
    });
    for (width, height) in [(1100., 600.), (700., 420.), (560., 360.)] {
        cx.simulate_resize(gpui::size(px(width), px(height)));
        cx.run_until_parked();
        let root = cx.debug_bounds("db-grid-pane").unwrap();
        let toolbar = cx.debug_bounds("db-grid-toolbar").unwrap();
        let header = cx.debug_bounds("db-grid-header").unwrap();
        let inspector = cx
            .debug_bounds("db-row-inspector")
            .expect("selected row opens the inspector");
        let footer = cx.debug_bounds("db-grid-footer").unwrap();
        assert!(
            inspector.right() <= root.right(),
            "inspector clipped at {width}"
        );
        assert!(inspector.top() >= toolbar.bottom());
        assert!(inspector.bottom() <= footer.top());
        assert!(inspector.size.width >= px(240.) && inspector.size.width <= px(380.));
        assert!(
            inspector.left() - root.left() >= px(280.),
            "grid squeezed below a usable width at {width}"
        );
        assert!(header.left() < inspector.left());
    }
    // Debug bounds are never cleared between frames in this GPUI version,
    // so closing is verified by the grid reclaiming the inspector's width.
    let with_inspector = cx.debug_bounds("db-table-grid").unwrap();
    pane.update(cx, |pane, cx| {
        pane.inspector_open = false;
        cx.notify();
    });
    cx.run_until_parked();
    let without_inspector = cx.debug_bounds("db-table-grid").unwrap();
    assert!(without_inspector.size.width > with_inspector.size.width + px(200.));
}

#[gpui::test]
fn first_load_failure_keeps_the_query_bar_and_offers_retry(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let mut pane = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut pane = fixture(window, cx, "");
            pane.loaded_once = false;
            pane.grid = GridModel::default();
            pane.error = Some("no such table: people".into());
            pane
        });
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    cx.simulate_resize(gpui::size(px(560.), px(360.)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("db-grid-toolbar").is_some());
    assert!(cx.debug_bounds("db-grid-header").is_none());
    assert!(
        cx.debug_bounds("db-grid-footer").is_none(),
        "status bar waits for a first page"
    );
    assert!(pane.is_some());
}

#[gpui::test]
fn invalid_filter_keeps_the_displayed_page_and_committed_query(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let mut pane = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| fixture(window, cx, "unknown=value"));
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    pane.unwrap().update(cx, |pane, cx| {
        pane.apply_filter(cx);
        assert_eq!(pane.page, 1);
        assert_eq!(pane.filters[0].value, "committed");
        assert!(pane.filter_error.is_some());
        assert!(!pane.loading);
        assert!(pane.pending_request.is_none());
    });
}

#[gpui::test]
fn failed_pagination_ignores_draft_filters_and_preserves_the_grid(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let mut pane = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| fixture(window, cx, "column_0=unapplied"));
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    pane.update(cx, |pane, cx| {
        pane.load_page(2, cx);
        let request = pane.pending_request.as_ref().unwrap();
        assert_eq!(request.page, 2);
        assert_eq!(request.filters[0].value, "committed");
        assert_eq!(pane.page, 1);
    });
    cx.run_until_parked();
    pane.update(cx, |pane, _| {
        assert!(!pane.loading);
        assert!(pane.error.is_some());
        assert_eq!(pane.page, 1);
        assert_eq!(pane.filters[0].value, "committed");
        assert_eq!(pane.grid.rows.len(), 50);
    });
    assert!(
        cx.debug_bounds("db-grid-header").is_some(),
        "failed reload hid the grid"
    );
    assert!(cx.debug_bounds("db-grid-footer").is_some());
}
