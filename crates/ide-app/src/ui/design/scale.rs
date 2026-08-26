//! Non-color design tokens — radii, the type scale, control heights, spacing.
//!
//! These are our decisions (not part of the color theme), kept in one place so
//! no component hard-codes a raw `px(..)`. Reference: `choro-design-system.html`.

use gpui::{hsla, point, px, BoxShadow, Pixels};

// ---- elevation -----------------------------------------------------------

/// The one soft drop shadow (design `--shadow`: `0 12px 30px -16px rgba(0,0,0,.6)`)
/// carried by floating surfaces — the composer box, dropdown menus, dialogs.
pub fn shadow() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: hsla(0., 0., 0., 0.6),
        offset: point(px(0.), px(12.)),
        blur_radius: px(30.),
        spread_radius: px(-16.),
    }]
}

/// The focused dropdown elevation from the canonical menu mock.
pub fn menu_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: hsla(0., 0., 0., 0.65),
        offset: point(px(0.), px(16.)),
        blur_radius: px(40.),
        spread_radius: px(-12.),
    }]
}

// ---- corner radii --------------------------------------------------------

/// 5px — tiny inline elements (status chips, menu rows, letter badges).
pub const R_XS: f32 = 5.0;
/// 7px — the standard control radius (buttons, inputs, chips, tabs).
pub const R_SM: f32 = 7.0;
/// 10px — panels, menus, small cards, drawers.
pub const R_MD: f32 = 10.0;
/// 13px — elevated content cards (chat cards, plan, composer, modals).
pub const R_LG: f32 = 13.0;
/// Full pill (source pills, live dots, toggles).
pub const R_PILL: f32 = 999.0;

pub fn r_xs() -> Pixels {
    px(R_XS)
}
pub fn r_sm() -> Pixels {
    px(R_SM)
}
pub fn r_md() -> Pixels {
    px(R_MD)
}
pub fn r_lg() -> Pixels {
    px(R_LG)
}
pub fn r_pill() -> Pixels {
    px(R_PILL)
}

// ---- type scale ----------------------------------------------------------

/// 11px — section labels (uppercase), timestamps, faint meta.
pub const TEXT_LABEL: f32 = 11.0;
/// 12px — compact monospaced file/path rows in dense developer-tool panels.
pub const TEXT_FILE: f32 = 12.0;
/// 12.5px — secondary UI: buttons, indicators, chips, step rows, list rows.
pub const TEXT_UI: f32 = 12.5;
/// 13.5px — body: chat prose, doc/list content, inputs.
pub const TEXT_BODY: f32 = 13.5;
/// 15.5px — titles: one per view (agent task, plan, panel head is 13px).
pub const TEXT_TITLE: f32 = 15.5;
/// 13px — panel title / header title (the middle-header + right-panel head).
pub const TEXT_HEAD: f32 = 13.0;
/// 28px — display: the one hero line (new-agent "what should we build?").
pub const TEXT_DISPLAY: f32 = 28.0;
/// Chat reading rhythm — slightly more open than compact application chrome.
pub const CHAT_PROSE_LINE_HEIGHT: f32 = 1.52;
pub const CHAT_LIST_LINE_HEIGHT: f32 = 1.5;
pub const CHAT_PARAGRAPH_GAP_REMS: f32 = 1.3;
/// Native code-editor chrome: compact enough for an agent workspace while
/// leaving the source canvas comfortably separated from tabs and surrounding
/// panels.
pub const EDITOR_BREADCRUMB_H: f32 = 32.0;
pub const EDITOR_STATUS_H: f32 = 26.0;
pub const EDITOR_PAD_X: f32 = 14.0;
pub const EDITOR_PAD_Y: f32 = 10.0;
pub const EDITOR_TAB_BAR_H: f32 = 34.0;
pub const EDITOR_TAB_MAX_W: f32 = 180.0;
pub const EDITOR_TAB_PAD_X: f32 = 11.0;
pub const EDITOR_TAB_GAP: f32 = 6.0;
/// Shared readable width for primary center-workspace content. Agent chat,
/// docs, tasks, services, and sibling center views all converge on this cap.
pub const CENTER_CONTENT_MAX_W: f32 = 1100.0;
/// Compatibility alias for agent-chat call sites.
pub const AGENT_CHAT_CONTENT_MAX_W: f32 = CENTER_CONTENT_MAX_W;
pub const AGENT_CHAT_GUTTER_X: f32 = 16.0;

