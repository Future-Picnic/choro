use std::collections::BTreeMap;
use std::path::Path;

use gpui::{
    div, img, prelude::FluentBuilder, px, svg, App, AppContext, ClipboardEntry, Context, Entity,
    Hsla, ImageFormat, InteractiveElement, IntoElement, ParentElement, PathPromptOptions, Pixels,
    Render, SharedString, StatefulInteractiveElement, Styled, StyledImage, WeakEntity, Window,
};
use gpui_component::{
    button::ButtonVariants,
    h_flex,
    input::{Input, InputState},
    menu::{ContextMenuExt, DropdownMenu as _, PopupMenuItem},
    v_flex, IconName, WindowExt,
};
use ide_core::{ProjectId, ProjectReference, ProjectReferenceKind};

use crate::state::designs::reference_folder;
use crate::state::{DesignsState, Workspace};
use crate::ui::center::{CenterArea, ContextMode};
use crate::ui::confirm::ConfirmDialog;
use crate::ui::style;

pub struct DesignsPanel {
    workspace: Entity<Workspace>,
    designs: Entity<DesignsState>,
    center: WeakEntity<CenterArea>,
    error: Option<String>,
}

impl DesignsPanel {
    pub fn view(
        workspace: Entity<Workspace>,
        designs: Entity<DesignsState>,
        center: WeakEntity<CenterArea>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            cx.observe(&designs, |_, _, cx| cx.notify()).detach();
            Self {
                workspace,
                designs,
                center,
                error: None,
            }
        })
    }

    fn active_project_id(&self, cx: &App) -> Option<ProjectId> {
        self.workspace
            .read(cx)
            .active_project()
            .map(|project| project.id)
    }

    /// Distinct folder names already in use for a project, for the folder picker.
    fn existing_folders(&self, project: ProjectId, cx: &App) -> Vec<String> {
        let mut folders: Vec<String> = self
            .designs
            .read(cx)
            .references_for_project(project)
            .iter()
            .filter_map(reference_folder)
            .collect();
        folders.sort();
        folders.dedup();
        folders
    }

    fn add_image(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Add Image".into()),
        });
        cx.spawn(async move |this, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else {
                return;
            };
            this.update(cx, |this, cx| {
                let result = this.designs.update(cx, |designs, cx| {
                    designs.create_image_reference(project, &path, cx)
                });
                this.handle_create_result(result, project, None, cx);
            })
            .ok();
        })
        .detach();
    }

    fn add_file(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Add File".into()),
        });
        cx.spawn(async move |this, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else {
                return;
            };
            this.update(cx, |this, cx| {
                let result = this.designs.update(cx, |designs, cx| {
                    designs.create_file_reference(project, &path, cx)
                });
                this.handle_create_result(result, project, None, cx);
            })
            .ok();
        })
        .detach();
    }

    fn paste_screenshot(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        let Some(image) = cx.read_from_clipboard().and_then(|item| {
            item.entries().iter().find_map(|entry| match entry {
                ClipboardEntry::Image(image) => Some(image.clone()),
                ClipboardEntry::String(_) => None,
            })
        }) else {
            self.error = Some("Clipboard does not contain an image.".into());
            cx.notify();
            return;
        };
        let extension = image_extension(image.format);
        let result = self.designs.update(cx, |designs, cx| {
            designs.create_image_reference_bytes(
                project,
                "Pasted screenshot",
                extension,
                &image.bytes,
                cx,
            )
        });
        self.handle_create_result(result, project, None, cx);
    }

    fn open_source_dialog(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let clipboard_source = cx
            .read_from_clipboard()
            .and_then(|item| {
                item.entries().iter().find_map(|entry| match entry {
                    ClipboardEntry::String(text) => Some(text.text().clone()),
                    ClipboardEntry::Image(_) => None,
                })
            })
            .unwrap_or_default();
        let folders = self.existing_folders(project, cx);
        let editor = cx.new(|cx| {
            DesignSourceDialog::new(clipboard_source, String::new(), folders, window, cx)
        });
        let panel = cx.entity().clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let editor = editor.clone();
            let save_editor = editor.clone();
            let panel = panel.clone();
            dialog
                .w(px(560.))
                .title(SharedString::from("Add asset"))
                .child(editor)
                .footer(move |_, _, _, cx| {
                    let editor = save_editor.clone();
                    let panel = panel.clone();
                    vec![
                        style::ghost_button_compact("add-design-source-cancel", "Cancel")
                            .custom(style::dialog_neutral_variant(cx))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        style::primary_button_compact("add-design-source-save", "Add", cx)
                            .on_click(move |_, window, cx| {
                                let Some((kind, title, source, notes, folder)) =
                                    editor.read(cx).collect(cx)
                                else {
                                    return;
                                };
                                panel.update(cx, |panel, cx| {
                                    let result = panel.designs.update(cx, |designs, cx| {
                                        designs.create_source_reference(
                                            project, kind, title, source, notes, cx,
                                        )
                                    });
                                    panel.handle_create_result(result, project, folder, cx);
                                });
                                window.close_dialog(cx);
                            }),
                    ]
                })
        });
    }

    fn handle_create_result(
        &mut self,
        result: anyhow::Result<ProjectReference>,
        project: ProjectId,
        folder: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(reference) => {
                self.error = None;
                if folder.is_some() {
                    let _ = self.designs.update(cx, |designs, cx| {
                        designs.set_reference_folder(reference.clone(), folder, cx)
                    });
                }
                let _ = self.center.update(cx, |center, cx| {
                    center.set_context_mode(ContextMode::Designs, cx);
                });
                self.designs
                    .update(cx, |designs, cx| designs.select(project, reference.id, cx));
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        cx.notify();
    }

    fn confirm_delete_reference(
        &mut self,
        project: ProjectId,
        reference_id: uuid::Uuid,
        title: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = cx.entity().clone();
        ConfirmDialog::new(
            "Delete asset?",
            format!("“{title}” will be removed from this project's assets."),
        )
        .confirm_label("Delete")
        .confirm_id("delete-design-reference-confirm")
        .on_confirm(move |_, cx| {
            panel.update(cx, |panel, cx| {
                panel.delete_reference(project, reference_id, cx);
            });
        })
        .open(window, cx);
    }

    fn delete_reference(
        &mut self,
        project: ProjectId,
        reference_id: uuid::Uuid,
        cx: &mut Context<Self>,
    ) {
        let result = self.designs.update(cx, |designs, cx| {
            designs.delete_reference(project, reference_id, cx)
        });
        if let Err(error) = result {
            self.error = Some(error.to_string());
        } else {
            self.error = None;
        }
        cx.notify();
    }

    fn render_reference_row(
        &self,
        project: ProjectId,
        reference: ProjectReference,
        is_selected: bool,
        key: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let kind = reference.kind;
        let title = SharedString::from(reference.title.clone());
        let delete_title = title.clone();
        let reference_id = reference.id;
        let panel = cx.entity().clone();
        let row_icon_color = design_kind_color(kind, cx);

        h_flex()
            .id(("design-reference-row", key))
            .w_full()
            .items_center()
            .gap(crate::ui::design::context_panel_row_gap())
            .px(crate::ui::design::context_panel_row_pad_x())
            .py(crate::ui::design::context_panel_row_pad_y())
            .cursor_pointer()
            .when(is_selected, |row| {
                row.bg(crate::ui::design::surface_2(cx))
                    .text_color(crate::ui::design::t1(cx))
            })
            .when(!is_selected, |row| {
                row.hover(|row| row.bg(crate::ui::design::surface_2(cx)))
            })
            .child(design_kind_glyph(
                kind,
                crate::ui::design::icon_md(),
                row_icon_color,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_head())
                    .font_weight(gpui::FontWeight::NORMAL)
                    .text_color(if is_selected {
                        crate::ui::design::t1(cx)
                    } else {
                        crate::ui::design::t2(cx)
                    })
                    .child(title),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.designs
                    .update(cx, |designs, cx| designs.select(project, reference_id, cx));
                let _ = this.center.update(cx, |center, cx| {
                    center.set_context_mode(ContextMode::Designs, cx);
                });
                cx.notify();
            }))
            .context_menu(move |menu, _, _| {
                let panel = panel.clone();
                let delete_title = delete_title.clone();
                menu.item(
                    PopupMenuItem::new("Remove from list")
                        .icon(IconName::Delete)
                        .on_click(move |_, window, cx| {
                            panel.update(cx, |this, cx| {
                                this.confirm_delete_reference(
                                    project,
                                    reference_id,
                                    delete_title.clone(),
                                    window,
                                    cx,
                                );
                            });
                        }),
                )
            })
            .into_any_element()
    }
}

