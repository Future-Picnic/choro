//! Optional named sections. Membership, order and presentation are revisioned
//! design metadata; the board geometry derived from them is deterministic and
//! shared by the native host, the canvas, and agent tools.
use super::*;
use anyhow::{bail, ensure, Context, Result};

pub const MAX_SECTIONS: usize = 100;
pub const MAX_SECTION_NAME: usize = 120;
/// Space between screens inside a section, in canvas units.
pub const DEFAULT_SECTION_GAP: u32 = 96;
pub const MAX_SECTION_GAP: u32 = 1000;
pub const SECTION_PADDING: i64 = 48;
/// Space between sections on the board.
pub const SECTION_SPACING: i64 = 160;
/// Title band reserved above a section's screens. Zoom-aware labels are drawn
/// at 16–30 display pixels; this band clears a 16 px label (1.35 line height)
/// down to the minimum supported label zoom ([`SECTION_LABEL_MIN_ZOOM`]).
pub const SECTION_HEADER: i64 = 192;
pub const SECTION_LABEL_MIN_ZOOM: f64 = (16. * 1.35 + 6.) / SECTION_HEADER as f64;
/// Space for each artboard's own name caption above its screen.
pub const SCREEN_CAPTION: i64 = 44;
pub const SECTION_MIN_WIDTH: i64 = 360;
/// A compact drop area for a section without visible screens.
pub const EMPTY_SECTION_WIDTH: i64 = 480;
pub const EMPTY_SECTION_HEIGHT: i64 = 280;
pub const MAX_BOARD_COORDINATE: i64 = 1_000_000;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StudioSectionDirection {
    /// Left to right, top aligned.
    #[default]
    Horizontal,
    /// Top to bottom, left aligned.
    Vertical,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StudioSectionTitleStyle {
    #[default]
    LeftTitle,
    FullWidthHeader,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StudioSectionHeaderAlignment {
    #[default]
    Left,
    Center,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StudioSectionArrangement {
    /// Sections below one another.
    #[default]
    Stacked,
    /// Sections beside one another.
    SideBySide,
}
fn default_gap() -> u32 {
    DEFAULT_SECTION_GAP
}
/// A named flow. Screen membership lives only here, in order.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StudioSection {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub screen_ids: Vec<Uuid>,
    #[serde(default)]
    pub direction: StudioSectionDirection,
    #[serde(default = "default_gap")]
    pub gap: u32,
    #[serde(default)]
    pub title_style: StudioSectionTitleStyle,
    #[serde(default)]
    pub header_alignment: StudioSectionHeaderAlignment,
}
impl StudioSection {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            screen_ids: Vec::new(),
            direction: Default::default(),
            gap: DEFAULT_SECTION_GAP,
            title_style: Default::default(),
            header_alignment: Default::default(),
        }
    }
}
/// Integer board origin, persisted so later edits never shift the whole board.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct StudioSectionOrigin {
    pub x: i64,
    pub y: i64,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct StudioSectionLayout {
    #[serde(default)]
    pub direction: StudioSectionArrangement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<StudioSectionOrigin>,
}
impl StudioSectionLayout {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// `create_screen.section_id`: omitted means the request's frozen current
/// section, explicit `null` means unsectioned, and a UUID names a section.
pub(crate) mod presence {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use uuid::Uuid;
    pub fn serialize<S: Serializer>(value: &Option<Option<Uuid>>, s: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(inner) => inner.serialize(s),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<Uuid>>, D::Error> {
        Option::<Uuid>::deserialize(d).map(Some)
    }
}

fn validate_section_name(name: &str) -> Result<()> {
    ensure!(
        !name.trim().is_empty()
            && name.chars().count() <= MAX_SECTION_NAME
            && !name.chars().any(char::is_control),
        "Use a section name of 1–{MAX_SECTION_NAME} characters"
    );
    Ok(())
}
fn validate_origin(origin: &StudioSectionOrigin) -> Result<()> {
    ensure!(
        origin.x.abs() <= MAX_BOARD_COORDINATE && origin.y.abs() <= MAX_BOARD_COORDINATE,
        "Section board origin is out of range"
    );
    Ok(())
}

/// Unique identities, valid names, bounded spacing and coordinates, existing
/// screen references, and at most one section per screen.
pub fn validate_sections(manifest: &StudioDesignManifest) -> Result<()> {
    ensure!(
        manifest.sections.len() <= MAX_SECTIONS,
        "Studio supports up to {MAX_SECTIONS} sections per design"
    );
    ensure!(
        manifest.sections.is_empty() || !manifest.system_workspace,
        "Design-system workspaces cannot contain sections"
    );
    if let Some(origin) = &manifest.section_layout.origin {
        validate_origin(origin)?;
    }
    let screens: BTreeSet<Uuid> = manifest.screens.iter().map(|s| s.id).collect();
    let mut sections = BTreeSet::new();
    let mut members = BTreeSet::new();
    for section in &manifest.sections {
        ensure!(!section.id.is_nil(), "Section identity cannot be empty");
        ensure!(sections.insert(section.id), "Duplicate section identity");
        validate_section_name(&section.name)?;
        ensure!(
            section.gap <= MAX_SECTION_GAP,
            "Section gap must be 0–{MAX_SECTION_GAP}"
        );
        for id in &section.screen_ids {
            ensure!(screens.contains(id), "Section {} refers to a missing screen", section.name);
            ensure!(members.insert(*id), "A screen can belong to only one section");
        }
    }
    Ok(())
}

impl StudioDesignManifest {
    pub fn section(&self, id: Uuid) -> Option<&StudioSection> {
        self.sections.iter().find(|s| s.id == id)
    }
    fn section_mut(&mut self, id: Uuid) -> Result<&mut StudioSection> {
        self.sections
            .iter_mut()
            .find(|s| s.id == id)
            .context("Section does not exist")
    }
    pub fn section_of(&self, screen: Uuid) -> Option<&StudioSection> {
        self.sections.iter().find(|s| s.screen_ids.contains(&screen))
    }
    fn require_screen(&self, screen: Uuid) -> Result<()> {
        ensure!(
            self.screens.iter().any(|s| s.id == screen),
            "Screen does not exist"
        );
        Ok(())
    }
    fn detach_screen(&mut self, screen: Uuid) {
        for section in &mut self.sections {
            section.screen_ids.retain(|id| *id != screen);
        }
    }
    /// Listed screens move into the new section, leaving any previous section.
    pub fn create_section(&mut self, section: &StudioSection) -> Result<()> {
        ensure!(
            self.section(section.id).is_none(),
            "Section identity already exists"
        );
        validate_section_name(&section.name)?;
        let mut section = section.clone();
        section.name = section.name.trim().into();
        let mut seen = BTreeSet::new();
        for id in &section.screen_ids {
            self.require_screen(*id)?;
            ensure!(seen.insert(*id), "A screen is listed twice");
        }
        for id in &section.screen_ids {
            self.detach_screen(*id);
        }
        self.sections.push(section);
        Ok(())
    }
    pub fn update_section(
        &mut self,
        id: Uuid,
        name: Option<&str>,
        direction: Option<StudioSectionDirection>,
        gap: Option<u32>,
        title_style: Option<StudioSectionTitleStyle>,
        header_alignment: Option<StudioSectionHeaderAlignment>,
    ) -> Result<()> {
        if let Some(name) = name {
            validate_section_name(name)?;
        }
        let section = self.section_mut(id)?;
        if let Some(name) = name {
            section.name = name.trim().into();
        }
        if let Some(direction) = direction {
            section.direction = direction;
        }
        if let Some(gap) = gap {
            ensure!(gap <= MAX_SECTION_GAP, "Section gap must be 0–{MAX_SECTION_GAP}");
            section.gap = gap;
        }
        if let Some(style) = title_style {
            section.title_style = style;
        }
        if let Some(alignment) = header_alignment {
            section.header_alignment = alignment;
        }
        Ok(())
    }
    /// Move into a section (before an optional member) or out to Unsectioned.
    /// Unsectioned order follows the manifest's screen order.
    pub fn move_screen_to_section(
        &mut self,
        screen: Uuid,
        section: Option<Uuid>,
        before: Option<Uuid>,
    ) -> Result<()> {
        self.require_screen(screen)?;
        ensure!(before != Some(screen), "A screen cannot be placed before itself");
        match section {
            Some(target) => {
                ensure!(self.section(target).is_some(), "Section does not exist");
                if let Some(before) = before {
                    ensure!(
                        self.section(target).is_some_and(|s| s.screen_ids.contains(&before)),
                        "before_screen_id must belong to the target section"
                    );
                }
                self.detach_screen(screen);
                let members = &mut self.section_mut(target)?.screen_ids;
                let index = before
                    .and_then(|before| members.iter().position(|id| *id == before))
                    .unwrap_or(members.len());
                members.insert(index, screen);
            }
            None => {
                if let Some(before) = before {
                    self.require_screen(before)?;
                    ensure!(
                        self.section_of(before).is_none(),
                        "before_screen_id must be an unsectioned screen"
                    );
                }
                self.detach_screen(screen);
                if let Some(before) = before {
                    let moved = self.screens.remove(
                        self.screens.iter().position(|s| s.id == screen).context("Screen does not exist")?,
                    );
                    let index = self
                        .screens
                        .iter()
                        .position(|s| s.id == before)
                        .context("Screen does not exist")?;
                    self.screens.insert(index, moved);
                }
            }
        }
        Ok(())
    }
    pub fn reorder_section_screens(&mut self, id: Uuid, order: &[Uuid]) -> Result<()> {
        let section = self.section_mut(id)?;
        ensure!(
            order.len() == section.screen_ids.len()
                && order.iter().copied().collect::<BTreeSet<_>>()
                    == section.screen_ids.iter().copied().collect::<BTreeSet<_>>(),
            "Reorder must list every screen in the section exactly once"
        );
        section.screen_ids = order.to_vec();
        Ok(())
    }
    pub fn reorder_sections(&mut self, order: &[Uuid]) -> Result<()> {
        ensure!(
            order.len() == self.sections.len()
                && order.iter().copied().collect::<BTreeSet<_>>()
                    == self.sections.iter().map(|s| s.id).collect::<BTreeSet<_>>(),
            "Reorder must list every section exactly once"
        );
        self.sections
            .sort_by_key(|s| order.iter().position(|id| *id == s.id));
        Ok(())
    }
    pub fn set_section_layout(
        &mut self,
        direction: StudioSectionArrangement,
        origin: Option<StudioSectionOrigin>,
    ) -> Result<()> {
        if let Some(origin) = &origin {
            validate_origin(origin)?;
        }
        self.section_layout.direction = direction;
        if origin.is_some() {
            self.section_layout.origin = origin;
        }
        Ok(())
    }
    /// Removes only the grouping; every screen and its content remain.
    pub fn ungroup_section(&mut self, id: Uuid) -> Result<()> {
        let index = self
            .sections
            .iter()
            .position(|s| s.id == id)
            .context("Section does not exist")?;
        self.sections.remove(index);
        // With no board left, a future first section is placed afresh.
        if self.sections.is_empty() {
            self.section_layout.origin = None;
        }
        Ok(())
    }
    /// Place a new screen; `None` leaves it unsectioned.
    pub(crate) fn place_new_screen(&mut self, screen: Uuid, section: Option<Uuid>) -> Result<()> {
        if let Some(section) = section {
            self.move_screen_to_section(screen, Some(section), None)?;
        }
        Ok(())
    }
    /// Section membership and layout for an implementation handoff subset.
    pub(crate) fn retain_sections_for(&mut self, screens: &[Uuid]) {
        for section in &mut self.sections {
            section.screen_ids.retain(|id| screens.contains(id));
        }
        self.sections.retain(|s| !s.screen_ids.is_empty());
    }
}

/// One section's derived bounds, including its title band.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct StudioSectionBox {
    pub id: Uuid,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    pub header_height: i64,
    /// Ordered visible members; archived screens keep membership but no space.
    pub active_screen_ids: Vec<Uuid>,
}
impl StudioSectionBox {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x as f64
            && y >= self.y as f64
            && x <= (self.x + self.width) as f64
            && y <= (self.y + self.height) as f64
    }
}
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct StudioBoardGeometry {
    pub origin: StudioSectionOrigin,
    pub sections: Vec<StudioSectionBox>,
    /// Authoritative positions for visible grouped screens.
    pub positions: BTreeMap<Uuid, StudioCanvasPoint>,
}
impl StudioBoardGeometry {
    /// `(x, y, width, height)` of every section, or `None` for an empty board.
    pub fn bounds(&self) -> Option<(i64, i64, i64, i64)> {
        let first = self.sections.first()?;
        let (mut left, mut top) = (first.x, first.y);
        let (mut right, mut bottom) = (first.x + first.width, first.y + first.height);
        for b in &self.sections {
            left = left.min(b.x);
            top = top.min(b.y);
            right = right.max(b.x + b.width);
            bottom = bottom.max(b.y + b.height);
        }
        Some((left, top, right - left, bottom - top))
    }
    pub fn section_at(&self, x: f64, y: f64) -> Option<&StudioSectionBox> {
        self.sections.iter().find(|b| b.contains(x, y))
    }
}

