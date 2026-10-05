//! Reversible deletion keeps documents and comments together, under the project lock.
use super::*;

impl StudioStore {
    fn lifecycle_manifest(&self, root: &str, id: Uuid) -> Result<StudioDesignManifest> {
        let manifest: StudioDesignManifest =
            serde_json::from_slice(&self.read_file(&format!("{root}/{id}/design.json"))?)?;
        ensure!(
            manifest.id == id
                && manifest.schema_version == SCHEMA_VERSION
                && !manifest.system_workspace,
            "Unsupported Studio design"
        );
        Ok(manifest)
    }

    pub fn deleted_designs(&self) -> Result<Vec<StudioDesignManifest>> {
        let _lock = self.lock()?;
        self.recover()?;
        let root = format!("{DESIGNS_DIR}/.trash");
        let path = self.path(&root)?;
        if !path.exists() {
            return Ok(vec![]);
        }
        let mut designs = vec![];
        for entry in fs::read_dir(path)? {
            if let Ok(id) = entry?.file_name().to_string_lossy().parse::<Uuid>() {
                designs.push(self.lifecycle_manifest(&root, id)?);
            }
        }
        designs.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        Ok(designs)
    }

    /// Move one confirmed revision to Trash without removing any files or shared systems.
    pub fn delete_design(&self, id: Uuid, expected_revision: u64) -> Result<()> {
        let _lock = self.lock()?;
        self.recover()?;
        let manifest = self.lifecycle_manifest(DESIGNS_DIR, id)?;
        ensure!(
            manifest.revision == expected_revision,
            "Design changed. Refresh and try deleting it again."
        );
        ensure!(
            self.active_scopes(id).is_empty(),
            "Stop the design assistant before deleting this design."
        );
        let source = self.path(&format!("{DESIGNS_DIR}/{id}"))?;
        let target = self.path(&format!("{DESIGNS_DIR}/.trash/{id}"))?;
        ensure!(!target.exists(), "This design already has a copy in Trash");
        fs::create_dir_all(target.parent().context("Missing Trash directory")?)?;
        fs::rename(source, target).context("Could not move design to Trash")
    }

    pub fn restore_design(&self, id: Uuid) -> Result<()> {
        let _lock = self.lock()?;
        self.recover()?;
        self.lifecycle_manifest(&format!("{DESIGNS_DIR}/.trash"), id)?;
        let source = self.path(&format!("{DESIGNS_DIR}/.trash/{id}"))?;
        let target = self.path(&format!("{DESIGNS_DIR}/{id}"))?;
        ensure!(
            !target.exists(),
            "A design with this identity already exists. Nothing was overwritten."
        );
        fs::rename(source, target).context("Could not restore design")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, StudioStore, StudioDesign) {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        fs::create_dir(&project).unwrap();
        let store = StudioStore::new(project, dir.path().join("data")).unwrap();
        let design = store.create("Lifecycle").unwrap();
        (dir, store, design)
    }

    fn edit(design: &StudioDesign, scope: &StudioTurnScope) -> StudioTransaction {
        StudioTransaction {
            id: Uuid::new_v4(),
            scope_id: scope.id,
            design_id: design.manifest.id,
            expected_revision: design.manifest.revision,
            expected_fingerprint: design.fingerprint.clone(),
            operations: vec![StudioOperation::RenameDesign {
                name: "Updated design".into(),
            }],
        }
    }

