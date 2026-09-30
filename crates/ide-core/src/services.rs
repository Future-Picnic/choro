//! Detect the third-party services a project uses — from its files alone.
//!
//! Read-only and **secret-free**: detection looks at `package.json`
//! dependency *names*, deploy/config marker files, and platform folders. It
//! never opens `.env` files or reads any credential value. The result is a
//! per–sub-app inventory (a monorepo has several: app, web, server, admin…),
//! each service carrying a link to its dashboard so the UI can be a launchpad.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Coarse grouping used to organize the inventory. `order` sets display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceCategory {
    Hosting,
    MobilePlatform,
    Database,
    Backend,
    Auth,
    Payments,
    Analytics,
    Attribution,
    Notifications,
    Email,
    Observability,
    Ai,
    Search,
    Media,
    Localization,
    Framework,
    Infra,
    Other,
}

impl ServiceCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hosting => "Hosting",
            Self::MobilePlatform => "Mobile & Stores",
            Self::Database => "Database",
            Self::Backend => "Backend",
            Self::Auth => "Auth",
            Self::Payments => "Payments",
            Self::Analytics => "Analytics",
            Self::Attribution => "Attribution",
            Self::Notifications => "Notifications",
            Self::Email => "Email",
            Self::Observability => "Observability",
            Self::Ai => "AI",
            Self::Search => "Search",
            Self::Media => "Media",
            Self::Localization => "Localization",
            Self::Framework => "Framework",
            Self::Infra => "Infrastructure",
            Self::Other => "Other",
        }
    }

    pub fn order(self) -> u8 {
        match self {
            Self::Hosting => 0,
            Self::MobilePlatform => 1,
            Self::Backend => 2,
            Self::Database => 3,
            Self::Auth => 4,
            Self::Payments => 5,
            Self::Analytics => 6,
            Self::Attribution => 7,
            Self::Notifications => 8,
            Self::Email => 9,
            Self::Observability => 10,
            Self::Ai => 11,
            Self::Search => 12,
            Self::Media => 13,
            Self::Localization => 14,
            Self::Infra => 15,
            Self::Framework => 16,
            Self::Other => 17,
        }
    }
}

/// One detected service within a sub-app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedService {
    pub id: String,
    pub name: String,
    pub category: ServiceCategory,
    pub dashboard_url: Option<String>,
    pub docs_url: Option<String>,
    /// Brand tile color as `0xRRGGBB`.
    pub brand_hex: u32,
    /// 1–3 char monogram for the brand tile.
    pub monogram: String,
    /// How it was found (e.g. `"dep: stripe"`, `"render.yaml"`, `"ios/"`).
    pub evidence: Vec<String>,
    /// Real values pulled from the project's own config files — no credentials
    /// needed (e.g. a Render service's live `*.onrender.com` URL).
    pub facts: Vec<ServiceFact>,
}

/// A concrete value shown on a service card. `url` makes it clickable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceFact {
    pub label: String,
    pub value: String,
    pub url: Option<String>,
}

/// A folder in the project that behaves like its own app (has a `package.json`,
/// or is the repo root), with the services detected inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubAppServices {
    pub name: String,
    /// Path relative to the project root (`"."` for the root itself).
    pub rel_path: String,
    pub services: Vec<DetectedService>,
    /// Names of `.env*` files present in this sub-app (values NOT read here —
    /// this is just the file list for the sidebar; values are read on demand).
    pub env_files: Vec<String>,
}

/// One parsed environment file: its keys and values. Values are read only when
/// the Env view actually needs them, and the UI masks them by default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvFile {
    pub name: String,
    pub entries: Vec<EnvEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvEntry {
    pub key: String,
    pub value: String,
}

/// Detect every sub-app and its services under `root`. Fast, shallow, and never
/// reads file contents beyond `package.json` manifests.
pub fn detect_project_services(root: &Path) -> Vec<SubAppServices> {
    let mut sub_dirs = find_sub_app_dirs(root);
    // Always include the root as a sub-app (carries root-level deploy configs).
    if !sub_dirs.iter().any(|dir| dir == root) {
        sub_dirs.insert(0, root.to_path_buf());
    }
    sub_dirs.sort();
    sub_dirs.dedup();

    let mut result: Vec<SubAppServices> = sub_dirs
        .into_iter()
        .filter_map(|dir| detect_in_dir(root, &dir))
        .filter(|sub| !sub.services.is_empty() || !sub.env_files.is_empty())
        .collect();

    // Root first, then alphabetical.
    result.sort_by(|a, b| match (a.rel_path == ".", b.rel_path == ".") {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.rel_path.cmp(&b.rel_path),
    });
    result
}

