//! The row inspector: the selected row read top to bottom, one field per
//! line with its declared type, so wide tables and long values can be read
//! without scrolling sideways. Editing still goes through the keyed row
//! editor and its production confirmation.

use super::*;

const INSPECTOR_MIN_W: f32 = 240.;
const INSPECTOR_MAX_W: f32 = 380.;
const INSPECTOR_HEADER_H: f32 = 36.;

impl TablePane {
    pub(super) fn render_inspector(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let row = self.grid.rows.get(index)?;
        let editable = self.table_editable && !self.handle.is_read_only();
        let original = row.original.clone();
        let number = self.page * PAGE_SIZE + index as u64 + 1;
        let line = crate::ui::design::line(cx);

        let header = h_flex()
            .w_full()
            .h(px(INSPECTOR_HEADER_H))
            .flex_none()
            .pl_3()
            .pr_1()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(line.opacity(0.22))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t1(cx))
                    .child(format!("Row {number}")),
            )
            .when(editable, |header| {
                header.child(
                    crate::ui::style::secondary_button_compact("db-inspector-edit", "Edit row")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_row_editor(original.clone(), window, cx);
                        })),
                )
            })
            .child(
                crate::ui::style::header_icon_button("db-inspector-close", IconName::Close, cx)
                    .tooltip("Close inspector")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.selected_row = None;
                        cx.notify();
                    })),
            );

        let fields = self
            .grid
            .columns
            .iter()
            .zip(&row.cells)
            .map(|(column, value)| {
                v_flex()
                    .w_full()
                    .px_3()
                    .py_1p5()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(line.opacity(0.12))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1p5()
                            .items_center()
                            .when(column.primary_key, |label| {
                                label.child(crate::ui::design::indicator::lucide_icon(
                                    lucide_icons::Icon::KeyRound,
                                    crate::ui::design::amber(cx),
                                    crate::ui::design::icon_sm(),
                                ))
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(crate::ui::design::text_label())
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::t2(cx))
                                    .child(column.name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .max_w(px(120.))
                                    .truncate()
                                    .font_family(crate::ui::design::FONT_MONO)
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(column.data_type.clone()),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_ui())
                            .line_height(gpui::relative(1.4))
                            .whitespace_normal()
                            .text_color(data_grid::value_color(value, cx))
                            .when(value.is_absent(), |text| text.italic())
                            .child(value.inspector_text()),
                    )
            })
            .collect::<Vec<_>>();

        Some(
            v_flex()
                .debug_selector(|| "db-row-inspector".into())
                .flex_none()
                .h_full()
                .w(gpui::relative(0.34))
                .min_w(px(INSPECTOR_MIN_W))
                .max_w(px(INSPECTOR_MAX_W))
                .border_l_1()
                .border_color(line.opacity(0.28))
                .bg(crate::ui::design::surface(cx))
                .child(header)
                .child(
                    v_flex()
                        .id("db-inspector-fields")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .children(fields),
                )
                .into_any_element(),
        )
    }
}
