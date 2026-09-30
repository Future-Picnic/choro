use super::*;

use ide_core::local_store::LocalStore;
use ide_core::{ProjectReference, ProjectReferenceKind};

impl CenterArea {
    pub(super) fn render_context_section(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match self.context_mode {
            ContextMode::Docs => self.render_docs_section(project, window, cx),
            ContextMode::Designs => self.render_design_reference_section(project, window, cx),
        }
    }

    fn render_design_reference_section(
        &mut self,
        project: ProjectId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(reference) = self.designs.read(cx).selected_reference(project) else {
            let has_assets = !self
                .designs
                .read(cx)
                .references_for_project(project)
                .is_empty();
            let designs = self.designs.clone();

            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_4()
                .child(
                    div()
                        .w(px(160.))
                        .h(px(110.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            crate::ui::illustrations::illustration(
                                crate::ui::illustrations::Illustration::Assets,
                                cx,
                            )
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                        ),
                )
                .child(
                    v_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_title())
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(crate::ui::design::t1(cx))
                                .child(if has_assets {
                                    "Choose an asset"
                                } else {
                                    "No assets yet"
                                }),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child(if has_assets {
                                    "Select an asset from the Assets sidebar."
                                } else {
                                    "Add a Figma link or URL to start building your references."
                                }),
                        ),
                )
                .when(!has_assets, |empty| {
                    empty.child(
                        style::primary_button("assets-empty-add", "Add asset", cx)
                            .icon(IconName::Plus)
                            .on_click(move |_, window, cx| {
                                crate::ui::designs_panel::open_add_asset_dialog(
                                    project,
                                    designs.clone(),
                                    window,
                                    cx,
                                );
                            }),
                    )
                })
                .into_any_element();
        };

        let preview_path = design_reference_preview_path(&reference);
        let kind = reference.kind;
        let title = reference.title.clone();
        let notes = reference.notes.clone();
        let accent = crate::ui::designs_panel::design_kind_color(kind, cx);
        let open_reference = reference.clone();
        let edit_reference = reference.clone();
        let copy_reference = reference.clone();
        let agent_reference = reference.clone();
        let preview_open_reference = reference.clone();