pub const RAIL_FOOTER_CELL_H: f32 = 52.0;

pub fn text_label() -> Pixels {
    px(TEXT_LABEL)
}
pub fn text_file() -> Pixels {
    px(TEXT_FILE)
}
pub fn text_ui() -> Pixels {
    px(TEXT_UI)
}
pub fn text_body() -> Pixels {
    px(TEXT_BODY)
}
pub fn text_title() -> Pixels {
    px(TEXT_TITLE)
}
pub fn text_head() -> Pixels {
    px(TEXT_HEAD)
}
pub fn text_display() -> Pixels {
    px(TEXT_DISPLAY)
}

pub fn editor_breadcrumb_h() -> Pixels {
    px(EDITOR_BREADCRUMB_H)
}

pub fn editor_status_h() -> Pixels {
    px(EDITOR_STATUS_H)
}

pub fn editor_pad_x() -> Pixels {
    px(EDITOR_PAD_X)
}

pub fn editor_pad_y() -> Pixels {
    px(EDITOR_PAD_Y)
}

pub fn editor_tab_bar_h() -> Pixels {
    px(EDITOR_TAB_BAR_H)
}

pub fn editor_tab_max_w() -> Pixels {
    px(EDITOR_TAB_MAX_W)
}

pub fn editor_tab_pad_x() -> Pixels {
    px(EDITOR_TAB_PAD_X)
}

pub fn editor_tab_gap() -> Pixels {
    px(EDITOR_TAB_GAP)
}

pub fn agent_chat_content_max_w() -> Pixels {
    px(AGENT_CHAT_CONTENT_MAX_W)
}

pub fn center_content_max_w() -> Pixels {
    px(CENTER_CONTENT_MAX_W)
}

pub fn agent_chat_gutter_x() -> Pixels {
    px(AGENT_CHAT_GUTTER_X)
}

pub fn agent_chat_frame_max_w() -> Pixels {
    center_content_frame_max_w()
}

pub fn center_content_frame_max_w() -> Pixels {
    px(CENTER_CONTENT_MAX_W + AGENT_CHAT_GUTTER_X * 2.0)
}

pub fn rail_footer_cell_h() -> Pixels {
    px(RAIL_FOOTER_CELL_H)
}

// ---- control heights -----------------------------------------------------

/// 28px — standard button / primary control height.
pub const CONTROL_H: f32 = 28.0;
/// 26px — compact control (chips, ghost toolbar buttons).
pub const CONTROL_H_SM: f32 = 26.0;
/// 24px — tiny control (split-buttons in list headers, stage-all).
pub const CONTROL_H_XS: f32 = 24.0;
/// 44px — compact dropdown containing one 14px icon, caret, gap, and padding.
pub const CONTROL_ICON_DROPDOWN_W: f32 = 44.0;
/// 27px — agent-header Ship action from the canonical element-header mock.
pub const HEADER_SHIP_H: f32 = 27.0;
/// 11px / 7px — Ship label inset and icon-to-label gap.
pub const HEADER_SHIP_PAD_X: f32 = 11.0;
pub const HEADER_SHIP_GAP: f32 = 7.0;
/// 40px — the header band height (middle header row, panel title row, drawer strip).
pub const HEADER_H: f32 = 40.0;
/// Shared inset from the center pane edge for titlebar controls and floating
/// header actions, keeping their right edges on one vertical guide.
pub const HEADER_EDGE_INSET_X: f32 = 8.0;
/// 2px — breathing room between a header divider and its anchored popup.
pub const HEADER_OVERLAY_GAP: f32 = 2.0;
/// 34px — drawer / dialog header row.
pub const SUBHEAD_H: f32 = 34.0;

pub fn control_h() -> Pixels {
    px(CONTROL_H)
}
pub fn control_h_sm() -> Pixels {
    px(CONTROL_H_SM)
}
pub fn control_h_xs() -> Pixels {
    px(CONTROL_H_XS)
}
pub fn control_icon_dropdown_w() -> Pixels {
    px(CONTROL_ICON_DROPDOWN_W)
}
pub fn header_ship_h() -> Pixels {
    px(HEADER_SHIP_H)
}
pub fn header_ship_pad_x() -> Pixels {
    px(HEADER_SHIP_PAD_X)
}
pub fn header_ship_gap() -> Pixels {
    px(HEADER_SHIP_GAP)
}
pub fn header_h() -> Pixels {
    px(HEADER_H)
}
pub fn header_edge_inset_x() -> Pixels {
    px(HEADER_EDGE_INSET_X)
}
pub fn header_overlay_top() -> Pixels {
    px(HEADER_H + HEADER_OVERLAY_GAP)
}
pub fn subhead_h() -> Pixels {
    px(SUBHEAD_H)
}

