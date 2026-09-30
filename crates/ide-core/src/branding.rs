use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "Choro";
pub const APP_ID: &str = "com.ritmus.choro";
pub const LEGACY_APP_ID: &str = "com.ritmus.myide";
pub const DOCS_DIR_NAME: &str = "choro_docs";
pub const LEGACY_DOCS_DIR_NAME: &str = "my_ide_docs";

/// Converts a repository-relative path from the former docs directory name.
/// Paths outside that exact top-level directory are returned unchanged.
pub fn migrate_legacy_doc_path(path: &Path) -> PathBuf {
    path.strip_prefix(LEGACY_DOCS_DIR_NAME)
        .map(|suffix| Path::new(DOCS_DIR_NAME).join(suffix))
        .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_only_the_legacy_top_level_docs_directory() {
        assert_eq!(
            migrate_legacy_doc_path(Path::new("my_ide_docs/features/spec.md")),
            PathBuf::from("choro_docs/features/spec.md")
        );
        assert_eq!(
            migrate_legacy_doc_path(Path::new("notes/my_ide_docs/spec.md")),
            PathBuf::from("notes/my_ide_docs/spec.md")
        );
        assert_eq!(
            migrate_legacy_doc_path(Path::new("choro_docs/spec.md")),
            PathBuf::from("choro_docs/spec.md")
        );
    }
}