        v_flex()
            .size_full()
            .bg(crate::ui::design::base(cx))
            .child(
                crate::ui::design::header::workspace_bar(cx)
                    .child(
                        crate::ui::design::header::title_col(cx)
                            .child(crate::ui::design::header::title(title, cx))
                            .child(crate::ui::design::header::subline().child(
                                crate::ui::design::indicator::subline_indicator_with_icon(
                                    crate::ui::designs_panel::design_kind_glyph(
                                        kind,
                                        crate::ui::design::icon_ind(),
                                        accent,
                                    ),
                                    crate::ui::designs_panel::design_kind_label(kind),
                                    cx,
                                ),
                            )),
                    )
                    .child(
                        crate::ui::design::header::actions()
                            .child(
                                style::secondary_button_compact("design-open-source", "Open")
                                    .icon(IconName::ExternalLink)
                                    .tooltip("Open the source in its app")
                                    .on_click(move |_, _, _| {
                                        open_design_reference_source(&open_reference)
                                    }),
                            )
                            .child(
                                style::ghost_button_compact("design-edit-reference", "Edit")
                                    .icon(IconName::Settings)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_design_reference_edit_dialog(
                                            project,
                                            edit_reference.clone(),
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                style::header_icon_button(
                                    "design-copy-markdown",
                                    IconName::Copy,
                                    cx,
                                )
                                .tooltip("Copy as Markdown")
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        design_reference_markdown(&copy_reference),
                                    ));
                                }),
                            )
                            .child(
                                style::implement_button("design-use-agent", "Use in Agent", cx)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.use_design_reference_in_agent(
                                            project,
                                            agent_reference.clone(),
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                    ),
            )
            .child(
                // The asset owns the whole width now — no metadata column. Type
                // and source live in the header above; notes sit in a slim strip
                // under the preview.
                v_flex()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .px(crate::ui::design::center_column_pad_x())
                    .py(crate::ui::design::center_column_pad_y())
                    .gap(crate::ui::design::asset_detail_gap())
                    .child(self.render_design_preview_region(
                        kind,
                        accent,
                        preview_path,
                        preview_open_reference.clone(),
                        cx,
                    ))
                    .child(self.render_design_notes_strip(project, reference.clone(), notes, cx))
                    // The asset alone, without the header's Open / Edit / Copy /
                    // Use in Agent. The tour's "look at it" step has nothing to
                    // gain from lighting four controls beside it.
                    .child(crate::ui::onboarding::target_marker(
                        crate::ui::onboarding::SpotlightTarget::AssetBody,
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// A slim strip under the preview for the one piece of metadata worth
    /// surfacing: the note the agent should read. Shows the note when present,
    /// or a quiet "add note" affordance; either way clicking opens the edit
    /// dialog where the note is edited.
    fn render_design_notes_strip(
        &self,
        project: ProjectId,
        reference: ProjectReference,
        notes: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let has_note = !notes.trim().is_empty();
        let (icon, text, text_color) = if has_note {
            (
                IconName::Info,
                SharedString::from(notes.trim().to_string()),
                crate::ui::design::t2(cx),
            )
        } else {
            (
                IconName::Plus,
                SharedString::from("Add a note for the agent"),
                crate::ui::design::t3(cx),
            )
        };
        h_flex()
            .id("design-notes-strip")
            .w_full()
            .flex_none()
            .items_center()
            .gap(crate::ui::design::context_panel_row_gap())
            .px(crate::ui::design::chat_card_row_pad_x())
            .py(crate::ui::design::chat_card_head_pad_y())
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(style::border(cx))
            .bg(style::surface(cx))
            .cursor_pointer()
            .hover(|strip| strip.border_color(crate::ui::design::line(cx)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_design_reference_edit_dialog(project, reference.clone(), window, cx);
            }))
            .child(
                Icon::new(icon)
                    .size(crate::ui::design::icon())
                    .text_color(crate::ui::design::t3(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(crate::ui::design::text_ui())
                    .truncate()
                    .text_color(text_color)
                    .child(text),
            )
            .when(has_note, |strip| {
                strip.child(
                    Icon::new(IconName::Settings)
                        .size(crate::ui::design::icon())
                        .text_color(crate::ui::design::t3(cx)),
                )
            })
    }

    /// The single preview boundary in the design detail view.
    ///
    /// For URL references it hosts a live in-app web page: a native `WKWebView`
    /// is overlaid on the `canvas` region below (see `web_preview`), with a
    /// native "Loading…" backdrop that shows only until WebKit paints (or if the
    /// build fails). For every other kind it shows the cached image or the
    /// static kind placeholder. Only one webview is ever alive (see `web_host`).
    fn render_design_preview_region(
        &self,
        kind: ProjectReferenceKind,
        accent: gpui::Hsla,
        preview_path: Option<std::path::PathBuf>,
        reference: ProjectReference,
        cx: &App,
    ) -> gpui::AnyElement {
        let container = div()
            .flex_1()
            .w_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .rounded(crate::ui::design::r_lg())
            .border_1()
            .border_color(style::border(cx))
            .overflow_hidden();

        if design_web_preview_url(&reference).is_some() {
            let host = self.web_host.clone();
            // The onboarding tour docks an action bar under the live page on its
            // final step. The WKWebView paints above all gpui content, so the bar
            // can't simply overlay it — instead we reserve a strip at the bottom
            // and shrink the webview out of it, leaving the strip for gpui.
            let reserve = crate::ui::onboarding::reserves_preview_action_bar(cx);
            let bar_h = px(crate::ui::onboarding::PREVIEW_ACTION_BAR_H);
            container
                .relative()
                .child(
                    v_flex()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .gap(crate::ui::design::asset_detail_gap())
                        .child(crate::ui::designs_panel::design_kind_glyph(
                            kind,
                            crate::ui::design::asset_preview_glyph_size(),
                            accent,
                        ))
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("Loading web preview…"),
                        ),
                )
                // Zero-footprint region that reports its laid-out rect to the
                // webview host each paint, so the WKWebView tracks resizes.
                // During the tour's final step its bottom is lifted by `bar_h`
                // so the native view never sits under the action bar.
                .child(
                    canvas(
                        move |bounds, window, cx| {
                            host.update(cx, |host, _| host.place(bounds, window));
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0()
                    .when(reserve, |c| c.bottom(bar_h)),
                )
                // The freed strip: a target the tour anchors its action bar to.
                .when(reserve, |region| {
                    region.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(bar_h)
                            .child(crate::ui::onboarding::target_marker(
                                crate::ui::onboarding::SpotlightTarget::PreviewActionZone,
                                cx,
                            )),
                    )
                })
                .into_any_element()
        } else {
            container
                .flex()
                .items_center()
                .justify_center()
                .child(match preview_path.filter(|path| path.is_file()) {
                    Some(path) => img(path)
                        .max_w_full()
                        .max_h_full()
                        .object_fit(ObjectFit::Contain)
                        .with_fallback(move || design_kind_placeholder(kind, accent))
                        .into_any_element(),
                    None => self.render_design_preview_placeholder(kind, accent, reference, cx),
                })
                .into_any_element()
        }
    }

    /// The center preview for a reference without an image (Figma / URL / Pencil):
    /// a large tinted kind glyph plus a one-tap "open in its app" action.
    fn render_design_preview_placeholder(
        &self,
        kind: ProjectReferenceKind,
        accent: gpui::Hsla,
        reference: ProjectReference,
        cx: &App,
    ) -> gpui::AnyElement {
        let (caption, action) = match kind {
            ProjectReferenceKind::Figma => ("Figma design", Some("Open in Figma")),
            ProjectReferenceKind::Url => ("Web reference", Some("Open in browser")),
            ProjectReferenceKind::Pencil => ("Pencil file", Some("Open .pen file")),
            ProjectReferenceKind::File => ("File", Some("Open file")),
            ProjectReferenceKind::Image => ("No preview image", None),
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(crate::ui::design::asset_detail_gap())
            .p(crate::ui::design::asset_preview_empty_pad())
            .child(crate::ui::designs_panel::design_kind_glyph(
                kind,
                crate::ui::design::asset_preview_glyph_size(),
                accent,
            ))
            .child(
                div()
                    .text_size(crate::ui::design::text_body())
                    .text_color(crate::ui::design::t3(cx))
                    .child(caption),
            )
            .when_some(action, |column, label| {
                column.child(
                    style::secondary_button_compact("design-preview-open", label)
                        .icon(IconName::ExternalLink)
                        .on_click(move |_, _, _| open_design_reference_source(&reference)),
                )
            })
            .into_any_element()
    }

    /// Distinct folder names already used by this project's assets, for the
    /// folder picker in the edit dialog.
    fn existing_asset_folders(&self, project: ProjectId, cx: &App) -> Vec<String> {
        let mut folders: Vec<String> = self
            .designs
            .read(cx)
            .references_for_project(project)
            .iter()
            .filter_map(crate::state::designs::reference_folder)
            .collect();
        folders.sort();
        folders.dedup();
        folders
    }

    fn open_design_reference_edit_dialog(
        &mut self,
        project: ProjectId,
        reference: ProjectReference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folders = self.existing_asset_folders(project, cx);
        let editor = cx.new(|cx| DesignReferenceEditDialog::new(&reference, folders, window, cx));
        let center = cx.entity().clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let editor = editor.clone();
            let save_editor = editor.clone();
            let center = center.clone();
            let footer_reference = reference.clone();
            dialog
                .w(px(560.))
                .title(SharedString::from("Edit asset"))
                .child(editor)
                .footer(move |_, _, _, cx| {
                    let editor = save_editor.clone();
                    let center = center.clone();
                    let reference = footer_reference.clone();
                    vec![
                        style::ghost_button_compact("design-reference-edit-cancel", "Cancel")
                            .custom(style::dialog_neutral_variant(cx))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        style::primary_button_compact("design-reference-edit-save", "Save", cx)
                            .on_click(move |_, window, cx| {
                                let Some((title, source, notes, folder)) =
                                    editor.read(cx).collect(cx)
                                else {
                                    return;
                                };
                                center.update(cx, |center, cx| {
                                    if let Err(error) = center.designs.update(cx, |designs, cx| {
                                        designs.update_reference(
                                            reference.clone(),
                                            title,
                                            source,
                                            notes,
                                            folder,
                                            cx,
                                        )
                                    }) {
                                        eprintln!("failed to update design reference: {error:#}");
                                    }
                                    center.context_mode = ContextMode::Designs;
                                    center.set_view_mode(CenterMode::Docs, cx);
                                    cx.notify();
                                });
                                window.close_dialog(cx);
                            }),
                    ]
                })
        });
    }

    fn use_design_reference_in_agent(
        &mut self,
        project: ProjectId,
        reference: ProjectReference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_new_agent_composer_for_project(project, window, cx);
        let prompt = design_reference_agent_prompt(&reference);
        let preview_path = design_reference_preview_path(&reference);
        if let Some(composer) = self.new_agent_composer.as_mut() {
            composer.prompt.update(cx, |input, cx| {
                input.set_value(prompt.clone(), window, cx);
                input.set_cursor_position(
                    input_position_for_byte_offset(&prompt, prompt.len()),
                    window,
                    cx,
                );
                input.focus(window, cx);
            });
            composer.attached_files.clear();
            if let Some(path) = preview_path.filter(|path| path.is_file()) {
                composer.attached_files.push(path);
            }
            composer.error = None;
        }
        cx.notify();
    }
}

struct DesignReferenceEditDialog {
    title: Entity<InputState>,
    source: Entity<InputState>,
    notes: Entity<InputState>,
    folder: Entity<crate::ui::folder_picker::FolderPicker>,
}

impl DesignReferenceEditDialog {
    fn new(
        reference: &ProjectReference,
        folders: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let current_folder = crate::state::designs::reference_folder(reference);
        Self {
            title: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Title")
                    .default_value(reference.title.clone())
            }),
            source: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Source URL or path")
                    .default_value(reference.source.clone())
            }),
            notes: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Notes")
                    .default_value(reference.notes.clone())
            }),
            folder: cx.new(|cx| {
                crate::ui::folder_picker::FolderPicker::new(folders, current_folder, window, cx)
            }),
        }
    }

    fn collect(&self, cx: &App) -> Option<(String, String, String, Option<String>)> {
        let source = self.source.read(cx).value().trim().to_string();
        if source.is_empty() {
            return None;
        }
        let title = self.title.read(cx).value().trim().to_string();
        let title = if title.is_empty() {
            "Asset".to_string()
        } else {
            title
        };
        let notes = self.notes.read(cx).value().trim().to_string();
        let folder = self.folder.read(cx).value();
        Some((title, source, notes, folder))
    }
}

