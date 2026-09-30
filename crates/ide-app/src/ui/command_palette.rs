use gpui::{
    actions, div, prelude::FluentBuilder, px, AnyElement, App, AppContext, Context, FocusHandle,
    FontWeight, InteractiveElement, IntoElement, KeyBinding, ParentElement, Render, ScrollHandle,
    StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    input::{Input, InputEvent, InputState},
    v_flex, Icon, IconName, WindowExt,
};

use crate::keymap;
use crate::ui::{design, palette_ui};

actions!(
    command_palette,
    [CommandNext, CommandPrevious, CommandConfirm]
);

const CONTEXT: &str = "CommandPalette";

pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("down", CommandNext, Some("CommandPalette > Input")),
        KeyBinding::new("up", CommandPrevious, Some("CommandPalette > Input")),
        KeyBinding::new("enter", CommandConfirm, Some("CommandPalette > Input")),
        KeyBinding::new(
            "secondary-enter",
            CommandConfirm,
            Some("CommandPalette > Input"),
        ),
    ]
}

pub struct CommandPalette {
    input: gpui::Entity<InputState>,
    selected: usize,
    overrides: std::collections::HashMap<String, String>,
    workspace_focus: FocusHandle,
    scroll: ScrollHandle,
}

impl CommandPalette {
    pub fn open(workspace_focus: FocusHandle, window: &mut Window, cx: &mut App) {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Type a command…")
                .default_value("")
        });
        let palette = cx.new(|cx| {
            cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.selected = 0;
                    cx.notify();
                }
            })
            .detach();
            Self {
                input,
                selected: 0,
                overrides: ide_core::AppConfig::load().keymap,
                workspace_focus,
                scroll: ScrollHandle::new(),
            }
        });
        let dialog_palette = palette.clone();
        palette_ui::soften_overlay(cx);
        window.open_dialog(cx, move |dialog, _, cx| {
            palette_ui::styled_dialog(dialog, palette_ui::DIALOG_W, cx)
                .child(dialog_palette.clone())
        });
        let input = palette.read(cx).input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn filtered_indexes(&self, cx: &App) -> Vec<usize> {
        let query = self.input.read(cx).value().trim().to_lowercase();
        keymap::shortcuts()
            .iter()
            .enumerate()
            .filter(|(_, command)| command.in_commands)
            .filter(|(_, command)| {
                query.is_empty()
                    || command.title.to_lowercase().contains(&query)
                    || command.description.to_lowercase().contains(&query)
                    || command.category.title().to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.filtered_indexes(cx).len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected as isize + delta).rem_euclid(len as isize) as usize;
        cx.notify();
    }

    fn run_index(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(command) = keymap::shortcuts().into_iter().nth(index) else {
            return;
        };
        let action = command.action();
        palette_ui::close(window, cx);
        window.focus(&self.workspace_focus);
        window.dispatch_action(action, cx);
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.filtered_indexes(cx).get(self.selected).copied() else {
            return;
        };
        self.run_index(index, window, cx);
    }

    fn render_rows(
        &self,
        indexes: &[usize],
        query: &str,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let shortcuts = keymap::shortcuts();
        let mut rows = Vec::new();
        let mut last_category = None;

        for (row_index, index) in indexes.iter().copied().enumerate() {
            let command = &shortcuts[index];
            if last_category != Some(command.category) {
                rows.push(palette_ui::group_label(command.category.title(), cx).into_any_element());
                last_category = Some(command.category);
            }

            let selected = row_index == self.selected;
            if selected {
                self.scroll.scroll_to_item(rows.len());
            }
            let display = command
                .keystroke(&self.overrides)
                .map(keymap::display_keystroke);
            rows.push(
                palette_ui::row(selected, cx)
                    .id(("command-palette-row", index))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.run_index(index, window, cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(design::text_body())
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(palette_ui::highlighted_title(command.title, query, cx)),
                    )
                    .when_some(display, |row, display| {
                        row.child(palette_ui::keycap(display, cx))
                    })
                    .into_any_element(),
            );
        }

        rows
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let indexes = self.filtered_indexes(cx);
        self.selected = self.selected.min(indexes.len().saturating_sub(1));
        let query = self.input.read(cx).value().trim().to_string();
        let count = indexes.len();
        let rows = self.render_rows(&indexes, &query, cx);

        palette_ui::frame(cx)
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &CommandNext, _, cx| this.move_selection(1, cx)))
            .on_action(cx.listener(|this, _: &CommandPrevious, _, cx| this.move_selection(-1, cx)))
            .on_action(cx.listener(|this, _: &CommandConfirm, window, cx| this.confirm(window, cx)))
            .child(
                palette_ui::header_band()
                    .child(
                        Icon::new(IconName::Search)
                            .size(design::icon())
                            .text_color(design::t3(cx)),
                    )
                    .child(
                        div().flex_1().min_w(px(0.)).child(
                            Input::new(&self.input)
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false),
                        ),
                    )
                    .child(
                        div()
                            .text_size(design::text_ui())
                            .text_color(design::t3(cx))
                            .child("Commands"),
                    ),
            )
            .child(
                v_flex()
                    .id("command-palette-results")
                    .track_scroll(&self.scroll)
                    .max_h(px(390.))
                    .min_h(px(72.))
                    .px(px(palette_ui::LIST_PAD))
                    .pb(px(palette_ui::LIST_PAD_BOTTOM))
                    .gap_0p5()
                    .overflow_y_scroll()
                    .when(rows.is_empty(), |list| {
                        list.child(
                            div()
                                .h(px(56.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(design::text_body())
                                .text_color(design::t3(cx))
                                .child("No matching commands"),
                        )
                    })
                    .children(rows),
            )
            .child(
                palette_ui::footer_band()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(design::text_ui())
                            .text_color(design::t4(cx))
                            .child(format!("{count} commands")),
                    )
                    .child(palette_ui::key_hint("↑↓", "navigate", cx))
                    .child(palette_ui::key_hint("↩", "run", cx))
                    .child(palette_ui::key_hint("esc", "close", cx)),
            )
    }
}
