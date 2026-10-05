//! Screen comments are a separate sidecar, outside design revisions, exports,
//! and preview fingerprints. Read only when the user enters Comments mode.
use super::*;
use std::io::Read;

pub const MAX_COMMENT_BODY: usize = 8000;
const MAX_THREADS: usize = 500;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StudioCommentPin {
    pub id: Uuid,
    pub screen_id: Uuid,
    /// Fractions of screen size, independent of canvas position and zoom.
    pub x: f64,
    pub y: f64,
    pub resolved: bool,
    pub body: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StudioComments {
    pub schema_version: u32,
    pub revision: u64,
    pub pins: Vec<StudioCommentPin>,
}
impl Default for StudioComments {
    fn default() -> Self {
        Self { schema_version: 1, revision: 0, pins: vec![] }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StudioCommentOperation {
    Create { id: Uuid, screen_id: Uuid, x: f64, y: f64, body: String },
    Resolve { id: Uuid },
}
fn valid_body(body: &str) -> Result<()> {
    ensure!(!body.trim().is_empty() && body.len() <= MAX_COMMENT_BODY,
        "Comments must contain text and be at most 8000 bytes");
    Ok(())
}
impl StudioCommentOperation {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Create { id, screen_id, x, y, body } => {
                ensure!(!id.is_nil() && !screen_id.is_nil(), "Invalid comment identity");
                ensure!(x.is_finite() && y.is_finite() && (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y), "Comment must be on the screen");
                valid_body(body)
            }
            Self::Resolve { id } => {
                ensure!(!id.is_nil(), "Invalid comment identity");
                Ok(())
            }
        }
    }
}
impl StudioComments {
    fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "Unsupported comments version");
        ensure!(self.pins.len() <= MAX_THREADS, "Too many comments");
        let mut ids = BTreeSet::new();
        for pin in &self.pins {
            ensure!(ids.insert(pin.id), "Duplicate comment identity");
            StudioCommentOperation::Create {
                id: pin.id, screen_id: pin.screen_id, x: pin.x, y: pin.y,
                body: pin.body.clone(),
            }.validate()?;
        }
        Ok(())
    }
}
impl StudioStore {
    fn comments_path(&self, id: Uuid) -> Result<PathBuf> {
        self.path(&format!("{DESIGNS_DIR}/{id}/comments.json"))
    }
    pub fn comments(&self, id: Uuid) -> Result<StudioComments> {
        let path = self.comments_path(id)?;
        match fs::File::open(path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(MAX_FILE as u64 + 1).read_to_end(&mut bytes)?;
                ensure!(bytes.len() <= MAX_FILE, "Comments file is too large");
                let comments: StudioComments = serde_json::from_slice(&bytes).context("Could not read screen comments")?;
                comments.validate()?;
                Ok(comments)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
            Err(error) => Err(error.into()),
        }
    }
    pub fn apply_comment(&self, design_id: Uuid, revision: u64, operation: &StudioCommentOperation) -> Result<StudioComments> {
        operation.validate()?;
        let _lock = self.lock()?;
        // Validate current screen membership under the same lock as mutations.
        let manifest: StudioDesignManifest = serde_json::from_slice(&self.read_file(&format!("{DESIGNS_DIR}/{design_id}/design.json"))?)?;
        ensure!(manifest.id == design_id && manifest.schema_version == SCHEMA_VERSION, "Unsupported Studio manifest");
        let mut state = self.comments(design_id)?;
        ensure!(state.revision == revision, "Comments changed. Review the latest comments and try again.");
        let active = |screen| manifest.screens.iter().any(|s| s.id == screen && !s.archived);
        match operation {
            StudioCommentOperation::Create { id, screen_id, x, y, body } => {
                ensure!(active(*screen_id), "This screen is no longer available");
                ensure!(state.pins.len() < MAX_THREADS, "This design has reached 500 comments");
                state.pins.push(StudioCommentPin {
                    id: *id, screen_id: *screen_id, x: *x, y: *y, resolved: false,
                    body: body.trim().into(), created_at: now(),
                });
            }
            StudioCommentOperation::Resolve { id } => {
                let pin = state.pins.iter_mut().find(|pin| pin.id == *id).context("Comment no longer exists")?;
                ensure!(active(pin.screen_id), "This screen is no longer available");
                pin.resolved = true;
            }
        }
        state.revision = state.revision.checked_add(1).context("Comment revision overflow")?;
        state.validate()?;
        let bytes = serde_json::to_vec_pretty(&state)?;
        ensure!(bytes.len() <= MAX_FILE, "Comments file is too large");
        atomic(&self.comments_path(design_id)?, &bytes)?;
        Ok(state)
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
        let design = store.create("Comments").unwrap();
        (dir, store, design)
    }
    #[test]
    fn comments_survive_reload_without_changing_design_or_preview_keys() {
        let (_dir, store, design) = fixture();
        let screen_id = design.manifest.screens[0].id;
        let key = store.thumbnail_path(&design, screen_id);
        let id = Uuid::new_v4();
        let state = store.apply_comment(design.manifest.id, 0, &StudioCommentOperation::Create {
            id, screen_id, x: 0.25, y: 0.75, body: "  Make this clearer  ".into(),
        }).unwrap();
        assert_eq!(store.comments(design.manifest.id).unwrap(), state);
        assert_eq!(store.load(design.manifest.id).unwrap(), design);
        assert_eq!(store.thumbnail_path(&store.load(design.manifest.id).unwrap(), screen_id), key);
        assert_eq!(state.pins[0].body, "Make this clearer");
        let state = store.apply_comment(design.manifest.id, 1, &StudioCommentOperation::Resolve { id }).unwrap();
        assert!(state.pins[0].resolved);
        assert_eq!(store.comments(design.manifest.id).unwrap(), state);
        assert_eq!(store.load(design.manifest.id).unwrap(), design);
        assert_eq!(store.thumbnail_path(&design, screen_id), key);
    }
    #[test]
    fn invalid_and_stale_writes_preserve_saved_comments() {
        let (_dir, store, design) = fixture();
        let create = StudioCommentOperation::Create { id: Uuid::new_v4(), screen_id: design.manifest.screens[0].id, x: 0.5, y: 0.5, body: "Feedback".into() };
        let saved = store.apply_comment(design.manifest.id, 0, &create).unwrap();
        assert!(store.apply_comment(design.manifest.id, 0, &create).is_err());
        for (x, body) in [(f64::NAN, "Text".into()), (1.1, "Text".into()), (0.5, " ".into()), (0.5, "x".repeat(MAX_COMMENT_BODY + 1))] {
            let invalid = StudioCommentOperation::Create { id: Uuid::new_v4(), screen_id: design.manifest.screens[0].id, x, y: 0.5, body };
            assert!(store.apply_comment(design.manifest.id, 1, &invalid).is_err());
        }
        let unknown = StudioCommentOperation::Create { id: Uuid::new_v4(), screen_id: Uuid::new_v4(), x: 0.5, y: 0.5, body: "Text".into() };
        assert!(store.apply_comment(design.manifest.id, 1, &unknown).is_err());
        assert_eq!(store.comments(design.manifest.id).unwrap(), saved);
    }
    #[test]
    fn corrupt_or_future_comments_are_never_overwritten() {
        let (_dir, store, design) = fixture();
        let path = store.comments_path(design.manifest.id).unwrap();
        for bytes in [b"invalid".as_slice(), br#"{"schema_version":2,"revision":0,"pins":[]}"#] {
            fs::write(&path, bytes).unwrap();
            assert!(store.apply_comment(design.manifest.id, 0, &StudioCommentOperation::Create { id: Uuid::new_v4(), screen_id: design.manifest.screens[0].id, x: 0.5, y: 0.5, body: "Text".into() }).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}
