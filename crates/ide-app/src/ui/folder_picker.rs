//! A search-or-create folder picker, modelled on the Git panel's branch
//! selector: a button that expands an inline popup where you filter existing
//! folders by typing and either pick one, clear the folder, or create a new one
//! from the query. Embedded by the asset Add/Edit dialogs so folder entry is
//! never typo-prone free text.

use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    button::Button,
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, Icon, IconName, Sizable,
};

pub struct FolderPicker {
    query: Entity<InputState>,
    folders: Vec<String>,
    selected: Option<String>,
    open: bool,
}

impl FolderPicker {
    pub fn new(
        folders: Vec<String>,
        initial: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search or create folder…"));
        // Re-render as the user types so the filtered list updates live.
        cx.subscribe(&query, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        Self {
            query,
            folders,
            selected: initial.filter(|folder| !folder.trim().is_empty()),
            open: false,
        }
    }

    /// The chosen folder, or `None` for "no folder".
    pub fn value(&self) -> Option<String> {
        self.selected.clone()
    }

    fn choose(&mut self, folder: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = folder;
        self.open = false;
        self.query
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    fn render_popup(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.query.read(cx).value().trim().to_string();
        let needle = query.to_lowercase();
        let matches: Vec<String> = self
            .folders
            .iter()
            .filter(|folder| needle.is_empty() || folder.to_lowercase().contains(&needle))
            .cloned()
            .collect();
        let exact = self
            .folders
            .iter()
            .any(|folder| folder.eq_ignore_ascii_case(query.trim()));
        let can_create = !query.trim().is_empty() && !exact;
        let selected = self.selected.clone();

        v_flex()
            .w_full()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.42))
            .bg(crate::ui::design::focus(cx))
            .text_color(crate::ui::design::t1(cx))
            .shadow_lg()
            .overflow_hidden()
            .child(
                v_flex()
                    .id("folder-picker-scroll")
                    .max_h(px(220.))
                    .overflow_y_scroll()
                    .p_1()
                    .gap_0p5()
                    .child(folder_row(
                        "folder-none",
                        IconName::Close,
                        "No folder",
                        selected.is_none(),
                        cx.listener(|this, _, window, cx| this.choose(None, window, cx)),
                        cx,
                    ))
                    .when(can_create, |list| {
                        let name = query.trim().to_string();
                        let create = name.clone();
                        list.child(folder_row(
                            "folder-create",
                            IconName::Plus,
                            SharedString::from(format!("Create “{name}”")),
                            false,
                            cx.listener(move |this, _, window, cx| {
                                this.choose(Some(create.clone()), window, cx)
                            }),
                            cx,
                        ))
                    })
                    .children(matches.into_iter().enumerate().map(|(index, folder)| {
                        let is_selected = selected.as_deref() == Some(folder.as_str());
                        let chosen = folder.clone();
                        folder_row(
                            ("folder-item", index),
                            IconName::Folder,
                            SharedString::from(folder),
                            is_selected,
                            cx.listener(move |this, _, window, cx| {
                                this.choose(Some(chosen.clone()), window, cx)
                            }),
                            cx,
                        )
                    })),
            )
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.28))
                    .child(div().flex_1().min_w(px(0.)).child(Input::new(&self.query)))
                    .child(
                        Icon::new(IconName::Search)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t3(cx)),
                    ),
            )
    }
}

fn folder_row(
    id: impl Into<gpui::ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    is_selected: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .id(id.into())
        .w_full()
        .px_2()
        .py_1()
        .gap_2()
        .items_center()
        .rounded(crate::ui::design::r_sm())
        .cursor_pointer()
        .when(is_selected, |row| row.bg(crate::ui::design::surface_2(cx)))
        .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
        .on_click(on_click)
        .child(
            Icon::new(icon)
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::t3(cx)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(crate::ui::design::text_body())
                .truncate()
                .child(label.into()),
        )
}

impl Render for FolderPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label: SharedString = self
            .selected
            .clone()
            .map(SharedString::from)
            .unwrap_or_else(|| "No folder".into());
        v_flex()
            .w_full()
            .gap_1()
            .child(
                Button::new("folder-picker-toggle")
                    .outline()
                    .small()
                    .w_full()
                    .label(label)
                    .icon(IconName::ChevronDown)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open = !this.open;
                        if this.open {
                            this.query
                                .update(cx, |input, cx| input.set_value("", window, cx));
                        }
                        cx.notify();
                    })),
            )
            .when(self.open, |field| field.child(self.render_popup(cx)))
    }
}
