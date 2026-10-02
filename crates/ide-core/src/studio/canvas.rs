//! Personal canvas layout. Screen contents/dimensions remain in revisioned design files.
use super::*;
use anyhow::{ensure, Context, Result};
use std::fs;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StudioOverviewMode {
    #[default]
    Canvas,
    Grid,
    Focus,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct StudioCanvasPoint {
    pub x: f64,
    pub y: f64,
}
impl StudioCanvasPoint {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.x.is_finite() && self.y.is_finite() && self.x.abs() <= 1e7 && self.y.abs() <= 1e7,
            "Invalid canvas coordinates"
        );
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct StudioCanvasViewport {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}
impl Default for StudioCanvasViewport {
    fn default() -> Self {
        Self {
            x: 30.,
            y: 50.,
            zoom: 0.25,
        }
    }
}
impl StudioCanvasViewport {
    pub fn validate(&self) -> Result<()> {
        StudioCanvasPoint {
            x: self.x,
            y: self.y,
        }
        .validate()?;
        ensure!(
            self.zoom.is_finite() && (0.02..=2.).contains(&self.zoom),
            "Invalid canvas zoom"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StudioCanvasState {
    pub schema_version: u32,
    pub overview_mode: StudioOverviewMode,
    pub viewport: StudioCanvasViewport,
    /// Free positions. Grouped screens keep their old entry, but the section
    /// board's derived geometry overrides it while they are grouped.
    pub positions: BTreeMap<Uuid, StudioCanvasPoint>,
    pub selected_screen_id: Option<Uuid>,
    /// Exclusive with `selected_screen_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_section_id: Option<Uuid>,
    /// Personal fallback board origin, frozen once, for designs whose sections
    /// were created without a saved origin (for example by an agent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_origin: Option<StudioSectionOrigin>,
}
impl Default for StudioCanvasState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            overview_mode: StudioOverviewMode::Canvas,
            viewport: Default::default(),
            positions: Default::default(),
            selected_screen_id: None,
            selected_section_id: None,
            section_origin: None,
        }
    }
}
impl StudioCanvasState {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "Unsupported canvas layout version"
        );
        ensure!(self.positions.len() <= 200, "Too many canvas positions");
        self.viewport.validate()?;
        for point in self.positions.values() {
            point.validate()?;
        }
        if let Some(origin) = &self.section_origin {
            ensure!(
                origin.x.abs() <= MAX_BOARD_COORDINATE && origin.y.abs() <= MAX_BOARD_COORDINATE,
                "Invalid section board origin"
            );
        }
        Ok(())
    }
    /// The saved design origin, else this person's frozen fallback, else a
    /// position below the existing unsectioned screens.
    pub fn board_origin(&self, manifest: &StudioDesignManifest) -> StudioSectionOrigin {
        manifest
            .section_layout
            .origin
            .or(self.section_origin)
            .unwrap_or_else(|| self.default_board_origin(manifest))
    }
    /// Below the visible unsectioned screens, aligned with their left edge.
    pub fn default_board_origin(&self, manifest: &StudioDesignManifest) -> StudioSectionOrigin {
        let free: Vec<_> = manifest
            .screens
            .iter()
            .filter(|s| !s.archived && manifest.section_of(s.id).is_none())
            .filter_map(|s| self.positions.get(&s.id).map(|p| (s, p)))
            .collect();
        if free.is_empty() {
            return StudioSectionOrigin::default();
        }
        let left = free.iter().map(|(_, p)| p.x).fold(f64::INFINITY, f64::min);
        let bottom = free
            .iter()
            .map(|(s, p)| p.y + s.height as f64)
            .fold(f64::NEG_INFINITY, f64::max);
        StudioSectionOrigin {
            x: (left.round() as i64).clamp(-MAX_BOARD_COORDINATE, MAX_BOARD_COORDINATE),
            y: ((bottom + SECTION_SPACING as f64).round() as i64)
                .clamp(-MAX_BOARD_COORDINATE, MAX_BOARD_COORDINATE),
        }
    }
    pub fn board(&self, manifest: &StudioDesignManifest) -> StudioBoardGeometry {
        board_geometry(manifest, self.board_origin(manifest))
    }
    /// Free positions with grouped screens' authoritative board positions.
    pub fn effective_positions(
        &self,
        board: &StudioBoardGeometry,
    ) -> BTreeMap<Uuid, StudioCanvasPoint> {
        let mut positions = self.positions.clone();
        positions.extend(board.positions.iter().map(|(id, p)| (*id, *p)));
        positions
    }
    /// Lowest edge of the board and visible unsectioned screens.
    fn content_bottom(&self, manifest: &StudioDesignManifest, board: &StudioBoardGeometry) -> f64 {
        let free = manifest
            .screens
            .iter()
            .filter(|s| !s.archived && manifest.section_of(s.id).is_none())
            .filter_map(|s| self.positions.get(&s.id).map(|p| p.y + s.height as f64));
        let board = board.bounds().map(|(_, y, _, h)| (y + h) as f64);
        free.chain(board).fold(f64::NEG_INFINITY, f64::max)
    }
    /// Newly unsectioned screens leave the board area. A remembered free
    /// position is kept when it no longer overlaps any section.
    pub fn place_outside_board(&mut self, manifest: &StudioDesignManifest, screens: &[Uuid]) {
        let board = self.board(manifest);
        let overlaps = |p: &StudioCanvasPoint, s: &StudioScreen| {
            board.sections.iter().any(|b| {
                p.x < (b.x + b.width) as f64
                    && p.x + s.width as f64 > b.x as f64
                    && p.y < (b.y + b.height) as f64
                    && p.y + s.height as f64 > b.y as f64
            })
        };
        let moving: Vec<&StudioScreen> = screens
            .iter()
            .filter_map(|id| manifest.screens.iter().find(|s| s.id == *id))
            .filter(|s| self.positions.get(&s.id).is_none_or(|p| overlaps(p, s)))
            .collect();
        if moving.is_empty() {
            return;
        }
        for screen in &moving {
            self.positions.remove(&screen.id);
        }
        let left = board
            .bounds()
            .map(|(x, ..)| x as f64)
            .unwrap_or_default();
        let mut y = self.content_bottom(manifest, &board).max(0.) + SECTION_SPACING as f64;
        for row in moving.chunks(4) {
            let mut x = left;
            let mut height: f64 = 0.;
            for screen in row {
                self.positions.insert(screen.id, StudioCanvasPoint { x, y });
                x += screen.width as f64 + 120.;
                height = height.max(screen.height as f64);
            }
            y += height + 120.;
        }
    }
    /// Section-aware reconcile: retain every remembered position, clear stale
    /// selections, freeze a fallback board origin, and place missing
    /// unsectioned screens below both the free screens and the board.
    pub fn reconcile_design(&mut self, manifest: &StudioDesignManifest) {
        if self
            .selected_section_id
            .is_some_and(|id| manifest.section(id).is_none())
        {
            self.selected_section_id = None;
        }
        if self.selected_section_id.is_some() && self.selected_screen_id.is_some() {
            self.selected_section_id = None;
        }
        if manifest.sections.is_empty() {
            self.section_origin = None;
            self.reconcile(&manifest.screens);
            return;
        }
        let grouped: BTreeSet<Uuid> = manifest
            .sections
            .iter()
            .flat_map(|s| s.screen_ids.iter().copied())
            .collect();
        self.positions
            .retain(|id, _| manifest.screens.iter().any(|s| s.id == *id));
        if manifest.section_layout.origin.is_none() && self.section_origin.is_none() {
            self.section_origin = Some(self.default_board_origin(manifest));
        }
        let missing: Vec<Uuid> = manifest
            .screens
            .iter()
            .filter(|s| !grouped.contains(&s.id) && !self.positions.contains_key(&s.id))
            .map(|s| s.id)
            .collect();
        self.place_outside_board(manifest, &missing);
        // Selection rules shared with the section-free path.
        let screens = manifest.screens.clone();
        if self
            .selected_screen_id
            .is_some_and(|id| !screens.iter().any(|s| s.id == id && !s.archived))
        {
            self.selected_screen_id = None;
        }
        if self.overview_mode == StudioOverviewMode::Focus && self.selected_screen_id.is_none() {
            self.selected_screen_id = screens.iter().find(|s| !s.archived).map(|s| s.id);
            self.selected_section_id = None;
        }
    }
    pub fn select_section(&mut self, section: Option<Uuid>) {
        self.selected_section_id = section;
        if section.is_some() {
            self.selected_screen_id = None;
        }
    }
    pub fn select_screen(&mut self, screen: Option<Uuid>) {
        self.selected_screen_id = screen;
        if screen.is_some() {
            self.selected_section_id = None;
        }
    }
    /// Append missing screens below existing artboards; retain archived positions.
    pub fn reconcile(&mut self, screens: &[StudioScreen]) {
        self.positions
            .retain(|id, _| screens.iter().any(|s| s.id == *id));
        if self
            .selected_screen_id
            .is_some_and(|id| !screens.iter().any(|s| s.id == id && !s.archived))
        {
            self.selected_screen_id = None;
        }
        if self.overview_mode == StudioOverviewMode::Focus && self.selected_screen_id.is_none() {
            self.selected_screen_id = screens.iter().find(|s| !s.archived).map(|s| s.id);
        }
        let mut y = screens
            .iter()
            .filter_map(|s| {
                self.positions
                    .get(&s.id)
                    .map(|p| p.y + s.height as f64 + 120.)
            })
            .fold(0., f64::max);
        let missing: Vec<_> = screens
            .iter()
            .filter(|s| !self.positions.contains_key(&s.id))
            .collect();
        for row in missing.chunks(4) {
            let mut x = 0.;
            let mut height: f64 = 0.;
            for screen in row {
                self.positions.insert(screen.id, StudioCanvasPoint { x, y });
                x += screen.width as f64 + 120.;
                height = height.max(screen.height as f64);
            }
            y += height + 120.;
        }
    }
    pub fn arrange(&mut self, screens: &[StudioScreen]) {
        // Archived artboards retain their previous personal placement.
        self.positions
            .retain(|id, _| screens.iter().any(|s| s.id == *id && s.archived));
        let archived = std::mem::take(&mut self.positions);
        self.reconcile(
            &screens
                .iter()
                .filter(|s| !s.archived)
                .cloned()
                .collect::<Vec<_>>(),
        );
        self.positions.extend(archived);
    }
    /// Arrange re-flows visible unsectioned screens; sections lay themselves out.
    pub fn arrange_design(&mut self, manifest: &StudioDesignManifest) {
        if manifest.sections.is_empty() {
            self.arrange(&manifest.screens);
            return;
        }
        self.positions.retain(|id, _| {
            manifest
                .screens
                .iter()
                .any(|s| s.id == *id && (s.archived || manifest.section_of(s.id).is_some()))
        });
        let free: Vec<_> = manifest
            .screens
            .iter()
            .filter(|s| !s.archived && manifest.section_of(s.id).is_none())
            .cloned()
            .collect();
        let mut above = Self {
            positions: Default::default(),
            ..self.clone()
        };
        above.reconcile(&free);
        // Free screens go above the board's origin so the board never moves.
        let origin = self.board_origin(manifest);
        let height = free
            .iter()
            .filter_map(|s| above.positions.get(&s.id).map(|p| p.y + s.height as f64))
            .fold(0., f64::max);
        for (id, p) in above.positions {
            self.positions.insert(
                id,
                StudioCanvasPoint {
                    x: p.x + origin.x as f64,
                    y: p.y + origin.y as f64 - height - SECTION_SPACING as f64,
                },
            );
        }
        self.reconcile_design(manifest);
    }
}
impl StudioStore {
    pub fn canvas_state(&self, id: Uuid) -> Result<StudioCanvasState> {
        use std::io::Read;
        let path = self.cache.join(format!("canvas-{id}.json"));
        match fs::File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(128 * 1024 + 1).read_to_end(&mut bytes)?;
                ensure!(bytes.len() <= 128 * 1024, "Canvas layout is too large");
                let value: StudioCanvasState =
                    serde_json::from_slice(&bytes).context("Could not read saved canvas layout")?;
                value.validate()?;
                Ok(value)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
            Err(error) => Err(error.into()),
        }
    }
    pub fn save_canvas_state(&self, id: Uuid, state: &StudioCanvasState) -> Result<()> {
        state.validate()?;
        // Do not silently overwrite a corrupt or future-version layout.
        self.canvas_state(id)?;
        atomic(
            &self.cache.join(format!("canvas-{id}.json")),
            &serde_json::to_vec(state)?,
        )
    }
    pub fn recover_canvas_state(&self, id: Uuid, state: &StudioCanvasState) -> Result<()> {
        state.validate()?;
        let path = self.cache.join(format!("canvas-{id}.json"));
        if path.exists() && self.canvas_state(id).is_err() {
            fs::copy(
                &path,
                self.cache
                    .join(format!("canvas-{id}-preserved-{}.json", Uuid::new_v4())),
            )?;
        }
        atomic(&path, &serde_json::to_vec(state)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn screen(id: u128, width: u32) -> StudioScreen {
        StudioScreen {
            id: Uuid::from_u128(id),
            name: format!("Screen {id}"),
            width,
            height: 960,
            archived: false,
            files: Default::default(),
        }
    }
    #[test]
    fn focus_reconciles_archived_selection_and_round_trips_without_moving_screens() {
        let mut state=StudioCanvasState::default();
        assert_eq!(state.overview_mode,StudioOverviewMode::Canvas);
        let mut screens=vec![screen(1,390),screen(2,1440)];
        state.reconcile(&screens);
        let positions=state.positions.clone();
        state.overview_mode=StudioOverviewMode::Focus;
        state.selected_screen_id=Some(screens[0].id);
        screens[0].archived=true;
        state.reconcile(&screens);
        assert_eq!(state.selected_screen_id,Some(screens[1].id));
        assert_eq!(state.positions,positions);
        let decoded:StudioCanvasState=serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert_eq!(decoded,state);
        decoded.validate().unwrap();
    }
    #[test]
    fn layout_appends_without_moving_existing_and_restores_archives() {
        let mut state = StudioCanvasState::default();
        let mut screens = vec![screen(1, 390), screen(2, 1440)];
        state.reconcile(&screens);
        assert_eq!(state.positions[&Uuid::from_u128(2)].x, 510.);
        state.positions.get_mut(&Uuid::from_u128(1)).unwrap().x = -300.;
        screens[0].archived = true;
        screens.push(screen(3, 1440));
        state.reconcile(&screens);
        assert_eq!(state.positions[&Uuid::from_u128(1)].x, -300.);
        assert_eq!(state.positions[&Uuid::from_u128(3)].y, 1080.);
        screens[0].archived = false;
        state.reconcile(&screens);
        assert_eq!(state.positions[&Uuid::from_u128(1)].x, -300.);
    }
    #[test]
    fn invalid_camera_and_corrupt_layout_are_preserved() {
        let path = std::env::temp_dir().join(format!("choro-canvas-state-{}", Uuid::new_v4()));
        fs::create_dir_all(path.join("project")).unwrap();
        let store = StudioStore::new(path.join("project"), path.join("data")).unwrap();
        let id = Uuid::new_v4();
        let file = store.cache.join(format!("canvas-{id}.json"));
        atomic(&file, b"broken").unwrap();
        assert!(store.save_canvas_state(id, &Default::default()).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"broken");
        store.recover_canvas_state(id, &Default::default()).unwrap();
        assert_eq!(
            store.canvas_state(id).unwrap(),
            StudioCanvasState::default()
        );
        let mut state = StudioCanvasState::default();
        state.viewport.zoom = f64::NAN;
        assert!(state.validate().is_err());
        state.viewport.zoom = 3.;
        assert!(state.validate().is_err());
    }
}

#[cfg(test)]
mod resize_tests {
    use super::*;
    #[test]
    fn saved_resize_is_one_undo_step_and_layout_does_not_change_design() {
        let root = std::env::temp_dir().join(format!("choro-canvas-resize-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("project")).unwrap();
        let store = StudioStore::new(root.join("project"), root.join("data")).unwrap();
        let original = store.create("Canvas resize test").unwrap();
        let id = original.manifest.screens[0].id;
        let scope = StudioTurnScope::whole_design(&original);
        let mut screen = original.manifest.screens[0].clone();
        screen.width = 390;
        screen.height = 844;
        let tx = StudioTransaction {
            id: Uuid::new_v4(),
            scope_id: scope.id,
            design_id: original.manifest.id,
            expected_revision: original.manifest.revision,
            expected_fingerprint: original.fingerprint.clone(),
            operations: vec![StudioOperation::UpdateScreen { screen }],
        };
        let saved = store.apply(&scope, &tx).unwrap();
        assert_eq!(saved.manifest.revision, original.manifest.revision + 1);
        assert_eq!(saved.documents, original.documents);
        let undone = store.undo_latest(saved.manifest.id).unwrap();
        assert_eq!(undone.manifest.screens[0], original.manifest.screens[0]);
        let redone = store.redo_latest(saved.manifest.id).unwrap();
        assert_eq!(redone.manifest.screens[0].width, 390);
        assert_eq!(redone.manifest.screens[0].height, 844);
        let mut layout = StudioCanvasState::default();
        layout.reconcile(&redone.manifest.screens);
        layout
            .positions
            .insert(id, StudioCanvasPoint { x: 500., y: -100. });
        store
            .save_canvas_state(redone.manifest.id, &layout)
            .unwrap();
        assert_eq!(
            store.load(redone.manifest.id).unwrap().fingerprint,
            redone.fingerprint
        );
        let scope = StudioTurnScope::whole_design(&redone);
        let mut renamed = redone.manifest.screens[0].clone();
        renamed.name = "Agent renamed this".into();
        let rename = StudioTransaction {
            id: Uuid::new_v4(),
            scope_id: scope.id,
            design_id: redone.manifest.id,
            expected_revision: redone.manifest.revision,
            expected_fingerprint: redone.fingerprint.clone(),
            operations: vec![StudioOperation::UpdateScreen { screen: renamed }],
        };
        let latest = store.apply(&scope, &rename).unwrap();
        let mut stale = rename.clone();
        stale.id = Uuid::new_v4();
        let mut resized = redone.manifest.screens[0].clone();
        resized.width = 600;
        stale.operations = vec![StudioOperation::UpdateScreen { screen: resized }];
        assert!(store.apply(&scope, &stale).is_err());
        assert_eq!(store.load(latest.manifest.id).unwrap(), latest);
    }
}
