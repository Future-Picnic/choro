//! Trusted editor wrapper and a serial, process-isolated thumbnail renderer.
use ide_core::studio::*;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

static RENDER_LOCK: Mutex<()> = Mutex::new(());
static URGENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct Urgent;
impl Drop for Urgent {
    fn drop(&mut self) {
        URGENT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

static VISIBLE: OnceLock<Mutex<(Uuid, Vec<Uuid>)>> = OnceLock::new();
pub fn prioritize(design: Uuid, screens: Vec<Uuid>) {
    *VISIBLE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = (design, screens);
}
static MESSAGES: OnceLock<Mutex<Vec<Value>>> = OnceLock::new();
pub fn enqueue(message: Value) {
    let mut messages = MESSAGES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if messages.len() < 256 {
        messages.push(message);
    }
}
pub fn drain() -> Vec<Value> {
    std::mem::take(
        &mut *MESSAGES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    )
}

/// Choro theme roles for Studio's embedded web chrome (editor and canvas).
/// Every value is a CSS color except `scheme`, which selects the light or dark
/// form-control and Bootstrap variants.
pub fn web_theme(cx: &gpui::App) -> Value {
    use crate::ui::design;
    use gpui_component::ActiveTheme as _;
    let css = |color: gpui::Hsla| color.to_string();
    json!({
        "bg": css(design::base(cx)),
        "panel": css(design::nav(cx)),
        "stage": css(design::stage(cx)),
        "track": css(design::track(cx)),
        "seg": css(design::track_choice(cx)),
        "field": css(design::field_well(cx)),
        "surface": css(design::surface(cx)),
        "raised": css(design::surface_2(cx)),
        "float": css(design::focus(cx)),
        "text": css(design::t1(cx)),
        "muted": css(design::t2(cx)),
        "faint": css(design::t3(cx)),
        "line": css(design::line(cx)),
        "line2": css(design::line_2(cx)),
        "accent": css(design::accent(cx)),
        "ink": css(design::accent_ink(design::base(cx), cx)),
        "danger": css(design::rose(cx)),
        "ok": css(design::sage(cx)),
        "warn": css(design::amber(cx)),
        "scheme": if cx.theme().is_dark() { "dark" } else { "light" },
    })
}

#[derive(Clone, Copy)]
pub enum EditorSurface { Screen, Canvas, Prototype }

pub fn document(
    store: &StudioStore,
    design: &StudioDesign,
    screen_id: Uuid,
    session: Uuid,
    thumbnail: bool,
    preview_mode: bool,
    theme: Value,
) -> anyhow::Result<String> {
    document_for_surface(store, design, screen_id, session, thumbnail, preview_mode, theme, EditorSurface::Screen)
}

pub fn document_for_surface(
    store: &StudioStore,
    design: &StudioDesign,
    screen_id: Uuid,
    session: Uuid,
    thumbnail: bool,
    preview_mode: bool,
    theme: Value,
    surface: EditorSurface,
) -> anyhow::Result<String> {
    use base64::Engine;
    let screen = design
        .manifest
        .screens
        .iter()
        .find(|s| s.id == screen_id)
        .ok_or_else(|| anyhow::anyhow!("Screen does not exist"))?;
    let mut assets = serde_json::Map::new();
    for (path, bytes) in store
        .saved_revision(
            design.manifest.id,
            design.manifest.revision,
            &design.fingerprint,
        )?
        .assets
    {
        let mime = match path
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_lowercase()
            .as_str()
        {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "woff2" => "font/woff2",
            "woff" => "font/woff",
            "ttf" => "font/ttf",
            "otf" => "font/otf",
            _ => continue,
        };
        assets.insert(
            path,
            json!(format!(
                "data:{mime};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )),
        );
    }
    let draft = if thumbnail {
        None
    } else {
        std::fs::read(store.cache.join("drafts").join(format!("{screen_id}.json")))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .filter(|v| {
                v["dirty"] == true
                    && v["document"]
                        != serde_json::to_value(design.documents.get(&screen_id))
                            .unwrap_or(Value::Null)
            })
    };
    let bootstrap = json!({"inline":matches!(surface,EditorSurface::Canvas),"prototype":matches!(surface,EditorSurface::Prototype),"system_specimen":design.manifest.system_workspace,"draft":draft,"session":session,"document":design.documents.get(&screen_id),"revision":design.manifest.revision,"fingerprint":design.fingerprint,
        "screen_id":screen_id,"overrides":design.overrides.tokens,"tokens":design.tokens(),"tokens_css":design.tokens_css(),"screens":design.manifest.screens,"width":screen.width,"height":screen.height,"assets":assets,"thumbnail":thumbnail,"mode":if preview_mode && !thumbnail { "preview" } else { "edit" },"theme":theme});
    // Bootstrap is data, never executable HTML supplied by a design.
    let bootstrap = serde_json::to_string(&bootstrap)?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    let vendor = format!(
        "window.__CHORO_STUDIO__={bootstrap};\n{}",
        [
            include_str!("../../../assets/studio/vendor/upstream/popper.min.js"),
            include_str!("../../../assets/studio/vendor/upstream/bootstrap.min.js"),
            include_str!("../../../assets/studio/vendor/upstream/builder.js"),
            include_str!("../../../assets/studio/vendor/templates.js"),
            include_str!("../../../assets/studio/vendor/upstream/undo.js"),
            include_str!("../../../assets/studio/vendor/upstream/inputs.js"),
            include_str!("../../../assets/studio/vendor/upstream/autocomplete.js"),
            include_str!("../../../assets/studio/vendor/upstream/components-common.js"),
            include_str!("../../../assets/studio/vendor/upstream/components-html.js"),
            include_str!("../../../assets/studio/vendor/upstream/coloris.js"),
        ]
        .join("\n")
    );
    let css = [
        include_str!("../../../assets/studio/vendor/upstream/editor.css"),
        include_str!("../../../assets/studio/vendor/fonts.css"),
        include_str!("../../../assets/studio/vendor/upstream/coloris.min.css"),
    ]
    .join("\n");
    Ok(include_str!("../../../assets/studio/editor.html")
        .replace("/*STUDIO_VENDOR_CSS*/", &css)
        .replace(
            "<!--STUDIO_RIGHT_PANEL-->",
            include_str!("../../../assets/studio/vendor/upstream/right-panel.html"),
        )
        .replace(
            "<!--STUDIO_INLINE_TOOLBAR-->",
            include_str!("../../../assets/studio/vendor/upstream/inline-toolbar.html"),
        )
        .replace(
            "/*STUDIO_VENDOR*/",
            &vendor.replace("</script", "<\\/script"),
        )
        .replace(
            "/*STUDIO_EDITOR*/",
            include_str!("../../../assets/studio/editor.js"),
        ))
}

// One trusted editor session per canvas. Only its parent canvas may relay mutations.
static INLINE: OnceLock<Mutex<std::collections::HashMap<Uuid, Uuid>>> = OnceLock::new();
pub fn register_inline(canvas: Uuid, editor: Uuid) {
    INLINE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).insert(canvas, editor);
}
pub fn revoke_inline(canvas: Uuid) {
    INLINE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).remove(&canvas);
}
pub fn enqueue_inline(canvas: Uuid, raw: &str) -> bool {
    if raw.len() > 12 * 1024 * 1024 { return false; }
    let Ok(value) = serde_json::from_str::<Value>(raw) else { return false; };
    if value["type"] != "inline-editor" || value["session"].as_str().and_then(|s|s.parse::<Uuid>().ok()) != Some(canvas) { return false; }
    let entries = INLINE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    let Some(editor) = entries.get(&canvas) else { return false; };
    let message = &value["message"];
    if message["session"].as_str().and_then(|s|s.parse::<Uuid>().ok()) != Some(*editor)
        || !matches!(message["type"].as_str(), Some("ready" | "dirty" | "save" | "recover" | "asset" | "selection" | "render-error" | "flushed")) { return false; }
    enqueue(message.clone()); true
}

pub fn export_png(
    store: StudioStore,
    mut design: StudioDesign,
    screen: Uuid,
) -> anyhow::Result<Vec<u8>> {
    design.manifest.screens.retain(|s| s.id == screen);
    anyhow::ensure!(!design.manifest.screens.is_empty(), "Screen does not exist");
    design.manifest.screens[0].archived = false;
    let path = render_path(&store, &design, screen, false);
    render_images(store, design, false)?;
    Ok(std::fs::read(path)?)
}

pub fn render_thumbnails(store: StudioStore, design: StudioDesign) -> anyhow::Result<()> {
    render_images(store, design, true)
}

fn render_images(
    store: StudioStore,
    design: StudioDesign,
    update_index: bool,
) -> anyhow::Result<()> {
    URGENT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let _urgent = Urgent;
    let _serial = RENDER_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Thumbnail worker stopped"))?;
    #[cfg(target_os = "macos")]
    {
        stop_canvas_worker();
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::fs::PermissionsExt;
        use std::process::{Command, Stdio};
        let mut pending = design
            .manifest
            .screens
            .iter()
            .filter(|s| !s.archived && !render_path(&store, &design, s.id, update_index).exists())
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return Ok(());
        }
        let binary = include_bytes!(concat!(env!("OUT_DIR"), "/choro-studio-thumbnail"));
        let bundled = std::env::current_exe()?.with_file_name("choro-studio-thumbnail");
        let helper = if bundled.is_file() {
            bundled
        } else {
            store
                .cache
                .join(format!("thumbnail-helper-{}", hash(binary)))
        };
        if !helper.exists() {
            atomic(&helper, binary)?;
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))?;
        }
        let mut child = Command::new(helper)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let result = (|| -> anyhow::Result<()> {
            let mut input = child
                .stdin
                .take()
                .ok_or_else(|| anyhow::anyhow!("Missing thumbnail input"))?;
            let output = BufReader::new(
                child
                    .stdout
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("Missing thumbnail output"))?,
            );
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for line in output.lines() {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
            let mut failures = Vec::new();
            while !pending.is_empty() {
                let priority = VISIBLE
                    .get_or_init(Default::default)
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let index = if priority.0 == design.manifest.id {
                    pending
                        .iter()
                        .position(|s| priority.1.contains(&s.id))
                        .unwrap_or(0)
                } else {
                    0
                };
                let screen = pending.remove(index);
                let html = document(
                    &store,
                    &design,
                    screen.id,
                    Uuid::new_v4(),
                    true,
                    false,
                    Value::Null,
                )?;
                let job = json!({"html":html,"output":render_path(&store,&design,screen.id,update_index),"width":screen.width,"height":screen.height,"full_resolution":!update_index});
                writeln!(input, "{job}")?;
                input.flush()?;
                let line = receiver
                    .recv_timeout(std::time::Duration::from_secs(12))
                    .map_err(|_| {
                        anyhow::anyhow!("Thumbnail helper stopped responding; retry rendering")
                    })??;
                let response: Value = serde_json::from_str(&line)?;
                if let Some(error) = response.get("error").and_then(Value::as_str) {
                    failures.push(error.to_string());
                } else if update_index {
                    let path = store.thumbnail_path(&design, screen.id);
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        atomic(
                            &store
                                .cache
                                .join("thumbnails")
                                .join(format!("latest-{}.json", screen.id)),
                            &serde_json::to_vec(name)?,
                        )?;
                    }
                }
            }
            drop(input);
            anyhow::ensure!(
                failures.is_empty(),
                "{} screen previews could not render: {}",
                failures.len(),
                failures.join("; ")
            );
            Ok(())
        })();
        for _ in 0..10 {
            if child.try_wait()?.is_some() {
                return result;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
        result
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (store, design);
        anyhow::bail!("Studio thumbnails require macOS WebKit")
    }
}

