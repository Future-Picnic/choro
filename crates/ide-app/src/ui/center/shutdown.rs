use super::*;

impl CenterArea {
    /// Flush editor-backed state that otherwise relies on short debounce windows.
    pub(crate) fn save_for_shutdown(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        self.flush_personal_editor(cx);
        self.docs.update(cx, |docs, cx| docs.save_all_now(cx))?;

        for item in self.editors.iter_mut().filter(|item| item.dirty) {
            if Self::file_changed_on_disk(&item.path, item.modified_at) {
                anyhow::bail!(
                    "{} changed on disk; review it before quitting",
                    item.path.display()
                );
            }
            let content = item.input.read(cx).value().to_string();
            std::fs::write(&item.path, content.as_bytes())?;
            item.modified_at = std::fs::metadata(&item.path)
                .and_then(|metadata| metadata.modified())
                .ok();
            item.dirty = false;
            item.saving = false;
        }

        Ok(())
    }
}
