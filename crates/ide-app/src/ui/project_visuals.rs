use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, OnceLock};

use anyhow::{bail, Context as _, Result};
use gpui::{div, relative, svg, AnyElement, App, Hsla, IntoElement, ParentElement, Pixels, Styled};
use ide_core::local_store::LocalStore;
use ide_core::{ProjectId, CUSTOM_PROJECT_SVG_ICON};
use lucide_icons::Icon as LucideIcon;
use regex::{Captures, Regex};
use sha2::{Digest, Sha256};

pub const LUCIDE_ICON_FONT_FAMILY: &str = "lucide";

#[derive(Clone)]
pub struct ProjectIconOption {
    pub id: String,
    pub label: String,
    pub icon: LucideIcon,
}

#[derive(Clone, Copy)]
pub struct ProjectColorOption {
    pub id: &'static str,
    pub label: &'static str,
    pub color: Option<Hsla>,
}

static PROJECT_ICON_OPTIONS: OnceLock<Vec<ProjectIconOption>> = OnceLock::new();
static PROJECT_SVG_ASSETS: LazyLock<Mutex<HashMap<String, Vec<u8>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static PROJECT_SVG_PATHS: LazyLock<Mutex<HashMap<PathBuf, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SVG_PAINT_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(fill|stroke)\s*=\s*("[^"]*"|'[^']*')"#)
        .expect("project SVG paint regex must compile")
});
static SVG_STYLE_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bstyle\s*=\s*("[^"]*"|'[^']*')"#)
        .expect("project SVG style regex must compile")
});

const MAX_PROJECT_SVG_BYTES: u64 = 256 * 1024;
const ALLOWED_SVG_ELEMENTS: &[&str] = &[
    "svg", "g", "path", "circle", "ellipse", "rect", "line", "polyline", "polygon", "defs",
    "clipPath", "mask", "symbol", "use", "title", "desc", "metadata",
];

/// Returns a runtime SVG previously registered by [`project_icon_element`].
/// `AppAssets` calls this while gpui resolves the generated asset path.
pub fn custom_project_svg(path: &str) -> Option<Vec<u8>> {
    PROJECT_SVG_ASSETS.lock().ok()?.get(path).cloned()
}

/// Validate, make monochrome, and copy a user-selected SVG into Choro's
/// per-project data directory. The returned path never depends on the source
/// file continuing to exist.
pub fn import_project_svg(project: ProjectId, source: &Path) -> Result<PathBuf> {
    if !source
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
    {
        bail!("Choose an SVG file (.svg)");
    }

    let metadata =
        fs::metadata(source).with_context(|| format!("Could not read {}", source.display()))?;
    if metadata.len() > MAX_PROJECT_SVG_BYTES {
        bail!("SVG must be 256 KB or smaller");
    }
    let source_bytes =
        fs::read(source).with_context(|| format!("Could not read {}", source.display()))?;
    let normalized = normalize_project_svg(&source_bytes)?;
    let digest = Sha256::digest(&normalized);
    let hash = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let store = LocalStore::open_default().context("Could not open Choro's local storage")?;
    let directory = store.project_icons_dir(project);
    fs::create_dir_all(&directory).context("Could not create the project icon directory")?;
    let target = directory.join(format!("icon-{hash}.svg"));
    if !target.exists() {
        fs::write(&target, normalized).context("Could not save the custom SVG")?;
    }
    Ok(target)
}