impl Render for DesignReferenceEditDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(detail_field("Title", cx).child(Input::new(&self.title)))
            .child(detail_field("Source", cx).child(Input::new(&self.source)))
            .child(detail_field("Folder", cx).child(self.folder.clone()))
            .child(detail_field("Notes", cx).child(Input::new(&self.notes)))
    }
}

/// Fallback shown inside the preview frame when an image reference fails to load.
fn design_kind_placeholder(kind: ProjectReferenceKind, accent: gpui::Hsla) -> gpui::AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .child(crate::ui::designs_panel::design_kind_glyph(
            kind,
            crate::ui::design::asset_preview_glyph_size(),
            accent,
        ))
        .into_any_element()
}

/// A labelled detail field whose value the caller supplies via `.child(...)`
/// (e.g. a kind badge or a clickable source link).
fn detail_field(label: &'static str, cx: &App) -> gpui::Div {
    v_flex().gap_1p5().child(
        div()
            .text_size(crate::ui::design::text_label())
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(crate::ui::design::t3(cx))
            .child(label),
    )
}

fn design_reference_preview_path(reference: &ProjectReference) -> Option<PathBuf> {
    let relative = reference.preview_relative_path.as_ref()?;
    LocalStore::open_default()
        .ok()
        .map(|store| store.root().join(relative))
}

