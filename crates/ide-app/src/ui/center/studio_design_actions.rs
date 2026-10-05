use super::*;

impl CenterArea {
    pub(super) fn confirm_delete_studio_design(
        &mut self,
        project: ProjectId,
        id: Uuid,
        revision: u64,
        title: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let center = cx.entity().clone();
        crate::ui::confirm::ConfirmDialog::new(
            "Delete design?",
            "This design and its screens will move to Trash. You can restore them, including comments, from the Designs page.",
        )
        .icon(IconName::Delete)
        .detail(title)
        .confirm_label("Delete")
        .confirm_id("delete-studio-design-confirm")
        .on_confirm(move |_, cx| {
            center.update(cx, |this, cx| this.change_studio_design_lifecycle(project, id, Some(revision), cx));
        })
        .open(window, cx);
    }

    pub(super) fn studio_trash_menu(
        &self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let designs = self
            .studio_trash_catalog
            .get(&project)
            .cloned()
            .unwrap_or_default();
        let center = cx.entity().clone();
        style::ghost_button_compact("design-hub-trash", "Trash")
            .icon(IconName::Delete)
            .disabled(designs.is_empty() || self.studio_design_mutations.contains(&project))
            .dropdown_menu(move |mut menu, window, _| {
                for design in &designs {
                    let id = design.id;
                    menu = menu.item(
                        PopupMenuItem::new(format!("Restore {}", design.name)).on_click(
                            window.listener_for(&center, move |this: &mut CenterArea, _, _, cx| {
                                this.change_studio_design_lifecycle(project, id, None, cx);
                            }),
                        ),
                    );
                }
                menu
            })
    }

    /// A revision means confirmed deletion; None restores the existing identity.
    fn change_studio_design_lifecycle(
        &mut self,
        project: ProjectId,
        id: Uuid,
        revision: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let Some((_, root)) = self.project_by_id(project, cx) else {
            return;
        };
        if self
            .studio
            .as_ref()
            .is_some_and(|s| s.project == project && s.design.manifest.id == id)
        {
            self.design_hub_error =
                Some("Return to the Designs page before deleting this design.".into());
            cx.notify();
            return;
        }
        if !self.studio_design_mutations.insert(project) {
            return;
        }
        *self.studio_catalog_generation.entry(project).or_default() += 1;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = ide_core::studio::StudioStore::for_project(root)?;
                    match revision {
                        Some(revision) => store.delete_design(id, revision),
                        None => store.restore_design(id),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.studio_design_mutations.remove(&project);
                *this.studio_catalog_generation.entry(project).or_default() += 1;
                match result {
                    Ok(()) => {
                        this.design_hub_error = None;
                        if revision.is_some() {
                            if let Some(designs) = this.studio_catalog.get_mut(&project) {
                                if let Some(index) =
                                    designs.iter().position(|design| design.id == id)
                                {
                                    let design = designs.remove(index);
                                    this.studio_trash_catalog
                                        .entry(project)
                                        .or_default()
                                        .push(design);
                                }
                            }
                            this.pending_studio_drafts.remove(&id);
                        } else if let Some(designs) = this.studio_trash_catalog.get_mut(&project) {
                            if let Some(index) = designs.iter().position(|design| design.id == id) {
                                let design = designs.remove(index);
                                this.studio_catalog.entry(project).or_default().push(design);
                            }
                        }
                    }
                    Err(error) => {
                        this.design_hub_error = Some(format!(
                            "Could not {} design: {error:#}",
                            if revision.is_some() {
                                "delete"
                            } else {
                                "restore"
                            }
                        ))
                    }
                }
                this.refresh_studio_catalog(project, cx);
                cx.notify();
            });
        })
        .detach();
    }
}