    #[test]
    fn trash_survives_reopen_and_restores_comments_documents_and_undo() {
        let (_dir, store, design) = fixture();
        let other = store.create("Keep me").unwrap();
        let id = design.manifest.id;
        let conversations = store.conversations(id).unwrap();
        let comments = store
            .apply_comment(
                id,
                0,
                &StudioCommentOperation::Create {
                    id: Uuid::new_v4(),
                    screen_id: design.manifest.screens[0].id,
                    x: 0.5,
                    y: 0.5,
                    body: "Keep this comment".into(),
                },
            )
            .unwrap();
        let scope = StudioTurnScope::whole_design(&design);
        let tx = edit(&design, &scope);
        let changed = store.apply(&scope, &tx).unwrap();
        store.delete_design(id, changed.manifest.revision).unwrap();
        let reopened = StudioStore::new(
            &store.project,
            store.cache.parent().unwrap().parent().unwrap(),
        )
        .unwrap();
        assert_eq!(reopened.list().unwrap(), vec![other.manifest.clone()]);
        assert_eq!(
            reopened.deleted_designs().unwrap(),
            vec![changed.manifest.clone()]
        );
        assert!(reopened.load(id).is_err());
        assert!(
            reopened.apply(&scope, &tx).is_err(),
            "A completed transaction retry must not succeed for a deleted design"
        );
        assert!(reopened.apply(&scope, &edit(&changed, &scope)).is_err());
        assert!(reopened
            .apply_comment(
                id,
                1,
                &StudioCommentOperation::Resolve {
                    id: comments.pins[0].id
                }
            )
            .is_err());
        assert_eq!(reopened.load(other.manifest.id).unwrap(), other);
        reopened.restore_design(id).unwrap();
        assert_eq!(reopened.load(id).unwrap(), changed);
        assert_eq!(reopened.comments(id).unwrap(), comments);
        assert_eq!(reopened.conversations(id).unwrap(), conversations);
        assert!(reopened.deleted_designs().unwrap().is_empty());
        assert_eq!(
            reopened.undo_latest(id).unwrap().manifest.name,
            design.manifest.name
        );
    }

    #[test]
    fn stale_confirmation_and_active_assistant_preserve_the_design() {
        let (_dir, store, design) = fixture();
        let scope = StudioTurnScope::whole_design(&design);
        let changed = store.apply(&scope, &edit(&design, &scope)).unwrap();
        assert!(store
            .delete_design(design.manifest.id, design.manifest.revision)
            .is_err());
        let agent = Uuid::new_v4();
        store.save_scope(agent, &scope).unwrap();
        assert!(store
            .delete_design(design.manifest.id, changed.manifest.revision)
            .is_err());
        assert_eq!(store.load(design.manifest.id).unwrap(), changed);
        assert!(store.deleted_designs().unwrap().is_empty());
        let mut ended = scope;
        ended.active = false;
        store.save_scope(agent, &ended).unwrap();
        store
            .delete_design(design.manifest.id, changed.manifest.revision)
            .unwrap();
    }

    #[test]
    fn restore_never_overwrites_an_existing_design() {
        let (_dir, store, design) = fixture();
        let id = design.manifest.id;
        store.delete_design(id, 0).unwrap();
        let collision = store.project.join(format!("{DESIGNS_DIR}/{id}"));
        fs::create_dir(&collision).unwrap();
        fs::write(collision.join("keep.txt"), "Existing work").unwrap();
        assert!(store.restore_design(id).is_err());
        assert_eq!(
            fs::read_to_string(collision.join("keep.txt")).unwrap(),
            "Existing work"
        );
        assert_eq!(store.deleted_designs().unwrap(), vec![design.manifest]);
    }

    #[test]
    fn deletion_preserves_shared_systems_and_immutable_handoffs() {
        let (_dir, store, design) = fixture();
        let system = store.create_system("Shared library", "Web", None).unwrap();
        let system_path = store.project.join(format!(
            "{DESIGNS_DIR}/design-systems/{}/system.json",
            system.id
        ));
        let system_bytes = fs::read(&system_path).unwrap();
        let handoff = store.handoff(design.manifest.id, None).unwrap();
        let assets = store
            .project
            .join(format!("{DESIGNS_DIR}/{}/assets", design.manifest.id));
        fs::create_dir_all(&assets).unwrap();
        fs::write(
            assets.join("local.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        )
        .unwrap();
        store
            .delete_design(design.manifest.id, design.manifest.revision)
            .unwrap();
        assert_eq!(fs::read(&system_path).unwrap(), system_bytes);
        assert_eq!(
            serde_json::to_value(store.read_handoff(handoff.id).unwrap()).unwrap(),
            serde_json::to_value(handoff).unwrap(),
        );
        assert!(store.delete_design(system.id, 0).is_err());
        store.restore_design(design.manifest.id).unwrap();
        assert_eq!(
            fs::read_to_string(assets.join("local.svg")).unwrap(),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_trash_cannot_move_files_outside_the_project() {
        let (dir, store, design) = fixture();
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(
            &outside,
            store.project.join(format!("{DESIGNS_DIR}/.trash")),
        )
        .unwrap();
        assert!(store.delete_design(design.manifest.id, 0).is_err());
        assert_eq!(store.load(design.manifest.id).unwrap(), design);
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    }
}