fn design_reference_source_target(reference: &ProjectReference) -> String {
    let source_path = PathBuf::from(&reference.source);
    if source_path
        .components()
        .next()
        .is_some_and(|component| component.as_os_str() == "data")
    {
        if let Ok(store) = LocalStore::open_default() {
            return store.root().join(source_path).to_string_lossy().to_string();
        }
    }
    reference.source.clone()
}

pub(super) fn open_design_reference_source(reference: &ProjectReference) {
    crate::open_with::open_in(None, &design_reference_source_target(reference));
}

/// The web URL to render live in the center preview, if any. Both Figma and
/// plain URL references point at a web page (a figma.com link or an arbitrary
/// URL), so either kind is shown in the in-app webview as long as its source is
/// an `http(s)` URL. Local kinds (Pencil `.pen` files, on-disk images) have no
/// web URL and fall back to the image/placeholder preview.
pub(super) fn design_web_preview_url(reference: &ProjectReference) -> Option<String> {
    let source = reference.source.trim();
    let is_web = source.starts_with("http://") || source.starts_with("https://");
    match reference.kind {
        // Figma's raw editor URL renders its whole toolbar / sign-up chrome.
        // The embed viewer shows just the design, so it reads far cleaner.
        ProjectReferenceKind::Figma if is_web => Some(figma_embed_url(source)),
        ProjectReferenceKind::Url if is_web => Some(source.to_string()),
        _ => None,
    }
}