/// Discover only the project folders that contain environment files.
///
/// Unlike [`detect_project_services`], this never reads dependency manifests or
/// integration marker files. It exists so opening Environment cannot trigger
/// the Integrations detector as a side effect.
pub fn detect_project_environment_files(root: &Path) -> Vec<SubAppServices> {
    let mut sub_dirs = find_sub_app_dirs(root);
    if !sub_dirs.iter().any(|dir| dir == root) {
        sub_dirs.insert(0, root.to_path_buf());
    }
    sub_dirs.sort();
    sub_dirs.dedup();

    let mut result = sub_dirs
        .into_iter()
        .filter_map(|dir| {
            let rel_path = dir
                .strip_prefix(root)
                .ok()
                .map(|path| path.to_string_lossy().to_string())
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| ".".to_string());
            let env_files = list_env_file_names(&dir);
            (!env_files.is_empty()).then(|| SubAppServices {
                name: sub_app_name(&dir, &rel_path),
                rel_path,
                services: Vec::new(),
                env_files,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by(|a, b| match (a.rel_path == ".", b.rel_path == ".") {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.rel_path.cmp(&b.rel_path),
    });
    result
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "dist",
    "build",
    ".next",
    ".expo",
    "Pods",
    "vendor",
    "target",
    ".turbo",
    "coverage",
    ".cache",
];

const MAX_DEPTH: usize = 3;
const MAX_SUB_APPS: usize = 16;

fn find_sub_app_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    walk_for_package_json(root, root, 0, &mut dirs);
    dirs.truncate(MAX_SUB_APPS);
    dirs
}

fn walk_for_package_json(root: &Path, dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if out.len() >= MAX_SUB_APPS || depth > MAX_DEPTH {
        return;
    }
    if dir.join("package.json").is_file() {
        out.push(dir.to_path_buf());
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && depth > 0 {
            continue;
        }
        if SKIP_DIRS.contains(&name.as_ref()) {
            continue;
        }
        // Don't descend into a mobile app's native folders looking for sub-apps.
        if depth > 0 && (name == "ios" || name == "android") {
            continue;
        }
        let _ = root;
        walk_for_package_json(root, &path, depth + 1, out);
    }
}

fn detect_in_dir(root: &Path, dir: &Path) -> Option<SubAppServices> {
    let rel = dir
        .strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| ".".to_string());

    let deps = read_package_dependency_names(dir);
    let markers = collect_markers(dir);

    let mut services: Vec<DetectedService> = Vec::new();
    for rule in RULES {
        let mut evidence = Vec::new();
        for dep in &deps {
            if rule.dep_patterns.iter().any(|pat| dep_matches(dep, pat)) {
                evidence.push(format!("dep: {dep}"));
            }
        }
        for &marker in rule.markers {
            if markers.contains(marker) {
                evidence.push(marker_label(marker));
            }
        }
        if evidence.is_empty() {
            continue;
        }
        evidence.truncate(3);
        services.push(DetectedService {
            id: rule.id.to_string(),
            name: rule.name.to_string(),
            category: rule.category,
            dashboard_url: rule.dashboard.map(str::to_string),
            docs_url: rule.docs.map(str::to_string),
            brand_hex: rule.brand,
            monogram: rule.monogram.to_string(),
            evidence,
            facts: Vec::new(),
        });
    }

    // Enrich with real values from config files (no credentials needed).
    for service in &mut services {
        if service.id == "render" {
            service.facts = render_facts(dir);
        }
    }

    services.sort_by(|a, b| {
        a.category
            .order()
            .cmp(&b.category.order())
            .then_with(|| a.name.cmp(&b.name))
    });

    Some(SubAppServices {
        name: sub_app_name(dir, &rel),
        rel_path: rel,
        services,
        env_files: list_env_file_names(dir),
    })
}

/// Real facts for a Render service, derived from `render.yaml`: each `web`
/// service's live `*.onrender.com` URL, plus other services by name/type. No
/// network, no credentials — Render's public URL is a function of the name.
fn render_facts(dir: &Path) -> Vec<ServiceFact> {
    let Ok(text) = std::fs::read_to_string(dir.join("render.yaml")) else {
        return Vec::new();
    };
    parse_render_services(&text)
        .into_iter()
        .map(|(name, kind)| {
            let public = matches!(kind.as_str(), "web" | "static" | "static_site" | "");
            ServiceFact {
                // The URL host already names the service, so a public one needs
                // no extra label; private services show their type instead.
                label: if public { String::new() } else { kind.clone() },
                value: if public {
                    format!("{name}.onrender.com")
                } else {
                    name.clone()
                },
                url: public.then(|| format!("https://{name}.onrender.com")),
            }
        })
        .collect()
}

/// Extract `(name, type)` for each service block in a `render.yaml`. A tiny
/// line parser — good enough for the flat `services:` list Render blueprints use.
fn parse_render_services(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_services = false;
    let mut name: Option<String> = None;
    let mut kind: Option<String> = None;

    let flush =
        |out: &mut Vec<(String, String)>, name: &mut Option<String>, kind: &mut Option<String>| {
            if let Some(n) = name.take() {
                out.push((n, kind.take().unwrap_or_default()));
            } else {
                *kind = None;
            }
        };

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if trimmed.starts_with("services:") {
            in_services = true;
            continue;
        }
        if !in_services {
            continue;
        }
        // A new top-level key ends the services list.
        if indent == 0 && !trimmed.starts_with('-') && trimmed.contains(':') {
            flush(&mut out, &mut name, &mut kind);
            in_services = false;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- ") {
            flush(&mut out, &mut name, &mut kind);
            apply_render_kv(rest.trim(), &mut name, &mut kind);
            continue;
        }
        apply_render_kv(trimmed, &mut name, &mut kind);
    }
    flush(&mut out, &mut name, &mut kind);
    out
}