// ---- responsive layout --------------------------------------------------

/// 260px — minimum contextual right-sidebar width. The single Git-view
/// dropdown leaves enough room for Board and the dense Git action/commit rows.
pub const RIGHT_SIDEBAR_MIN_W: f32 = 260.0;
/// 12px — separation between a contextual-panel identity and its tab cluster.
pub const PANEL_IDENTITY_TABS_GAP: f32 = 12.0;
/// 2px — optical baseline correction for compact tabs beside a larger title.
pub const PANEL_TAB_OFFSET_Y: f32 = 2.0;
/// 8px — minimum breathing room between contextual tabs and the view flip.
pub const PANEL_ACTION_GAP: f32 = 8.0;
/// 10px — stop glyph centered inside the composer's 28px send/stop square.
pub const COMPOSER_STOP_GLYPH: f32 = 10.0;
/// 50px — input line plus the mock's breathing room before the control row.
pub const COMPOSER_INPUT_MIN_H: f32 = 50.0;
/// 128px — canonical empty composer frame shared by agent chat and Git commit.
/// This accommodates the input region, control row, and their vertical insets
/// without either surface growing beyond the shared baseline.
pub const COMPOSER_FRAME_H: f32 = 128.0;
/// 188px — canonical minimum width for every popup menu.
pub const MENU_MIN_W: f32 = 188.0;
/// 500px — maximum popup width unless a caller supplies a tighter constraint.
pub const MENU_MAX_W: f32 = 500.0;
/// 450px — maximum popup height before scrolling, also capped at half a window.
pub const MENU_MAX_H: f32 = 450.0;
/// 4px — popup inner frame padding.
pub const MENU_OUTER_PAD: f32 = 4.0;
/// 9px — horizontal row padding and icon-to-label rhythm.
pub const MENU_ITEM_PAD_X: f32 = 9.0;
pub const MENU_ITEM_GAP: f32 = 9.0;
/// 14px — fixed leading glyph slot.
pub const MENU_ICON_SLOT_W: f32 = 14.0;
/// 6px × 4px — separator inset and vertical breathing room.
pub const MENU_SEPARATOR_MARGIN_X: f32 = 6.0;
pub const MENU_SEPARATOR_MARGIN_Y: f32 = 4.0;
/// 6px top / 3px bottom — compact section-label padding.
pub const MENU_LABEL_PAD_TOP: f32 = 6.0;
pub const MENU_LABEL_PAD_BOTTOM: f32 = 3.0;
/// 1px — menu separator hairline thickness.
pub const MENU_SEPARATOR_H: f32 = 1.0;
/// Popup/submenu placement geometry.
pub const MENU_WINDOW_MARGIN: f32 = 4.0;
pub const MENU_SUBMENU_FLIP_OFFSET: f32 = 16.0;
pub const MENU_SUBMENU_OVERLAP: f32 = 8.0;
/// Canonical run split-button geometry from the app-header mock.
pub const SPLIT_PRIMARY_PAD_X: f32 = 10.0;
pub const SPLIT_CARET_W: f32 = 22.0;
pub const SPLIT_DIVIDER_W: f32 = 1.0;
/// Shared chat-card frame rhythm from the canonical `.card` specification.
pub const CHAT_CARD_HEAD_PAD_X: f32 = 14.0;
pub const CHAT_CARD_HEAD_PAD_Y: f32 = 10.0;
pub const CHAT_CARD_HEAD_GAP: f32 = 9.0;
/// 40px — fixed chat-card header height after normalizing button line boxes.
pub const CHAT_CARD_HEAD_H: f32 = 40.0;
pub const CHAT_CARD_BODY_PAD_X: f32 = 16.0;
pub const CHAT_CARD_BODY_PAD_Y: f32 = 14.0;
pub const CHAT_CARD_ROW_PAD_X: f32 = 14.0;
pub const CHAT_CARD_ROW_PAD_Y: f32 = 9.0;
pub const CHAT_CARD_ROW_GAP: f32 = 10.0;
/// Agent detail bar: full-width 40px strip with its controls at the far right.
pub const AGENT_DETAIL_BAR_PAD_X: f32 = 12.0;
pub const AGENT_DETAIL_TAB_GROUP_GAP: f32 = 2.0;
pub const AGENT_DETAIL_TAB_PAD_X: f32 = 10.0;
pub const AGENT_DETAIL_TAB_GAP: f32 = 6.0;
/// 16px — aligns the Git last-commit strip with the PR content directly above.
pub const GIT_META_ROW_PAD_X: f32 = 16.0;
/// Fixed row height keeps the Git status list eligible for GPUI's lazy
/// `uniform_list`, so large worktrees do not increase unrelated repaint cost.
pub const GIT_STATUS_ROW_H: f32 = 26.0;
/// Canonical contextual-panel `.ptitle`, `.flipbtn`, `.lane`, and `.brow`
/// geometry shared by Assets, Services/Env, Docs, DB, and sibling navigators.
pub const CONTEXT_PANEL_TITLE_PAD_X: f32 = 14.0;
pub const CONTEXT_PANEL_TITLE_PAD_TOP: f32 = 12.0;
pub const CONTEXT_PANEL_TITLE_PAD_BOTTOM: f32 = 4.0;
pub const CONTEXT_PANEL_TITLE_GAP: f32 = 8.0;
pub const CONTEXT_PANEL_FLIP_H: f32 = 22.0;
pub const CONTEXT_PANEL_FLIP_PAD_X: f32 = 8.0;
pub const CONTEXT_PANEL_FLIP_GAP: f32 = 6.0;
pub const CONTEXT_PANEL_LANE_PAD_X: f32 = 14.0;
pub const CONTEXT_PANEL_LANE_PAD_TOP: f32 = 10.0;
pub const CONTEXT_PANEL_LANE_PAD_BOTTOM: f32 = 4.0;
pub const CONTEXT_PANEL_ROW_PAD_X: f32 = 14.0;
pub const CONTEXT_PANEL_ROW_PAD_Y: f32 = 7.0;
pub const CONTEXT_PANEL_ROW_GAP: f32 = 9.0;
/// Canonical centered content column (`--col`) and its horizontal inset.
pub const CENTER_COLUMN_MAX_W: f32 = CENTER_CONTENT_MAX_W;
pub const CENTER_COLUMN_PAD_X: f32 = 22.0;
pub const CENTER_COLUMN_PAD_Y: f32 = 20.0;
/// Services/Env center-detail rhythm.
pub const SERVICES_GROUP_GAP: f32 = 12.0;
pub const SERVICES_FACT_GAP: f32 = 6.0;
pub const SERVICES_ENV_KEY_W: f32 = 240.0;
pub const SERVICES_HEADER_TITLE_GAP: f32 = 8.0;
pub const SERVICES_EMPTY_GAP: f32 = 12.0;
pub const SERVICES_SPINNER_SIZE: f32 = 46.0;
/// Asset detail preview and notes rhythm.
pub const ASSET_DETAIL_GAP: f32 = 12.0;
pub const ASSET_PREVIEW_PAD: f32 = 24.0;
pub const ASSET_PREVIEW_EMPTY_PAD: f32 = 32.0;
pub const ASSET_PREVIEW_GLYPH_SIZE: f32 = 52.0;
/// 460px — maximum inline title-editor width inside an element header.
pub const ELEMENT_TITLE_EDIT_MAX_W: f32 = 460.0;