/// Deterministic board layout. Horizontal sections run left-to-right with top
/// alignment; vertical sections top-to-bottom with left alignment. No wrapping.
pub fn board_geometry(
    manifest: &StudioDesignManifest,
    origin: StudioSectionOrigin,
) -> StudioBoardGeometry {
    let screens: BTreeMap<Uuid, &StudioScreen> =
        manifest.screens.iter().map(|s| (s.id, s)).collect();
    let mut geometry = StudioBoardGeometry {
        origin,
        ..Default::default()
    };
    let (mut x, mut y) = (origin.x, origin.y);
    for section in &manifest.sections {
        let active: Vec<&StudioScreen> = section
            .screen_ids
            .iter()
            .filter_map(|id| screens.get(id).copied())
            .filter(|s| !s.archived)
            .collect();
        let gap = section.gap as i64;
        let content_top = y + SECTION_HEADER + SECTION_PADDING;
        let (width, height) = if active.is_empty() {
            (EMPTY_SECTION_WIDTH, SECTION_HEADER + EMPTY_SECTION_HEIGHT)
        } else {
            let mut cursor = 0;
            let mut extent = 0;
            for screen in &active {
                let (w, h) = (screen.width as i64, screen.height as i64);
                let point = match section.direction {
                    StudioSectionDirection::Horizontal => {
                        let p = (x + SECTION_PADDING + cursor, content_top + SCREEN_CAPTION);
                        cursor += w + gap;
                        extent = extent.max(h);
                        p
                    }
                    StudioSectionDirection::Vertical => {
                        let p = (x + SECTION_PADDING, content_top + SCREEN_CAPTION + cursor);
                        cursor += SCREEN_CAPTION + h + gap;
                        extent = extent.max(w);
                        p
                    }
                };
                geometry.positions.insert(
                    screen.id,
                    StudioCanvasPoint {
                        x: point.0 as f64,
                        y: point.1 as f64,
                    },
                );
            }
            let along = cursor - gap;
            match section.direction {
                StudioSectionDirection::Horizontal => (
                    along + 2 * SECTION_PADDING,
                    SECTION_HEADER + 2 * SECTION_PADDING + SCREEN_CAPTION + extent,
                ),
                StudioSectionDirection::Vertical => (
                    extent + 2 * SECTION_PADDING,
                    SECTION_HEADER + 2 * SECTION_PADDING + along,
                ),
            }
        };
        let width = width.max(SECTION_MIN_WIDTH);
        geometry.sections.push(StudioSectionBox {
            id: section.id,
            x,
            y,
            width,
            height,
            header_height: SECTION_HEADER,
            active_screen_ids: active.iter().map(|s| s.id).collect(),
        });
        match manifest.section_layout.direction {
            StudioSectionArrangement::Stacked => y += height + SECTION_SPACING,
            StudioSectionArrangement::SideBySide => x += width + SECTION_SPACING,
        }
    }
    geometry
}