fn apply_render_kv(line: &str, name: &mut Option<String>, kind: &mut Option<String>) {
    if let Some(v) = line.strip_prefix("name:") {
        *name = Some(v.trim().trim_matches(|c| c == '"' || c == '\'').to_string());
    } else if let Some(v) = line.strip_prefix("type:") {
        *kind = Some(v.trim().trim_matches(|c| c == '"' || c == '\'').to_string());
    }
}

/// `.env*` file names present in `dir`, ordered `.env` first then the rest, with
/// obvious backups filtered out. Only names — nothing is read.
fn list_env_file_names(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| name == ".env" || name.starts_with(".env."))
        .filter(|name| !name.contains(".bak") && !name.ends_with('~'))
        .collect();
    names.sort_by(|a, b| {
        env_file_rank(a)
            .cmp(&env_file_rank(b))
            .then_with(|| a.cmp(b))
    });
    names
}

/// Sort key so plain `.env` leads and `.env.example` trails.
fn env_file_rank(name: &str) -> u8 {
    match name {
        ".env" => 0,
        ".env.local" => 1,
        ".env.development" => 2,
        ".env.production" => 3,
        n if n.ends_with(".example") => 9,
        _ => 5,
    }
}

/// Parse a single env file into key/value entries. Values are returned verbatim
/// (minus surrounding quotes). Comments and blank lines are skipped. Malformed
/// or unreadable files yield an empty entry list rather than an error.
pub fn read_env_file(path: &Path) -> EnvFile {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut entries = Vec::new();
    if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.lines() {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let line = trimmed.strip_prefix("export ").unwrap_or(trimmed);
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            entries.push(EnvEntry {
                key: key.to_string(),
                value: unquote(value.trim()),
            });
        }
    }
    EnvFile { name, entries }
}

/// Read a sub-app's env files (given their names) into parsed key/value form.
pub fn read_sub_app_env(dir: &Path, file_names: &[String]) -> Vec<EnvFile> {
    file_names
        .iter()
        .map(|name| read_env_file(&dir.join(name)))
        .collect()
}