fn normalize_project_svg(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() as u64 > MAX_PROJECT_SVG_BYTES {
        bail!("SVG must be 256 KB or smaller");
    }
    let source = std::str::from_utf8(bytes).context("SVG must be valid UTF-8 text")?;
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let lowercase = source.to_ascii_lowercase();
    if lowercase.contains("<!doctype") || lowercase.contains("<!entity") {
        bail!("SVG document types and entities are not supported");
    }

    let document = roxmltree::Document::parse(source).context("SVG is not valid XML")?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        bail!("The file must have an <svg> root element");
    }
    if root
        .tag_name()
        .namespace()
        .is_some_and(|namespace| namespace != "http://www.w3.org/2000/svg")
    {
        bail!("The file must use the standard SVG namespace");
    }
    let view_box = root
        .attribute("viewBox")
        .ok_or_else(|| anyhow::anyhow!("SVG must include a viewBox"))?;
    let view_box_values = view_box
        .split(|character: char| character.is_ascii_whitespace() || character == ',')
        .filter(|part| !part.is_empty())
        .map(str::parse::<f64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("SVG viewBox must contain four numbers")?;
    if view_box_values.len() != 4 || view_box_values[2] <= 0.0 || view_box_values[3] <= 0.0 {
        bail!("SVG viewBox must have a positive width and height");
    }

    for node in document.descendants().filter(roxmltree::Node::is_element) {
        let element = node.tag_name().name();
        if node
            .tag_name()
            .namespace()
            .is_some_and(|namespace| namespace != "http://www.w3.org/2000/svg")
        {
            bail!("SVG may not contain elements from another XML namespace");
        }
        if !ALLOWED_SVG_ELEMENTS.contains(&element) {
            bail!("SVG element <{element}> is not supported; use simple vector paths");
        }
        for attribute in node.attributes() {
            let name = attribute.name();
            let name_lower = name.to_ascii_lowercase();
            let value_lower = attribute.value().to_ascii_lowercase();
            if name_lower == "href"
                && !(element == "use" && attribute.value().trim().starts_with('#'))
            {
                bail!("SVG <use> may only reference an ID inside the same file");
            }
            if name_lower.starts_with("on") {
                bail!("SVG attribute '{name}' is not supported");
            }
            if value_lower.contains("javascript:") || value_lower.contains("data:") {
                bail!("SVG may not contain embedded or executable content");
            }
            if name_lower == "style"
                && (value_lower.contains("@import")
                    || value_lower.contains("expression(")
                    || value_lower.contains("behavior:"))
            {
                bail!("SVG style contains unsupported executable content");
            }
            if value_lower.contains("url(") && !has_only_local_svg_urls(&value_lower) {
                bail!("SVG may only reference definitions inside the same file");
            }
        }
    }

    let mut prepared = source.to_string();
    if root.attribute("fill").is_none() {
        let insertion_point = root.range().start + "<svg".len();
        if source.as_bytes().get(root.range().start..insertion_point) != Some(b"<svg") {
            bail!("Prefixed SVG root elements are not supported");
        }
        prepared.insert_str(insertion_point, r#" fill="currentColor""#);
    }
    let normalized_styles =
        SVG_STYLE_ATTRIBUTE.replace_all(&prepared, |captures: &Captures<'_>| {
            let quoted_value = &captures[1];
            let quote = &quoted_value[..1];
            let style = &quoted_value[1..quoted_value.len() - 1];
            let normalized = style
                .split(';')
                .map(|declaration| {
                    let Some((property, value)) = declaration.split_once(':') else {
                        return declaration.to_string();
                    };
                    let value = value.trim();
                    if property.trim().eq_ignore_ascii_case("fill")
                        || property.trim().eq_ignore_ascii_case("stroke")
                    {
                        let value = if value.eq_ignore_ascii_case("none") {
                            "none"
                        } else {
                            "currentColor"
                        };
                        format!("{}:{value}", property.trim())
                    } else {
                        declaration.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(";");
            format!("style={quote}{normalized}{quote}")
        });
    let normalized = SVG_PAINT_ATTRIBUTE
        .replace_all(&normalized_styles, |captures: &Captures<'_>| {
            let attribute = &captures[1];
            let quoted_value = &captures[2];
            let value = quoted_value[1..quoted_value.len() - 1].trim();
            if value.eq_ignore_ascii_case("none") {
                format!(r#"{attribute}="none""#)
            } else {
                format!(r#"{attribute}="currentColor""#)
            }
        })
        .into_owned();
    Ok(normalized.into_bytes())
}

fn has_only_local_svg_urls(value: &str) -> bool {
    let mut remaining = value;
    while let Some(start) = remaining.find("url(") {
        let after_open = &remaining[start + "url(".len()..];
        let Some(close) = after_open.find(')') else {
            return false;
        };
        let reference = after_open[..close]
            .trim()
            .trim_matches(|character| character == '\'' || character == '"')
            .trim();
        let Some(id) = reference.strip_prefix('#') else {
            return false;
        };
        if id.is_empty() || id.chars().any(char::is_whitespace) {
            return false;
        }
        remaining = &after_open[close + 1..];
    }
    true
}

fn custom_svg_asset_path(path: &Path) -> Option<String> {
    if let Some(asset_path) = PROJECT_SVG_PATHS.lock().ok()?.get(path).cloned() {
        return Some(asset_path);
    }
    let bytes = fs::read(path).ok()?;
    let normalized = normalize_project_svg(&bytes).ok()?;
    let digest = Sha256::digest(&normalized);
    let hash = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let asset_path = format!("project-icons/{hash}.svg");
    PROJECT_SVG_ASSETS
        .lock()
        .ok()?
        .entry(asset_path.clone())
        .or_insert(normalized);
    PROJECT_SVG_PATHS
        .lock()
        .ok()?
        .insert(path.to_path_buf(), asset_path.clone());
    Some(asset_path)
}

/// Returns every glyph exposed by the pinned Lucide icon font. Lucide assigns
/// its glyphs to the Unicode private-use area, so walking that small range lets
/// us build the searchable catalog without maintaining a second 1,700-item
/// name list beside the crate's generated enum.
pub fn project_icon_options() -> &'static [ProjectIconOption] {
    PROJECT_ICON_OPTIONS.get_or_init(|| {
        let mut icons = (0xE000..=0xE7FF)
            .filter_map(char::from_u32)
            .filter_map(|unicode| LucideIcon::try_from(unicode).ok())
            .map(|icon| {
                let id = icon.to_string();
                ProjectIconOption {
                    label: id.replace('-', " "),
                    id,
                    icon,
                }
            })
            .collect::<Vec<_>>();
        icons.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        icons
    })
}

/// A calm first page for the picker. Search still covers the full catalog.
pub const POPULAR_PROJECT_ICONS: &[&str] = &[
    "folder",
    "star",
    "briefcase-business",
    "code-2",
    "database",
    "file",
    "inbox",
    "globe",
    "git-branch",
    "search",
    "copy",
    "square-terminal",
    "bot",
    "settings",
    "wrench",
    "layout-dashboard",
    "panel-bottom-open",
    "panel-left-open",
    "book-open",
    "building-2",
    "chart-pie",
    "map",
    "bell",
    "info",
    "alert-triangle",
    "check-circle",
    "frame",
    "palette",
    "heart",
    "eye",
    "sun",
    "moon",
    "asterisk",
    "user",
    "circle-user",
    "calendar",
    "case-sensitive",
    "a-large-small",
    "replace",
    "redo-2",
    "undo-2",
    "thumbs-up",
    "thumbs-down",
    "rocket",
    "lightbulb",
    "package",
    "shopping-cart",
];

pub fn project_color_options() -> Vec<ProjectColorOption> {
    vec![
        ProjectColorOption {
            id: "default",
            label: "Default",
            color: None,
        },
        ProjectColorOption {
            id: "blue",
            label: "Slate Blue",
            color: Some(crate::ui::design::palette::project_avatar(0)),
        },
        ProjectColorOption {
            id: "green",
            label: "Sage",
            color: Some(crate::ui::design::palette::project_avatar(1)),
        },
        ProjectColorOption {
            id: "amber",
            label: "Honey",
            color: Some(crate::ui::design::palette::project_avatar(2)),
        },
        ProjectColorOption {
            id: "red",
            label: "Dusty Rose",
            color: Some(crate::ui::design::palette::project_avatar(3)),
        },
        ProjectColorOption {
            id: "pink",
            label: "Mauve",
            color: Some(crate::ui::design::palette::project_avatar(4)),
        },
        ProjectColorOption {
            id: "purple",
            label: "Amethyst",
            color: Some(crate::ui::design::palette::project_avatar(5)),
        },
        ProjectColorOption {
            id: "cyan",
            label: "Mist Cyan",
            color: Some(crate::ui::design::palette::project_avatar(6)),
        },
        ProjectColorOption {
            id: "gray",
            label: "Warm Gray",
            color: Some(crate::ui::design::palette::project_avatar(7)),
        },
        ProjectColorOption {
            id: "orange",
            label: "Terracotta",
            color: Some(crate::ui::design::palette::project_avatar(8)),
        },
        ProjectColorOption {
            id: "lime",
            label: "Olive",
            color: Some(crate::ui::design::palette::project_avatar(9)),
        },
        ProjectColorOption {
            id: "teal",
            label: "Patina",
            color: Some(crate::ui::design::palette::project_avatar(10)),
        },
        ProjectColorOption {
            id: "sky",
            label: "Cloud Blue",
            color: Some(crate::ui::design::palette::project_avatar(11)),
        },
        ProjectColorOption {
            id: "indigo",
            label: "Dusky Indigo",
            color: Some(crate::ui::design::palette::project_avatar(12)),
        },
        ProjectColorOption {
            id: "violet",
            label: "Velvet Violet",
            color: Some(crate::ui::design::palette::project_avatar(13)),
        },
        ProjectColorOption {
            id: "magenta",
            label: "Muted Orchid",
            color: Some(crate::ui::design::palette::project_avatar(14)),
        },
        ProjectColorOption {
            id: "coral",
            label: "Soft Coral",
            color: Some(crate::ui::design::palette::project_avatar(15)),
        },
        ProjectColorOption {
            id: "gold",
            label: "Antique Gold",
            color: Some(crate::ui::design::palette::project_avatar(16)),
        },
        ProjectColorOption {
            id: "mint",
            label: "Eucalyptus",
            color: Some(crate::ui::design::palette::project_avatar(17)),
        },
        ProjectColorOption {
            id: "brown",
            label: "Cocoa",
            color: Some(crate::ui::design::palette::project_avatar(18)),
        },
    ]
}

pub fn project_icon(icon_id: &str) -> LucideIcon {
    let canonical = match icon_id {
        // IDs from the original small picker, retained for compatibility.
        "terminal" => "square-terminal",
        "tools" => "settings-2",
        "dashboard" => "layout-dashboard",
        "panel" => "panel-bottom-open",
        "sidebar" => "panel-left-open",
        "book" => "book-open",
        "building" => "building-2",
        "inspector" => "scan-search",
        "chart" => "chart-pie",
        "alert" => "alert-triangle",
        "check" => "check-circle",
        "github" => "git-branch",
        "account" => "circle-user",
        "case" => "case-sensitive",
        "type" => "a-large-small",
        "redo" => "redo-2",
        "undo" => "undo-2",
        "thumbs_up" => "thumbs-up",
        "thumbs_down" => "thumbs-down",
        other => other,
    };
    LucideIcon::try_from(canonical).unwrap_or(LucideIcon::Folder)
}

pub fn project_icon_color(color_id: &str, cx: &App) -> Hsla {
    project_color_options()
        .into_iter()
        .find(|option| option.id == color_id)
        .and_then(|option| option.color)
        .unwrap_or_else(|| crate::ui::design::t3(cx))
}

pub fn project_icon_glyph(icon: LucideIcon, color: Hsla, glyph_size: Pixels) -> AnyElement {
    div()
        .flex_none()
        .font_family(LUCIDE_ICON_FONT_FAMILY)
        .text_size(glyph_size)
        .line_height(relative(1.))
        .text_color(color)
        .child(icon.unicode().to_string())
        .into_any_element()
}

pub fn project_icon_element(
    icon_id: &str,
    color_id: &str,
    custom_svg_path: Option<&Path>,
    size: Pixels,
    glyph_size: Pixels,
    cx: &App,
) -> AnyElement {
    let color = project_icon_color(color_id, cx);
    div()
        .size(size)
        .flex()
        .items_center()
        .justify_center()
        .child(project_icon_visual_glyph(
            icon_id,
            custom_svg_path,
            color,
            glyph_size,
        ))
        .into_any_element()
}

pub fn project_icon_visual_glyph(
    icon_id: &str,
    custom_svg_path: Option<&Path>,
    color: Hsla,
    glyph_size: Pixels,
) -> AnyElement {
    let custom_svg = (icon_id == CUSTOM_PROJECT_SVG_ICON)
        .then(|| custom_svg_path.and_then(custom_svg_asset_path))
        .flatten();
    if let Some(asset_path) = custom_svg {
        svg()
            .path(asset_path)
            .size(glyph_size)
            .text_color(color)
            .into_any_element()
    } else {
        project_icon_glyph(project_icon(icon_id), color, glyph_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lucide_catalog_exceeds_product_minimum() {
        assert!(project_icon_options().len() >= 1_000);
        assert!(project_icon_options()
            .iter()
            .any(|icon| icon.id == "folder"));
        for popular in POPULAR_PROJECT_ICONS {
            assert!(
                project_icon_options()
                    .iter()
                    .any(|icon| icon.id == *popular),
                "missing popular icon: {popular}"
            );
        }
    }

    #[test]
    fn project_palette_has_twenty_options() {
        assert_eq!(project_color_options().len(), 20);
    }

    #[test]
    fn custom_svg_is_normalized_to_current_color() {
        let svg = br##"<svg viewBox="0 0 24 24" fill="#112233"><path fill="none" stroke="#fff" d="M1 1h22v22z"/></svg>"##;
        let normalized = String::from_utf8(normalize_project_svg(svg).unwrap()).unwrap();
        assert!(normalized.contains(r#"fill="currentColor""#));
        assert!(normalized.contains(r#"fill="none""#));
        assert!(normalized.contains(r#"stroke="currentColor""#));
    }

    #[test]
    fn custom_svg_rejects_embedded_images() {
        let svg = br#"<svg viewBox="0 0 24 24"><image href="icon.png"/></svg>"#;
        let error = normalize_project_svg(svg).unwrap_err().to_string();
        assert!(error.contains("<image>"));
    }

    #[test]
    fn custom_svg_adds_a_theme_colored_default_fill() {
        let svg = br#"<svg viewBox="0 0 24 24"><path d="M1 1h22v22z"/></svg>"#;
        let normalized = String::from_utf8(normalize_project_svg(svg).unwrap()).unwrap();
        assert!(normalized.starts_with(r#"<svg fill="currentColor""#));
    }

    #[test]
    fn custom_svg_normalizes_inline_style_paints() {
        let svg = br##"<svg viewBox="0 0 24 24" style="fill:#123; stroke: rgb(1, 2, 3); stroke-width:2"><path style='fill:none;opacity:.6' d="M1 1h22v22z"/></svg>"##;
        let normalized = String::from_utf8(normalize_project_svg(svg).unwrap()).unwrap();
        assert!(
            normalized.contains(r#"style="fill:currentColor;stroke:currentColor; stroke-width:2""#)
        );
        assert!(normalized.contains(r#"style='fill:none;opacity:.6'"#));
    }

    #[test]
    fn custom_svg_allows_local_use_references() {
        let svg = br##"<svg viewBox="0 0 24 24"><defs><symbol id="mark"><path fill="#123" d="M1 1h22v22z"/></symbol></defs><use href="#mark" style="fill:#fff"/></svg>"##;
        let normalized = String::from_utf8(normalize_project_svg(svg).unwrap()).unwrap();
        assert!(normalized.contains(r##"href="#mark""##));
        assert!(normalized.contains(r#"style="fill:currentColor""#));
    }

    #[test]
    fn custom_svg_rejects_external_use_references() {
        let svg = br#"<svg viewBox="0 0 24 24"><use href="icons.svg#mark"/></svg>"#;
        let error = normalize_project_svg(svg).unwrap_err().to_string();
        assert!(error.contains("inside the same file"));
    }

    #[test]
    fn custom_svg_allows_quoted_and_spaced_local_urls() {
        for reference in ["url(#clip)", "url('#clip')", "url( &quot;#clip&quot; )"] {
            let svg = format!(
                r#"<svg viewBox="0 0 24 24"><defs><clipPath id="clip"><path d="M0 0h24v24z"/></clipPath></defs><g clip-path="{reference}"><path d="M1 1h22v22z"/></g></svg>"#
            );
            normalize_project_svg(svg.as_bytes()).unwrap();
        }
    }

    #[test]
    fn custom_svg_rejects_external_urls() {
        let svg = br#"<svg viewBox="0 0 24 24"><path fill="url(https://example.com/icon.svg#paint)" d="M1 1h22v22z"/></svg>"#;
        let error = normalize_project_svg(svg).unwrap_err().to_string();
        assert!(error.contains("inside the same file"));
    }
}