pub fn right_sidebar_min_w() -> Pixels {
    px(RIGHT_SIDEBAR_MIN_W)
}

pub fn panel_identity_tabs_gap() -> Pixels {
    px(PANEL_IDENTITY_TABS_GAP)
}

pub fn panel_tab_offset_y() -> Pixels {
    px(PANEL_TAB_OFFSET_Y)
}

pub fn panel_action_gap() -> Pixels {
    px(PANEL_ACTION_GAP)
}

pub fn composer_stop_glyph() -> Pixels {
    px(COMPOSER_STOP_GLYPH)
}

pub fn composer_input_min_h() -> Pixels {
    px(COMPOSER_INPUT_MIN_H)
}
pub fn composer_frame_h() -> Pixels {
    px(COMPOSER_FRAME_H)
}

pub fn menu_min_w() -> Pixels {
    px(MENU_MIN_W)
}
pub fn menu_max_w() -> Pixels {
    px(MENU_MAX_W)
}
pub fn menu_max_h() -> Pixels {
    px(MENU_MAX_H)
}
pub fn menu_outer_pad() -> Pixels {
    px(MENU_OUTER_PAD)
}
pub fn menu_item_pad_x() -> Pixels {
    px(MENU_ITEM_PAD_X)
}
pub fn menu_item_gap() -> Pixels {
    px(MENU_ITEM_GAP)
}
pub fn menu_icon_slot_w() -> Pixels {
    px(MENU_ICON_SLOT_W)
}
pub fn menu_separator_margin_x() -> Pixels {
    px(MENU_SEPARATOR_MARGIN_X)
}
pub fn menu_separator_margin_y() -> Pixels {
    px(MENU_SEPARATOR_MARGIN_Y)
}
pub fn menu_label_pad_top() -> Pixels {
    px(MENU_LABEL_PAD_TOP)
}
pub fn menu_label_pad_bottom() -> Pixels {
    px(MENU_LABEL_PAD_BOTTOM)
}
pub fn menu_separator_h() -> Pixels {
    px(MENU_SEPARATOR_H)
}
pub fn menu_window_margin() -> Pixels {
    px(MENU_WINDOW_MARGIN)
}
pub fn menu_submenu_flip_offset() -> Pixels {
    px(MENU_SUBMENU_FLIP_OFFSET)
}
pub fn menu_submenu_overlap() -> Pixels {
    px(MENU_SUBMENU_OVERLAP)
}