/// Set (or add) `key`'s value in an env file, preserving every other line —
/// comments, ordering, blank lines, and any `export ` prefix on the edited line.
/// Written atomically (temp file + rename) so a crash can't corrupt the file.
pub fn set_env_value(path: &Path, key: &str, value: &str) -> std::io::Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let formatted = format_env_value(value);
    let mut lines: Vec<String> = Vec::new();
    let mut replaced = false;

    for line in existing.lines() {
        let trimmed = line.trim_start();
        let candidate = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let is_target = !trimmed.starts_with('#')
            && candidate
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key);
        if is_target && !replaced {
            let indent = &line[..line.len() - trimmed.len()];
            let export = if trimmed.starts_with("export ") {
                "export "
            } else {
                ""
            };
            lines.push(format!("{indent}{export}{key}={formatted}"));
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        lines.push(format!("{key}={formatted}"));
    }

    let mut content = lines.join("\n");
    content.push('\n');
    write_atomic(path, content.as_bytes())
}

fn format_env_value(value: &str) -> String {
    let needs_quote = value.is_empty()
        || value.chars().any(|c| c.is_whitespace())
        || value.contains('#')
        || value.contains('"')
        || value.contains('\'');
    if needs_quote {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("env");
    let mut tmp = path.to_path_buf();
    tmp.set_file_name(format!(".{file_name}.ide-tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

fn sub_app_name(dir: &Path, rel: &str) -> String {
    if rel == "." {
        return "Project".to_string();
    }
    // Prefer a readable last path segment; fall back to the whole rel path.
    dir.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| rel.to_string())
}

/// Dependency names from `package.json` (`dependencies` + `devDependencies`).
/// Only names — values and the rest of the manifest are ignored.
fn read_package_dependency_names(dir: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else {
        return names;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return names;
    };
    for key in ["dependencies", "devDependencies"] {
        if let Some(map) = value.get(key).and_then(|v| v.as_object()) {
            for dep in map.keys() {
                names.insert(dep.to_ascii_lowercase());
            }
        }
    }
    names
}

/// Marker keys present in `dir` (config files + platform folders).
fn collect_markers(dir: &Path) -> BTreeSet<&'static str> {
    let mut markers = BTreeSet::new();
    const FILES: &[&str] = &[
        "render.yaml",
        "vercel.json",
        "netlify.toml",
        "fly.toml",
        "railway.json",
        "wrangler.toml",
        "Dockerfile",
        "docker-compose.yml",
        "docker-compose.yaml",
        "app.json",
        "eas.json",
        "firebase.json",
        "google-services.json",
        "supabase",
        "prisma",
    ];
    for file in FILES {
        if dir.join(file).exists() {
            markers.insert(*file);
        }
    }
    if dir.join(".vercel").is_dir() {
        markers.insert("vercel.json");
    }
    if dir.join("ios").is_dir() {
        markers.insert("dir:ios");
    }
    if dir.join("android").is_dir() {
        markers.insert("dir:android");
    }
    markers
}

fn marker_label(marker: &str) -> String {
    match marker {
        "dir:ios" => "ios/".to_string(),
        "dir:android" => "android/".to_string(),
        other => other.to_string(),
    }
}

/// A dependency matches a pattern when it equals it, or — for a pattern ending
/// in `/` (a scope prefix like `@react-native-firebase/`) — starts with it.
fn dep_matches(dep: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('/') {
        dep.starts_with(prefix)
    } else {
        dep == pattern
    }
}

struct Rule {
    id: &'static str,
    name: &'static str,
    category: ServiceCategory,
    dashboard: Option<&'static str>,
    docs: Option<&'static str>,
    brand: u32,
    monogram: &'static str,
    dep_patterns: &'static [&'static str],
    markers: &'static [&'static str],
}