impl Render for DesignsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_project = self.active_project_id(cx);
        // Catch assets added out-of-band (e.g. saved by an agent) without a restart.
        if let Some(project) = active_project {
            self.designs
                .update(cx, |designs, cx| designs.poll_refresh(project, cx));
        }
        let references = active_project
            .map(|project| self.designs.read(cx).references_for_project(project))
            .unwrap_or_default();
        let selected = active_project.and_then(|project| {
            self.designs
                .read(cx)
                .selected_reference(project)
                .map(|reference| reference.id)
        });
        let rows: Vec<gpui::AnyElement> = if let Some(project) = active_project {
            let mut rows = Vec::new();
            let mut key = 0usize;
            let mut groups: BTreeMap<String, Vec<ProjectReference>> = BTreeMap::new();
            for reference in &references {
                let lane = reference_folder(reference).unwrap_or_else(|| match reference.kind {
                    ProjectReferenceKind::Figma | ProjectReferenceKind::Url => {
                        "References".to_string()
                    }
                    ProjectReferenceKind::Image
                    | ProjectReferenceKind::Pencil
                    | ProjectReferenceKind::File => "Brand".to_string(),
                });
                groups.entry(lane).or_default().push(reference.clone());
            }
            for (lane, items) in groups {
                rows.push(asset_lane(&lane, cx));
                for reference in items {
                    let is_selected = selected == Some(reference.id);
                    rows.push(self.render_reference_row(project, reference, is_selected, key, cx));
                    key += 1;
                }
            }
            rows
        } else {
            Vec::new()
        };
        let view = cx.entity().clone();

        v_flex()
            .size_full()
            .child(
                crate::ui::design::header::panel_bar(cx)
                    .child(crate::ui::design::header::panel_identity(
                        None, "Assets", cx,
                    ))
                    .child(div().flex_1())
                    .when_some(active_project, |header, project| {
                        let view = view.clone();
                        header.child(
                            style::context_panel_action_button(
                                "design-add",
                                IconName::Plus,
                                "Add",
                                cx,
                            )
                            .dropdown_menu(move |menu, window, _| {
                                let view = view.clone();
                                menu.item(
                                    PopupMenuItem::new("Image…")
                                        .icon(IconName::GalleryVerticalEnd)
                                        .on_click(window.listener_for(
                                            &view,
                                            move |this, _, _, cx| {
                                                this.add_image(project, cx);
                                            },
                                        )),
                                )
                                .item(
                                    PopupMenuItem::new("Paste screenshot")
                                        .icon(IconName::Copy)
                                        .on_click(window.listener_for(
                                            &view,
                                            move |this, _, _, cx| {
                                                this.paste_screenshot(project, cx);
                                            },
                                        )),
                                )
                                .item(
                                    PopupMenuItem::new("Figma or URL…")
                                        .icon(IconName::Globe)
                                        .on_click(window.listener_for(
                                            &view,
                                            move |this, _, window, cx| {
                                                this.open_source_dialog(project, window, cx);
                                            },
                                        )),
                                )
                                .item(
                                    PopupMenuItem::new("File…").icon(IconName::File).on_click(
                                        window.listener_for(&view, move |this, _, _, cx| {
                                            this.add_file(project, cx);
                                        }),
                                    ),
                                )
                            }),
                        )
                    })
                    .child(style::right_sidebar_toggle(true, false, cx)),
            )
            .when_some(self.error.clone(), |panel, error| {
                panel.child(
                    div()
                        .mx_2()
                        .mb_2()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.24))
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .px_2()
                        .py_1()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .when(active_project.is_none(), |panel| {
                panel.child(style::empty_state(
                    IconName::GalleryVerticalEnd,
                    "No project open",
                    "Open a project to attach assets",
                    cx,
                ))
            })
            .when(active_project.is_some() && references.is_empty(), |panel| {
                panel.child(
                    v_flex()
                        .flex_1()
                        .min_h(px(0.))
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .text_color(crate::ui::design::t3(cx))
                        .child(
                            gpui_component::Icon::new(IconName::Image)
                                .size(crate::ui::design::icon_xl()),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .child("No assets yet"),
                        ),
                )
            })
            .when(
                active_project.is_some() && !references.is_empty(),
                |panel| {
                    panel.child(
                        v_flex()
                            .id("designs-panel-scroll")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .children(rows),
                    )
                },
            )
            .into_any_element()
    }
}

