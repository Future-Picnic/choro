use super::*;

impl SettingsView {
    pub(super) fn render_data_page(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .w_full()
            .gap_2()
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .p_4()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line_2(cx))
                    .bg(crate::ui::design::surface(cx).opacity(0.55))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(crate::ui::design::text_body())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Export or restore the local workspace archive."),
                            )
                            .when(self.data_busy, |row| row.child(Spinner::new().xsmall()))
                            .child(
                                crate::ui::style::settings_action_button(
                                    "settings-export-workspace",
                                    "Export",
                                )
                                .disabled(self.data_busy)
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.export_workspace(cx);
                                    },
                                )),
                            )
                            .child(
                                crate::ui::style::settings_action_button(
                                    "settings-import-workspace",
                                    "Import",
                                )
                                .disabled(self.data_busy)
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.import_workspace(cx);
                                    },
                                )),
                            ),
                    )
                    .when_some(self.data_status.clone(), |card, status| {
                        card.child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .child(SharedString::from(status)),
                        )
                    }),
            )
            .into_any_element()
    }
}
impl SettingsView {
    fn export_workspace(&mut self, cx: &mut Context<Self>) {
        if self.data_busy {
            return;
        }
        self.data_busy = true;
        self.data_status = Some("Exporting workspace...".into());
        let target = default_export_path();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn({
                    let target = target.clone();
                    async move {
                        LocalStore::open_default()
                            .and_then(|store| store.export_workspace(&target))
                            .map(|_| target)
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.data_busy = false;
                this.data_status = Some(match result {
                    Ok(path) => format!("Exported {}", path.display()),
                    Err(error) => format!("Export failed: {error:#}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn import_workspace(&mut self, cx: &mut Context<Self>) {
        if self.data_busy {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        cx.spawn(async move |this, cx| {
            let selected = receiver.await;
            let archive = match selected {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(archive) = archive else {
                return;
            };
            this.update(cx, |this, cx| {
                this.data_busy = true;
                this.data_status = Some("Importing workspace...".into());
                cx.notify();
            })
            .ok();
            let result = cx
                .background_executor()
                .spawn(async move {
                    LocalStore::open_default()
                        .and_then(|store| store.import_workspace_replace(&archive))
                })
                .await;
            this.update(cx, |this, cx| {
                this.data_busy = false;
                this.data_status = Some(match result {
                    Ok(backup) => format!(
                        "Imported workspace. Backup: {}. Restart the app to reload all views.",
                        backup.display()
                    ),
                    Err(error) => format!("Import failed: {error:#}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

fn default_export_path() -> PathBuf {
    let dir = dirs::download_dir()
        .or_else(dirs::desktop_dir)
        .unwrap_or_else(|| {
            LocalStore::open_default()
                .map(|store| store.root().to_path_buf())
                .unwrap_or_else(|_| PathBuf::from("."))
        });
    dir.join(format!("choro-workspace-export-{}.zip", unix_now_secs()))
}