/// Insertion target for a screen dropped at a canvas point: the section under
/// the point and the member it should precede, or `None` outside every section.
pub fn drop_target(
    manifest: &StudioDesignManifest,
    geometry: &StudioBoardGeometry,
    dragged: Uuid,
    x: f64,
    y: f64,
) -> Option<(Uuid, Option<Uuid>)> {
    let section = geometry.section_at(x, y)?;
    let direction = manifest.section(section.id)?.direction;
    let screens: BTreeMap<Uuid, &StudioScreen> =
        manifest.screens.iter().map(|s| (s.id, s)).collect();
    let before = section
        .active_screen_ids
        .iter()
        .filter(|id| **id != dragged)
        .find(|id| {
            let (Some(point), Some(screen)) = (geometry.positions.get(id), screens.get(id)) else {
                return false;
            };
            match direction {
                StudioSectionDirection::Horizontal => x < point.x + screen.width as f64 / 2.,
                StudioSectionDirection::Vertical => y < point.y + screen.height as f64 / 2.,
            }
        })
        .copied();
    Some((section.id, before))
}

/// Summary used by agent context, reads and handoffs.
pub fn section_summary(manifest: &StudioDesignManifest, section: &StudioSection) -> serde_json::Value {
    let screens = section
        .screen_ids
        .iter()
        .enumerate()
        .filter_map(|(order, id)| {
            let screen = manifest.screens.iter().find(|s| s.id == *id)?;
            Some(serde_json::json!({
                "order": order + 1,
                "id": screen.id,
                "name": screen.name,
                "width": screen.width,
                "height": screen.height,
                "archived": screen.archived,
            }))
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "id": section.id,
        "name": section.name,
        "direction": section.direction,
        "gap": section.gap,
        "title_style": section.title_style,
        "header_alignment": section.header_alignment,
        "screen_ids": section.screen_ids,
        "screens": screens,
    })
}

pub(crate) fn reject_in_system(manifest: &StudioDesignManifest) -> Result<()> {
    if manifest.system_workspace {
        bail!("Design-system workspaces cannot contain sections");
    }
    Ok(())
}

#[cfg(test)]
#[path = "sections_tests.rs"]
mod tests;