pub(crate) fn open_add_asset_dialog(
    project: ProjectId,
    designs: Entity<DesignsState>,
    window: &mut Window,
    cx: &mut App,
) {
    let clipboard_source = cx
        .read_from_clipboard()
        .and_then(|item| {
            item.entries().iter().find_map(|entry| match entry {
                ClipboardEntry::String(text) => Some(text.text().clone()),
                ClipboardEntry::Image(_) => None,
            })
        })
        .unwrap_or_default();
    let mut folders = designs
        .read(cx)
        .references_for_project(project)
        .iter()
        .filter_map(reference_folder)
        .collect::<Vec<_>>();
    folders.sort();
    folders.dedup();
    let editor =
        cx.new(|cx| DesignSourceDialog::new(clipboard_source, String::new(), folders, window, cx));

    window.open_dialog(cx, move |dialog, _, _| {
        let save_editor = editor.clone();
        let save_designs = designs.clone();
        dialog
            .w(px(560.))
            .title(SharedString::from("Add asset"))
            .child(editor.clone())
            .footer(move |_, _, _, cx| {
                let editor = save_editor.clone();
                let designs = save_designs.clone();
                vec![
                    style::ghost_button_compact("center-add-asset-cancel", "Cancel")
                        .custom(style::dialog_neutral_variant(cx))
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                    style::primary_button_compact("center-add-asset-save", "Add", cx).on_click(
                        move |_, window, cx| {
                            let Some((kind, title, source, notes, folder)) =
                                editor.read(cx).collect(cx)
                            else {
                                return;
                            };
                            let result = designs.update(cx, |designs, cx| {
                                designs.create_source_reference(
                                    project, kind, title, source, notes, cx,
                                )
                            });
                            match result {
                                Ok(reference) => {
                                    designs.update(cx, |designs, cx| {
                                        if folder.is_some() {
                                            let _ = designs.set_reference_folder(
                                                reference.clone(),
                                                folder,
                                                cx,
                                            );
                                        }
                                        designs.select(project, reference.id, cx);
                                    });
                                    window.close_dialog(cx);
                                }
                                Err(error) => eprintln!("failed to add asset: {error:#}"),
                            }
                        },
                    ),
                ]
            })
    });
}