/// Wrap a Figma file/prototype URL in its clean embed viewer.
fn figma_embed_url(source: &str) -> String {
    format!(
        "https://www.figma.com/embed?embed_host=choro&url={}",
        percent_encode(source)
    )
}

/// Percent-encode a string for use as a query-parameter value (encodes
/// everything outside the RFC 3986 unreserved set).
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn design_reference_markdown(reference: &ProjectReference) -> String {
    let mut markdown = format!(
        "### Asset: {}\n\n- Type: {}\n- Source: {}\n",
        reference.title,
        design_reference_kind_label(reference.kind),
        reference.source
    );
    if !reference.notes.trim().is_empty() {
        markdown.push_str(&format!("- Notes: {}\n", reference.notes));
    }
    if let Some(path) = design_reference_preview_path(reference) {
        markdown.push_str(&format!(
            "\n![{}]({})\n",
            reference.title,
            path.to_string_lossy()
        ));
    }
    markdown
}

fn design_reference_agent_prompt(reference: &ProjectReference) -> String {
    let mut prompt = format!(
        "Use this asset as project context.\n\nTitle: {}\nType: {}\nSource: {}\n",
        reference.title,
        design_reference_kind_label(reference.kind),
        reference.source
    );
    if !reference.notes.trim().is_empty() {
        prompt.push_str(&format!("Notes: {}\n", reference.notes));
    }
    match reference.kind {
        ProjectReferenceKind::Figma => {
            prompt.push_str(
                "If Figma tooling is available, use the source link for deeper structure.\n",
            );
        }
        ProjectReferenceKind::Pencil => {
            prompt.push_str(
                "If Pencil MCP is available, use the .pen source for deeper structure.\n",
            );
        }
        ProjectReferenceKind::File => {
            prompt.push_str("The file is attached in the project's assets.\n");
        }
        ProjectReferenceKind::Image | ProjectReferenceKind::Url => {}
    }
    prompt
}

fn design_reference_kind_label(kind: ProjectReferenceKind) -> &'static str {
    match kind {
        ProjectReferenceKind::Image => "Image",
        ProjectReferenceKind::Figma => "Figma",
        ProjectReferenceKind::Pencil => "Pencil",
        ProjectReferenceKind::Url => "URL",
        ProjectReferenceKind::File => "File",
    }
}