pub fn split_primary_pad_x() -> Pixels {
    px(SPLIT_PRIMARY_PAD_X)
}
pub fn split_caret_w() -> Pixels {
    px(SPLIT_CARET_W)
}
pub fn split_divider_w() -> Pixels {
    px(SPLIT_DIVIDER_W)
}

pub fn chat_card_head_pad_x() -> Pixels {
    px(CHAT_CARD_HEAD_PAD_X)
}
pub fn chat_card_head_pad_y() -> Pixels {
    px(CHAT_CARD_HEAD_PAD_Y)
}
pub fn chat_card_head_gap() -> Pixels {
    px(CHAT_CARD_HEAD_GAP)
}
pub fn chat_card_head_h() -> Pixels {
    px(CHAT_CARD_HEAD_H)
}
pub fn chat_card_body_pad_x() -> Pixels {
    px(CHAT_CARD_BODY_PAD_X)
}
pub fn chat_card_body_pad_y() -> Pixels {
    px(CHAT_CARD_BODY_PAD_Y)
}
pub fn chat_card_row_pad_x() -> Pixels {
    px(CHAT_CARD_ROW_PAD_X)
}
pub fn chat_card_row_pad_y() -> Pixels {
    px(CHAT_CARD_ROW_PAD_Y)
}
pub fn chat_card_row_gap() -> Pixels {
    px(CHAT_CARD_ROW_GAP)
}
pub fn agent_detail_bar_pad_x() -> Pixels {
    px(AGENT_DETAIL_BAR_PAD_X)
}
pub fn agent_detail_tab_group_gap() -> Pixels {
    px(AGENT_DETAIL_TAB_GROUP_GAP)
}
pub fn agent_detail_tab_pad_x() -> Pixels {
    px(AGENT_DETAIL_TAB_PAD_X)
}
pub fn agent_detail_tab_gap() -> Pixels {
    px(AGENT_DETAIL_TAB_GAP)
}
pub fn git_meta_row_pad_x() -> Pixels {
    px(GIT_META_ROW_PAD_X)
}
pub fn git_status_row_h() -> Pixels {
    px(GIT_STATUS_ROW_H)
}
pub fn context_panel_title_pad_x() -> Pixels {
    px(CONTEXT_PANEL_TITLE_PAD_X)
}
pub fn context_panel_title_pad_top() -> Pixels {
    px(CONTEXT_PANEL_TITLE_PAD_TOP)
}
pub fn context_panel_title_pad_bottom() -> Pixels {
    px(CONTEXT_PANEL_TITLE_PAD_BOTTOM)
}
pub fn context_panel_title_gap() -> Pixels {
    px(CONTEXT_PANEL_TITLE_GAP)
}
pub fn context_panel_flip_h() -> Pixels {
    px(CONTEXT_PANEL_FLIP_H)
}
pub fn context_panel_flip_pad_x() -> Pixels {
    px(CONTEXT_PANEL_FLIP_PAD_X)
}
pub fn context_panel_flip_gap() -> Pixels {
    px(CONTEXT_PANEL_FLIP_GAP)
}
pub fn context_panel_lane_pad_x() -> Pixels {
    px(CONTEXT_PANEL_LANE_PAD_X)
}
pub fn context_panel_lane_pad_top() -> Pixels {
    px(CONTEXT_PANEL_LANE_PAD_TOP)
}
pub fn context_panel_lane_pad_bottom() -> Pixels {
    px(CONTEXT_PANEL_LANE_PAD_BOTTOM)
}
pub fn context_panel_row_pad_x() -> Pixels {
    px(CONTEXT_PANEL_ROW_PAD_X)
}
pub fn context_panel_row_pad_y() -> Pixels {
    px(CONTEXT_PANEL_ROW_PAD_Y)
}
pub fn context_panel_row_gap() -> Pixels {
    px(CONTEXT_PANEL_ROW_GAP)
}
pub fn center_column_max_w() -> Pixels {
    px(CENTER_COLUMN_MAX_W)
}
pub fn center_column_pad_x() -> Pixels {
    px(CENTER_COLUMN_PAD_X)
}
pub fn center_column_pad_y() -> Pixels {
    px(CENTER_COLUMN_PAD_Y)
}
/// Project Home. The composer is the one raised surface; lanes are sunken wells
/// whose rows fill on hover, so the column reads as one strip without dividers.
pub fn services_group_gap() -> Pixels {
    px(SERVICES_GROUP_GAP)
}
pub fn services_fact_gap() -> Pixels {
    px(SERVICES_FACT_GAP)
}
pub fn services_env_key_w() -> Pixels {
    px(SERVICES_ENV_KEY_W)
}
pub fn services_header_title_gap() -> Pixels {
    px(SERVICES_HEADER_TITLE_GAP)
}
pub fn services_empty_gap() -> Pixels {
    px(SERVICES_EMPTY_GAP)
}
pub fn asset_detail_gap() -> Pixels {
    px(ASSET_DETAIL_GAP)
}
pub fn asset_preview_pad() -> Pixels {
    px(ASSET_PREVIEW_PAD)
}
pub fn asset_preview_empty_pad() -> Pixels {
    px(ASSET_PREVIEW_EMPTY_PAD)
}
pub fn asset_preview_glyph_size() -> Pixels {
    px(ASSET_PREVIEW_GLYPH_SIZE)
}