struct DesignSourceDialog {
    title: Entity<InputState>,
    source: Entity<InputState>,
    notes: Entity<InputState>,
    folder: Entity<crate::ui::folder_picker::FolderPicker>,
}

impl DesignSourceDialog {
    fn new(
        default_source: String,
        default_folder: String,
        folders: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let default_title = if default_source.trim().is_empty() {
            String::new()
        } else {
            title_for_source(default_source.trim())
        };
        let initial_folder =
            (!default_folder.trim().is_empty()).then(|| default_folder.trim().to_string());
        Self {
            title: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Title")
                    .default_value(default_title)
            }),
            source: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Figma URL or web URL")
                    .default_value(default_source)
            }),
            notes: cx.new(|cx| InputState::new(window, cx).placeholder("Notes")),
            folder: cx.new(|cx| {
                crate::ui::folder_picker::FolderPicker::new(folders, initial_folder, window, cx)
            }),
        }
    }

    fn folder_value(&self, cx: &App) -> Option<String> {
        self.folder.read(cx).value()
    }

    fn collect(
        &self,
        cx: &App,
    ) -> Option<(ProjectReferenceKind, String, String, String, Option<String>)> {
        let source = self.source.read(cx).value().trim().to_string();
        if source.is_empty() {
            return None;
        }
        let title = self.title.read(cx).value().trim().to_string();
        let title = if title.is_empty() {
            title_for_source(&source)
        } else {
            title
        };
        let notes = self.notes.read(cx).value().trim().to_string();
        let folder = self.folder_value(cx);
        Some((kind_for_source(&source), title, source, notes, folder))
    }

    fn render_folder_field(&self, cx: &mut Context<Self>) -> gpui::Div {
        design_field_group("Folder", cx).child(self.folder.clone())
    }
}

impl Render for DesignSourceDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let detected = kind_for_source(&self.source.read(cx).value());
        v_flex()
            .gap_4()
            .child(design_field_group("Title", cx).child(Input::new(&self.title)))
            .child(
                design_field_group("Source", cx)
                    .child(Input::new(&self.source))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .pt_0p5()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Detected"),
                            )
                            .child(design_kind_badge(detected, cx)),
                    ),
            )
            .child(self.render_folder_field(cx))
            .child(design_field_group("Notes", cx).child(Input::new(&self.notes)))
    }
}