const RULES: &[Rule] = &[
    // ── Hosting / deploy ──────────────────────────────────────────────
    Rule {
        id: "render",
        name: "Render",
        category: ServiceCategory::Hosting,
        dashboard: Some("https://dashboard.render.com"),
        docs: Some("https://render.com/docs"),
        brand: 0x14b8a6,
        monogram: "Rn",
        dep_patterns: &[],
        markers: &["render.yaml"],
    },
    Rule {
        id: "vercel",
        name: "Vercel",
        category: ServiceCategory::Hosting,
        dashboard: Some("https://vercel.com/dashboard"),
        docs: Some("https://vercel.com/docs"),
        brand: 0x2b2b2b,
        monogram: "Vc",
        dep_patterns: &["vercel"],
        markers: &["vercel.json"],
    },
    Rule {
        id: "netlify",
        name: "Netlify",
        category: ServiceCategory::Hosting,
        dashboard: Some("https://app.netlify.com"),
        docs: Some("https://docs.netlify.com"),
        brand: 0x00c7b7,
        monogram: "Nf",
        dep_patterns: &["netlify-cli"],
        markers: &["netlify.toml"],
    },
    Rule {
        id: "fly",
        name: "Fly.io",
        category: ServiceCategory::Hosting,
        dashboard: Some("https://fly.io/dashboard"),
        docs: Some("https://fly.io/docs"),
        brand: 0x8b5cf6,
        monogram: "Fl",
        dep_patterns: &[],
        markers: &["fly.toml"],
    },
    Rule {
        id: "railway",
        name: "Railway",
        category: ServiceCategory::Hosting,
        dashboard: Some("https://railway.app/dashboard"),
        docs: None,
        brand: 0x6c5ce7,
        monogram: "Rw",
        dep_patterns: &[],
        markers: &["railway.json"],
    },
    Rule {
        id: "cloudflare",
        name: "Cloudflare",
        category: ServiceCategory::Hosting,
        dashboard: Some("https://dash.cloudflare.com"),
        docs: None,
        brand: 0xf6821f,
        monogram: "Cf",
        dep_patterns: &["wrangler"],
        markers: &["wrangler.toml"],
    },
    // ── Mobile & stores ───────────────────────────────────────────────
    Rule {
        id: "appstore",
        name: "App Store",
        category: ServiceCategory::MobilePlatform,
        dashboard: Some("https://appstoreconnect.apple.com"),
        docs: None,
        brand: 0x0ea5e9,
        monogram: "iOS",
        dep_patterns: &[],
        markers: &["dir:ios"],
    },
    Rule {
        id: "googleplay",
        name: "Google Play",
        category: ServiceCategory::MobilePlatform,
        dashboard: Some("https://play.google.com/console"),
        docs: None,
        brand: 0x22c55e,
        monogram: "GP",
        dep_patterns: &[],
        markers: &["dir:android"],
    },
    Rule {
        id: "expo",
        name: "Expo",
        category: ServiceCategory::MobilePlatform,
        dashboard: Some("https://expo.dev"),
        docs: Some("https://docs.expo.dev"),
        brand: 0x1b1f23,
        monogram: "Ex",
        dep_patterns: &["expo"],
        markers: &["eas.json"],
    },
    // ── Backend platforms ─────────────────────────────────────────────
    Rule {
        id: "firebase",
        name: "Firebase",
        category: ServiceCategory::Backend,
        dashboard: Some("https://console.firebase.google.com"),
        docs: Some("https://firebase.google.com/docs"),
        brand: 0xf59e0b,
        monogram: "Fb",
        dep_patterns: &[
            "firebase",
            "firebase-admin",
            "@react-native-firebase/",
            "@firebase/",
        ],
        markers: &["firebase.json", "google-services.json"],
    },
    Rule {
        id: "supabase",
        name: "Supabase",
        category: ServiceCategory::Backend,
        dashboard: Some("https://supabase.com/dashboard"),
        docs: Some("https://supabase.com/docs"),
        brand: 0x3ecf8e,
        monogram: "Sb",
        dep_patterns: &["@supabase/"],
        markers: &["supabase"],
    },
    // ── Database ──────────────────────────────────────────────────────
    Rule {
        id: "mongodb",
        name: "MongoDB",
        category: ServiceCategory::Database,
        dashboard: Some("https://cloud.mongodb.com"),
        docs: Some("https://www.mongodb.com/docs"),
        brand: 0x10b981,
        monogram: "Mo",
        dep_patterns: &["mongoose", "mongodb"],
        markers: &[],
    },
    Rule {
        id: "prisma",
        name: "Prisma / Postgres",
        category: ServiceCategory::Database,
        dashboard: Some("https://cloud.prisma.io"),
        docs: Some("https://www.prisma.io/docs"),
        brand: 0x0c344b,
        monogram: "Pr",
        dep_patterns: &["@prisma/client", "prisma", "pg"],
        markers: &["prisma"],
    },
    Rule {
        id: "redis",
        name: "Redis",
        category: ServiceCategory::Database,
        dashboard: Some("https://app.redislabs.com"),
        docs: None,
        brand: 0xdc382d,
        monogram: "Rd",
        dep_patterns: &["redis", "ioredis"],
        markers: &[],
    },
    // ── Auth ──────────────────────────────────────────────────────────
    Rule {
        id: "clerk",
        name: "Clerk",
        category: ServiceCategory::Auth,
        dashboard: Some("https://dashboard.clerk.com"),
        docs: None,
        brand: 0x6c47ff,
        monogram: "Ck",
        dep_patterns: &["@clerk/"],
        markers: &[],
    },
    Rule {
        id: "auth0",
        name: "Auth0",
        category: ServiceCategory::Auth,
        dashboard: Some("https://manage.auth0.com"),
        docs: None,
        brand: 0xeb5424,
        monogram: "A0",
        dep_patterns: &["auth0", "@auth0/"],
        markers: &[],
    },
    // ── Payments ──────────────────────────────────────────────────────
    Rule {
        id: "stripe",
        name: "Stripe",
        category: ServiceCategory::Payments,
        dashboard: Some("https://dashboard.stripe.com"),
        docs: Some("https://stripe.com/docs"),
        brand: 0x635bff,
        monogram: "St",
        dep_patterns: &["stripe", "@stripe/"],
        markers: &[],
    },
    Rule {
        id: "revenuecat",
        name: "RevenueCat",
        category: ServiceCategory::Payments,
        dashboard: Some("https://app.revenuecat.com"),
        docs: Some("https://www.revenuecat.com/docs"),
        brand: 0xef4444,
        monogram: "Rc",
        dep_patterns: &["react-native-purchases", "@revenuecat/"],
        markers: &[],
    },
    // ── Analytics / experimentation ───────────────────────────────────
    Rule {
        id: "amplitude",
        name: "Amplitude",
        category: ServiceCategory::Analytics,
        dashboard: Some("https://app.amplitude.com"),
        docs: None,
        brand: 0x3b82f6,
        monogram: "Am",
        dep_patterns: &["@amplitude/", "amplitude"],
        markers: &[],
    },
    Rule {
        id: "mixpanel",
        name: "Mixpanel",
        category: ServiceCategory::Analytics,
        dashboard: Some("https://mixpanel.com"),
        docs: None,
        brand: 0x7c3aed,
        monogram: "Mx",
        dep_patterns: &["mixpanel", "mixpanel-browser"],
        markers: &[],
    },
    Rule {
        id: "posthog",
        name: "PostHog",
        category: ServiceCategory::Analytics,
        dashboard: Some("https://app.posthog.com"),
        docs: None,
        brand: 0xf54e00,
        monogram: "Ph",
        dep_patterns: &["posthog-js", "posthog-node"],
        markers: &[],
    },
    Rule {
        id: "statsig",
        name: "Statsig",
        category: ServiceCategory::Analytics,
        dashboard: Some("https://console.statsig.com"),
        docs: None,
        brand: 0xeab308,
        monogram: "Sg",
        dep_patterns: &["@statsig/", "statsig"],
        markers: &[],
    },
    // ── Attribution / marketing ───────────────────────────────────────
    Rule {
        id: "appsflyer",
        name: "AppsFlyer",
        category: ServiceCategory::Attribution,
        dashboard: Some("https://hq1.appsflyer.com"),
        docs: None,
        brand: 0x06b6d4,
        monogram: "Af",
        dep_patterns: &["react-native-appsflyer", "appsflyer"],
        markers: &[],
    },
    // ── Notifications ─────────────────────────────────────────────────
    Rule {
        id: "onesignal",
        name: "OneSignal",
        category: ServiceCategory::Notifications,
        dashboard: Some("https://app.onesignal.com"),
        docs: None,
        brand: 0xff3b30,
        monogram: "Os",
        dep_patterns: &["react-native-onesignal", "onesignal-node"],
        markers: &[],
    },
    Rule {
        id: "wonderpush",
        name: "WonderPush",
        category: ServiceCategory::Notifications,
        dashboard: Some("https://dashboard.wonderpush.com"),
        docs: None,
        brand: 0xec4899,
        monogram: "Wp",
        dep_patterns: &["react-native-wonderpush"],
        markers: &[],
    },
    // ── Email ─────────────────────────────────────────────────────────
    Rule {
        id: "resend",
        name: "Resend",
        category: ServiceCategory::Email,
        dashboard: Some("https://resend.com/overview"),
        docs: None,
        brand: 0x2b2b2b,
        monogram: "Re",
        dep_patterns: &["resend"],
        markers: &[],
    },
    Rule {
        id: "sendgrid",
        name: "SendGrid",
        category: ServiceCategory::Email,
        dashboard: Some("https://app.sendgrid.com"),
        docs: None,
        brand: 0x1a82e2,
        monogram: "SG",
        dep_patterns: &["@sendgrid/"],
        markers: &[],
    },
    // ── Observability ─────────────────────────────────────────────────
    Rule {
        id: "sentry",
        name: "Sentry",
        category: ServiceCategory::Observability,
        dashboard: Some("https://sentry.io"),
        docs: None,
        brand: 0x362d59,
        monogram: "Se",
        dep_patterns: &["@sentry/"],
        markers: &[],
    },
    Rule {
        id: "axiom",
        name: "Axiom",
        category: ServiceCategory::Observability,
        dashboard: Some("https://app.axiom.co"),
        docs: None,
        brand: 0xf97316,
        monogram: "Ax",
        dep_patterns: &["@axiomhq/", "axiom"],
        markers: &[],
    },
    // ── AI ────────────────────────────────────────────────────────────
    Rule {
        id: "openai",
        name: "OpenAI",
        category: ServiceCategory::Ai,
        dashboard: Some("https://platform.openai.com"),
        docs: None,
        brand: 0x10a37f,
        monogram: "AI",
        dep_patterns: &["openai"],
        markers: &[],
    },
    Rule {
        id: "anthropic",
        name: "Anthropic",
        category: ServiceCategory::Ai,
        dashboard: Some("https://console.anthropic.com"),
        docs: None,
        brand: 0xd97757,
        monogram: "An",
        dep_patterns: &["@anthropic-ai/"],
        markers: &[],
    },
    // ── Search / media / i18n ─────────────────────────────────────────
    Rule {
        id: "algolia",
        name: "Algolia",
        category: ServiceCategory::Search,
        dashboard: Some("https://dashboard.algolia.com"),
        docs: None,
        brand: 0x5468ff,
        monogram: "Ag",
        dep_patterns: &["algoliasearch"],
        markers: &[],
    },
    Rule {
        id: "cloudinary",
        name: "Cloudinary",
        category: ServiceCategory::Media,
        dashboard: Some("https://console.cloudinary.com"),
        docs: None,
        brand: 0x3448c5,
        monogram: "Cl",
        dep_patterns: &["cloudinary"],
        markers: &[],
    },
    Rule {
        id: "tolgee",
        name: "Tolgee",
        category: ServiceCategory::Localization,
        dashboard: Some("https://app.tolgee.io"),
        docs: None,
        brand: 0x64748b,
        monogram: "Tg",
        dep_patterns: &["@tolgee/"],
        markers: &[],
    },
    // ── Infra / framework ─────────────────────────────────────────────
    Rule {
        id: "docker",
        name: "Docker",
        category: ServiceCategory::Infra,
        dashboard: Some("https://hub.docker.com"),
        docs: None,
        brand: 0x2496ed,
        monogram: "Dk",
        dep_patterns: &[],
        markers: &["Dockerfile", "docker-compose.yml", "docker-compose.yaml"],
    },
    Rule {
        id: "aws",
        name: "AWS",
        category: ServiceCategory::Infra,
        dashboard: Some("https://console.aws.amazon.com"),
        docs: None,
        brand: 0xff9900,
        monogram: "AWS",
        dep_patterns: &["aws-sdk", "@aws-sdk/"],
        markers: &[],
    },
    Rule {
        id: "nextjs",
        name: "Next.js",
        category: ServiceCategory::Framework,
        dashboard: None,
        docs: Some("https://nextjs.org/docs"),
        brand: 0x2b2b2b,
        monogram: "N",
        dep_patterns: &["next"],
        markers: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(dir.join(name).parent().unwrap()).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn dep_matching_is_exact_or_scope_prefix() {
        assert!(dep_matches("next", "next"));
        assert!(!dep_matches("next-auth", "next")); // exact only, no false Next.js
        assert!(dep_matches(
            "@react-native-firebase/auth",
            "@react-native-firebase/"
        ));
        assert!(!dep_matches("firebaseui", "firebase"));
    }

    #[test]
    fn detects_monorepo_sub_apps_and_services() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Root-level deploy config.
        write(root, "render.yaml", "services: []\n");
        // Server sub-app.
        write(
            root,
            "server/package.json",
            r#"{"dependencies":{"mongoose":"^8","stripe":"^14","firebase-admin":"^12"}}"#,
        );
        // Mobile sub-app with native folders.
        write(
            root,
            "client/AppTemplate/package.json",
            r#"{"dependencies":{"react-native-purchases":"^7","@amplitude/analytics-react-native":"^1"}}"#,
        );
        std::fs::create_dir_all(root.join("client/AppTemplate/ios")).unwrap();
        std::fs::create_dir_all(root.join("client/AppTemplate/android")).unwrap();

        let subs = detect_project_services(root);
        let names: Vec<&str> = subs.iter().map(|s| s.rel_path.as_str()).collect();
        assert!(names.contains(&"."), "root sub-app present: {names:?}");
        assert!(names.iter().any(|n| n.contains("server")));
        assert!(names.iter().any(|n| n.contains("AppTemplate")));

        let root_ids = ids_for(&subs, ".");
        assert!(root_ids.contains(&"render".to_string()));

        let server_ids = ids_for_contains(&subs, "server");
        assert!(server_ids.contains(&"mongodb".to_string()));
        assert!(server_ids.contains(&"stripe".to_string()));
        assert!(server_ids.contains(&"firebase".to_string()));

        let app_ids = ids_for_contains(&subs, "AppTemplate");
        assert!(app_ids.contains(&"revenuecat".to_string()));
        assert!(app_ids.contains(&"amplitude".to_string()));
        assert!(app_ids.contains(&"appstore".to_string()));
        assert!(app_ids.contains(&"googleplay".to_string()));
    }

    #[test]
    fn environment_inventory_does_not_run_integration_detection() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{"dependencies":{"stripe":"^14","@amplitude/analytics-browser":"^2"}}"#,
        );
        write(root, ".env.local", "API_URL=https://example.test\n");

        let environment = detect_project_environment_files(root);

        assert_eq!(environment.len(), 1);
        assert_eq!(environment[0].rel_path, ".");
        assert_eq!(environment[0].env_files, vec![".env.local"]);
        assert!(environment[0].services.is_empty());
        assert!(detect_project_services(root)[0]
            .services
            .iter()
            .any(|service| service.id == "stripe"));
    }

    fn ids_for(subs: &[SubAppServices], rel: &str) -> Vec<String> {
        subs.iter()
            .find(|s| s.rel_path == rel)
            .map(|s| s.services.iter().map(|x| x.id.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn set_env_value_preserves_the_rest_of_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".env");
        std::fs::write(
            &path,
            "# header comment\nAPI_URL=http://old\n\nexport TOKEN=abc\n",
        )
        .unwrap();

        // Edit existing (with quoting for a value that needs it).
        set_env_value(&path, "API_URL", "https://new host").unwrap();
        // Edit an exported key (export prefix preserved).
        set_env_value(&path, "TOKEN", "xyz").unwrap();
        // Add a brand-new key.
        set_env_value(&path, "NEW_KEY", "42").unwrap();

        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("# header comment"), "comment kept: {out}");
        assert!(
            out.contains("API_URL=\"https://new host\""),
            "quoted: {out}"
        );
        assert!(out.contains("export TOKEN=xyz"), "export kept: {out}");
        assert!(out.contains("NEW_KEY=42"), "appended: {out}");

        // Re-parsing yields the new values.
        let parsed = read_env_file(&path);
        let get = |k: &str| {
            parsed
                .entries
                .iter()
                .find(|e| e.key == k)
                .map(|e| e.value.as_str())
        };
        assert_eq!(get("API_URL"), Some("https://new host"));
        assert_eq!(get("TOKEN"), Some("xyz"));
        assert_eq!(get("NEW_KEY"), Some("42"));
    }

    fn ids_for_contains(subs: &[SubAppServices], needle: &str) -> Vec<String> {
        subs.iter()
            .find(|s| s.rel_path.contains(needle))
            .map(|s| s.services.iter().map(|x| x.id.clone()).collect())
            .unwrap_or_default()
    }
}