fn render_path(
    store: &StudioStore,
    design: &StudioDesign,
    screen: Uuid,
    thumbnail: bool,
) -> std::path::PathBuf {
    let path = store.thumbnail_path(design, screen);
    if thumbnail {
        path
    } else {
        store.cache.join("exports").join(path.file_name().unwrap())
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn inline_editor_grants_are_session_scoped_and_revocable() {
        let canvas=Uuid::new_v4();let editor=Uuid::new_v4();let next=Uuid::new_v4();
        let payload=|session,kind|json!({"session":canvas,"type":"inline-editor","message":{"session":session,"type":kind}}).to_string();
        assert!(!enqueue_inline(canvas,&payload(editor,"ready")));
        register_inline(canvas,editor);
        assert!(enqueue_inline(canvas,&payload(editor,"ready")));
        assert!(!enqueue_inline(Uuid::new_v4(),&payload(editor,"save")));
        assert!(!enqueue_inline(canvas,&payload(next,"save")));
        assert!(!enqueue_inline(canvas,&payload(editor,"navigate")));
        register_inline(canvas,next);
        assert!(!enqueue_inline(canvas,&payload(editor,"save")));
        assert!(enqueue_inline(canvas,&payload(next,"selection")));
        revoke_inline(canvas);
        assert!(!enqueue_inline(canvas,&payload(next,"save")));
    }

    // These integration tests exercise the same process-wide priority counter
    // and helper. Their independent scenarios must not become each other's
    // urgent export requests when Rust runs tests in parallel.
    static RENDER_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn canvas_renderer_reuses_one_helper_and_yields_to_exports() {
        let _test = RENDER_TEST_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("choro-canvas-renderer-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("project")).unwrap();
        let store = StudioStore::new(root.join("project"), root.join("data")).unwrap();
        let design = store.create("Canvas renderer fixture").unwrap();
        let screen = design.manifest.screens[0].id;
        URGENT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        {
            let _urgent = Urgent;
            assert!(canvas_preview(&store, &design, screen, 256)
                .unwrap()
                .is_none());
        }
        let first = canvas_preview(&store, &design, screen, 256)
            .unwrap()
            .unwrap();
        assert_eq!(png_size(&first), Some((256, 171)));
        let pid = CANVAS_WORKER
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .child
            .id();
        let second = canvas_preview(&store, &design, screen, 512)
            .unwrap()
            .unwrap();
        assert_eq!(png_size(&second), Some((512, 341)));
        assert_eq!(
            CANVAS_WORKER
                .get()
                .unwrap()
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .child
                .id(),
            pid
        );
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(CANVAS_WORKER.get().unwrap().lock().unwrap().is_none());
        assert_eq!(store.load(design.manifest.id).unwrap(), design);
    }
    #[test]
    fn png_export_uses_viewing_size_without_changing_design_or_overview() {
        let _test = RENDER_TEST_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("choro-studio-export-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("project")).unwrap();
        let store = StudioStore::new(root.join("project"), root.join("data")).unwrap();
        let design = store.create("Export fixture").unwrap();
        let screen = design.manifest.screens[0].id;
        let mut mobile = design.clone();
        mobile.manifest.screens[0].width = 390;
        mobile.manifest.screens[0].height = 844;
        // Populate the overview first: exporting must not reuse this smaller PNG.
        render_thumbnails(store.clone(), mobile.clone()).unwrap();
        let overview = store.last_thumbnail(screen).unwrap();
        let png = export_png(store.clone(), mobile, screen).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 390);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 844);
        assert_eq!(store.load(design.manifest.id).unwrap(), design);
        assert_eq!(store.last_thumbnail(screen).unwrap(), overview);
        assert!(export_png(store, design, Uuid::new_v4()).is_err());
    }
}

/// One bounded preview job. Explicit exports/review batches acquire priority.
pub fn canvas_preview(
    store: &StudioStore,
    design: &StudioDesign,
    screen: Uuid,
    tier: u32,
) -> anyhow::Result<Option<Vec<u8>>> {
    use std::sync::atomic::Ordering;
    anyhow::ensure!(
        [256, 512, 1024, 2048].contains(&tier),
        "Invalid canvas tier"
    );
    if URGENT.load(Ordering::SeqCst) > 0 {
        return Ok(None);
    }
    let _serial = RENDER_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Renderer stopped"))?;
    if URGENT.load(Ordering::SeqCst) > 0 {
        return Ok(None);
    }
    let screen = design
        .manifest
        .screens
        .iter()
        .find(|s| s.id == screen && !s.archived)
        .ok_or_else(|| anyhow::anyhow!("Screen unavailable"))?;
    if let Ok(bytes) = std::fs::read(store.thumbnail_path(design, screen.id)) {
        if png_size(&bytes).is_some_and(|(w, h)| w.max(h) <= tier && w.max(h) >= tier / 2) {
            return Ok(Some(bytes));
        }
    }
    #[cfg(target_os = "macos")]
    {
        let scale = tier as f64 / screen.width.max(screen.height) as f64;
        let width = (screen.width as f64 * scale).round().max(1.) as u32;
        let height = (screen.height as f64 * scale).round().max(1.) as u32;
        let output = store.cache.join("canvas-worker.png");
        let html = document(
            store,
            design,
            screen.id,
            Uuid::new_v4(),
            true,
            false,
            Value::Null,
        )?;
        let job = json!({"html":html,"output":output,"width":screen.width,"height":screen.height,"output_width":width,"output_height":height});
        let mut holder = CANVAS_WORKER
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if holder
            .as_mut()
            .is_some_and(|w| w.child.try_wait().ok().flatten().is_some())
        {
            holder.take();
        }
        if holder.is_none() {
            *holder = Some(CanvasWorker::new(store)?);
        }
        let result = holder.as_mut().unwrap().render(job);
        if result.is_err() {
            holder.take();
        }
        result?;
        let bytes = std::fs::read(output)?;
        anyhow::ensure!(
            png_size(&bytes) == Some((width, height)),
            "Incorrect preview dimensions"
        );
        return Ok(Some(bytes));
    }
    #[cfg(not(target_os = "macos"))]
    anyhow::bail!("Studio canvas requires macOS");
}
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

#[cfg(target_os = "macos")]
static CANVAS_WORKER: OnceLock<Mutex<Option<CanvasWorker>>> = OnceLock::new();
#[cfg(target_os = "macos")]
struct CanvasWorker {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::sync::mpsc::Receiver<std::io::Result<String>>,
    used: std::time::Instant,
}
#[cfg(target_os = "macos")]
impl CanvasWorker {
    fn new(store: &StudioStore) -> anyhow::Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        use std::{
            io::{BufRead, BufReader},
            process::{Command, Stdio},
        };
        let binary = include_bytes!(concat!(env!("OUT_DIR"), "/choro-studio-thumbnail"));
        let bundled = std::env::current_exe()?.with_file_name("choro-studio-thumbnail");
        let helper = if bundled.is_file() {
            bundled
        } else {
            store
                .cache
                .join(format!("thumbnail-helper-{}", hash(binary)))
        };
        if !helper.exists() {
            atomic(&helper, binary)?;
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))?;
        }
        let mut child = Command::new(helper)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let input = child.stdin.take();
        let output = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| anyhow::anyhow!("Missing renderer output"))?,
        );
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in output.lines() {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        // Reap the helper once its queue has drained. All locking uses the same
        // order as render jobs; cleanup never blocks the app's main thread.
        std::thread::spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let Ok(_serial) = RENDER_LOCK.try_lock() else {
                continue;
            };
            let mut worker = CANVAS_WORKER
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            match worker.as_ref() {
                None => break,
                Some(w) if w.used.elapsed() > std::time::Duration::from_millis(500) => {
                    worker.take();
                    break;
                }
                _ => {}
            }
        });
        Ok(Self {
            child,
            input,
            output: rx,
            used: std::time::Instant::now(),
        })
    }
    fn render(&mut self, job: Value) -> anyhow::Result<()> {
        use std::io::Write;
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Missing renderer input"))?;
        writeln!(input, "{job}")?;
        input.flush()?;
        let line = self
            .output
            .recv_timeout(std::time::Duration::from_secs(12))
            .map_err(|_| anyhow::anyhow!("Preview timed out; refresh to retry"))??;
        let reply: Value = serde_json::from_str(&line)?;
        self.used = std::time::Instant::now();
        if let Some(error) = reply["error"].as_str() {
            anyhow::bail!("{error}");
        }
        Ok(())
    }
}
#[cfg(target_os = "macos")]
impl Drop for CanvasWorker {
    fn drop(&mut self) {
        self.input.take();
        for _ in 0..10 {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[cfg(target_os = "macos")]
fn stop_canvas_worker() {
    CANVAS_WORKER
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
}