/// A labelled field group for the design dialogs: small caption above the input.
fn design_field_group(label: &'static str, cx: &App) -> gpui::Div {
    v_flex().gap_1p5().child(
        div()
            .text_size(crate::ui::design::text_label())
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(crate::ui::design::t3(cx))
            .child(label),
    )
}

fn asset_lane(label: &str, cx: &App) -> gpui::AnyElement {
    div()
        .px(crate::ui::design::context_panel_lane_pad_x())
        .pt(crate::ui::design::context_panel_lane_pad_top())
        .pb(crate::ui::design::context_panel_lane_pad_bottom())
        .text_size(crate::ui::design::text_label())
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(crate::ui::design::t4(cx))
        .child(SharedString::from(label.to_uppercase()))
        .into_any_element()
}

fn title_for_source(source: &str) -> String {
    let path = Path::new(source);
    path.file_stem()
        .or_else(|| path.file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            source
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .split('/')
                .find(|part| !part.is_empty())
                .unwrap_or("Asset")
                .to_string()
        })
}

fn kind_for_source(source: &str) -> ProjectReferenceKind {
    let lower = source.to_ascii_lowercase();
    if lower.contains("figma.com") {
        ProjectReferenceKind::Figma
    } else {
        ProjectReferenceKind::Url
    }
}

pub(crate) fn design_kind_label(kind: ProjectReferenceKind) -> &'static str {
    match kind {
        ProjectReferenceKind::Image => "Image",
        ProjectReferenceKind::Figma => "Figma",
        ProjectReferenceKind::Pencil => "Pencil",
        ProjectReferenceKind::Url => "URL",
        ProjectReferenceKind::File => "File",
    }
}

/// A distinct accent per reference kind so the gallery scans by type at a glance.
pub(crate) fn design_kind_color(kind: ProjectReferenceKind, cx: &App) -> gpui::Hsla {
    match kind {
        ProjectReferenceKind::Image => crate::ui::design::sky(cx),
        ProjectReferenceKind::Figma => crate::ui::design::accent(cx),
        ProjectReferenceKind::Pencil => crate::ui::design::sage(cx),
        ProjectReferenceKind::Url => crate::ui::design::t3(cx),
        ProjectReferenceKind::File => crate::ui::design::amber(cx),
    }
}

/// The bare kind glyph — no tile or background, sized to `size`. Figma keeps
/// its brand colors (rendered as an image); the others are single-color marks
/// that tint to `color`.
pub(crate) fn design_kind_glyph(
    kind: ProjectReferenceKind,
    size: Pixels,
    color: Hsla,
) -> gpui::AnyElement {
    match kind {
        ProjectReferenceKind::Figma => img("icons/asset-figma.svg")
            .size(size)
            .object_fit(gpui::ObjectFit::Contain)
            .into_any_element(),
        ProjectReferenceKind::Url => svg()
            .path("icons/asset-url.svg")
            .size(size)
            .text_color(color)
            .into_any_element(),
        ProjectReferenceKind::File => svg()
            .path("icons/asset-file.svg")
            .size(size)
            .text_color(color)
            .into_any_element(),
        ProjectReferenceKind::Pencil => svg()
            .path("agent-icons/pencil.svg")
            .size(size)
            .text_color(color)
            .into_any_element(),
        ProjectReferenceKind::Image => svg()
            .path("icons/asset-image.svg")
            .size(size)
            .text_color(color)
            .into_any_element(),
    }
}

/// A small tinted pill — icon + kind label — used in the gallery and the detail
/// header so a reference's type reads instantly.
pub(crate) fn design_kind_badge(kind: ProjectReferenceKind, cx: &App) -> gpui::AnyElement {
    let color = design_kind_color(kind, cx);
    h_flex()
        .flex_none()
        .items_center()
        .gap_1()
        .px_1p5()
        .py_0p5()
        .rounded(crate::ui::design::r_sm())
        .bg(color.opacity(0.14))
        .child(design_kind_glyph(kind, crate::ui::design::icon_sm(), color))
        .child(
            div()
                .text_size(crate::ui::design::text_label())
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(color)
                .child(design_kind_label(kind)),
        )
        .into_any_element()
}

fn image_extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Webp => "webp",
        ImageFormat::Gif => "gif",
        ImageFormat::Svg => "svg",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Tiff => "tiff",
    }
}
