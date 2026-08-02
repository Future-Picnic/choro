use std::path::PathBuf;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled,
    WeakEntity, Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputState},
    menu::{PopupMenu, PopupMenuItem},
    v_flex, IconName, WindowExt,
};
use ide_core::ProjectId;

use crate::state::DocsState;
use crate::ui::center::CenterArea;
use crate::ui::style;

pub fn doc_template_menu(
    mut menu: PopupMenu,
    docs: Entity<DocsState>,
    center: WeakEntity<CenterArea>,
    project: ProjectId,
    _window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let templates = docs.read(cx).doc_template_options(project);
    menu = menu.item(PopupMenuItem::label("START FROM"));
    for template in templates {
        let source = template.source.clone();
        let center = center.clone();
        menu = menu.item(
            PopupMenuItem::new(template.name)
                .icon(IconName::FileText)
                .on_click(move |_, _, cx| {
                    let _ = center.update(cx, |center, cx| {
                        center.create_doc_from_template(project, source.clone(), cx)
                    });
                }),
        );
    }
    menu
}

pub fn open_save_template_dialog(
    docs: Entity<DocsState>,
    project: ProjectId,
    path: PathBuf,
    default_name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let editor = cx.new(|cx| SaveDocTemplateDialog {
        docs,
        project,
        path,
        name: cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Template name")
                .default_value(default_name)
        }),
        error: None,
    });
    let dialog_editor = editor.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let footer_editor = dialog_editor.clone();
        dialog
            .w(px(500.))
            .title(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(crate::ui::confirm::icon_badge(
                        IconName::Copy,
                        crate::ui::design::accent(cx),
                        cx,
                    ))
                    .child("Save as template"),
            )
            .child(dialog_editor.clone())
            .footer(move |_, _, _, cx| {
                let save_editor = footer_editor.clone();
                vec![
                    style::dialog_neutral_button("save-template-cancel", "Cancel", cx)
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                    style::primary_button_compact("save-template-confirm", "Save Template", cx)
                        .icon(IconName::Check)
                        .on_click(move |_, window, cx| {
                            let saved = save_editor.update(cx, |editor, cx| editor.save(cx));
                            if saved {
                                window.close_dialog(cx);
                            }
                        }),
                ]
            })
    });
    let input = editor.read(cx).name.clone();
    input.update(cx, |input, cx| input.focus(window, cx));
}

struct SaveDocTemplateDialog {
    docs: Entity<DocsState>,
    project: ProjectId,
    path: PathBuf,
    name: Entity<InputState>,
    error: Option<String>,
}

impl SaveDocTemplateDialog {
    fn save(&mut self, cx: &mut Context<Self>) -> bool {
        let name = self.name.read(cx).value().trim().to_string();
        match self.docs.update(cx, |docs, cx| {
            docs.create_project_template_from_doc(self.project, &self.path, &name, cx)
        }) {
            Ok(_) => {
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
                false
            }
        }
    }
}

impl Render for SaveDocTemplateDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_size(crate::ui::design::text_body())
                    .text_color(crate::ui::design::t3(cx))
                    .child("Save a snapshot of this document as a reusable project template."),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child("Template name"),
                    )
                    .child(Input::new(&self.name)),
            )
            .when_some(self.error.clone(), |view, error| {
                view.child(
                    div()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.24))
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .px_3()
                        .py_2()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
    }
}