pub fn element_title_edit_max_w() -> Pixels {
    px(ELEMENT_TITLE_EDIT_MAX_W)
}

// ---- fonts ---------------------------------------------------------------

/// The monospace family — code, hex values, diff text, mono meta. The one place
/// the mono typeface is named (the UI family lives in `theme::UI_FONT_FAMILY`).
pub const FONT_MONO: &str = "Menlo";
/// Bundled Lucide glyph font for product icons absent from gpui-component.
pub const FONT_LUCIDE: &str = "lucide";

// ---- icon sizes ----------------------------------------------------------

/// 16px — default line icon.
pub const ICON: f32 = 16.0;
/// 12px — small inline icon (indicators, carets, step rows).
pub const ICON_SM: f32 = 12.0;
/// 13px — indicator-row icon (the header subline glyphs).
pub const ICON_IND: f32 = 13.0;
/// 14px — medium inline icon (button glyphs, list-row actions).
pub const ICON_MD: f32 = 14.0;
/// 20px — rail activity icon.
pub const ICON_LG: f32 = 20.0;
/// 24px — large feature icon (empty states, hero glyphs).
pub const ICON_XL: f32 = 24.0;

pub fn icon() -> Pixels {
    px(ICON)
}
pub fn icon_sm() -> Pixels {
    px(ICON_SM)
}
pub fn icon_ind() -> Pixels {
    px(ICON_IND)
}
pub fn icon_md() -> Pixels {
    px(ICON_MD)
}
pub fn icon_lg() -> Pixels {
    px(ICON_LG)
}
pub fn icon_xl() -> Pixels {
    px(ICON_XL)
}
