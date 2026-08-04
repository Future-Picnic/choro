use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub struct AppAssets;

impl AppAssets {
    const APP_ASSETS: [(&'static str, &'static [u8]); 35] = [
        (
            "icons/database.svg",
            include_bytes!("../assets/icons/database.svg"),
        ),
        (
            "icons/file-text.svg",
            include_bytes!("../assets/icons/file-text.svg"),
        ),
        (
            "icons/image.svg",
            include_bytes!("../assets/icons/image.svg"),
        ),
        (
            "icons/network.svg",
            include_bytes!("../assets/icons/network.svg"),
        ),
        (
            "icons/phone.svg",
            include_bytes!("../assets/icons/phone.svg"),
        ),
        (
            "icons/microphone.svg",
            include_bytes!("../assets/icons/microphone.svg"),
        ),
        (
            "icons/asset-figma.svg",
            include_bytes!("../assets/icons/figma.svg"),
        ),
        (
            "icons/asset-url.svg",
            include_bytes!("../assets/icons/url.svg"),
        ),
        (
            "icons/asset-file.svg",
            include_bytes!("../assets/icons/file.svg"),
        ),
        (
            "icons/asset-image.svg",
            include_bytes!("../assets/icons/asset-image.svg"),
        ),
        ("brand/jira.svg", include_bytes!("../assets/brand/jira.svg")),
        (
            "brand/linear.svg",
            include_bytes!("../assets/brand/linear.svg"),
        ),
        (
            "brand/asana.svg",
            include_bytes!("../assets/brand/asana.svg"),
        ),
        (
            "brand/clickup.svg",
            include_bytes!("../assets/brand/clickup.svg"),
        ),
        (
            "agent-icons/openai.svg",
            include_bytes!("../assets/agent-icons/openai.svg"),
        ),
        (
            "agent-icons/claude.svg",
            include_bytes!("../assets/agent-icons/claude.svg"),
        ),
        (
            "agent-icons/pencil.svg",
            include_bytes!("../assets/agent-icons/pencil.svg"),
        ),
        (
            "agent-icons/model.svg",
            include_bytes!("../assets/agent-icons/model.svg"),
        ),
        (
            "agent-icons/opencode.svg",
            include_bytes!("../assets/agent-icons/opencode.svg"),
        ),
        (
            "brand/choro-riff.svg",
            include_bytes!("../assets/brand/choro-riff.svg"),
        ),
        (
            "icons/operations-icon.svg",
            include_bytes!("../assets/icons/operations-icon.svg"),
        ),
        (
            "icons/add-row.svg",
            include_bytes!("../assets/icons/add-row.svg"),
        ),
        ("icons/play.svg", include_bytes!("../assets/icons/play.svg")),
        (
            "icons/circle.svg",
            include_bytes!("../assets/icons/circle.svg"),
        ),
        (
            "icons/circle-dashed.svg",
            include_bytes!("../assets/icons/circle-dashed.svg"),
        ),
        (
            "icons/file-diff.svg",
            include_bytes!("../assets/icons/file-diff.svg"),
        ),
        (
            "icons/target.svg",
            include_bytes!("../assets/icons/target.svg"),
        ),
        (
            "icons/branch.svg",
            include_bytes!("../assets/icons/branch.svg"),
        ),
        (
            "icons/wallpaper.svg",
            include_bytes!("../assets/icons/wallpaper.svg"),
        ),
        (
            "icon/workspace/folder.svg",
            include_bytes!("../../../vendor/velotype/assets/icon/workspace/folder.svg"),
        ),
        (
            "icon/workspace/markdown.svg",
            include_bytes!("../../../vendor/velotype/assets/icon/workspace/markdown.svg"),
        ),
        (
            "icon/titlebar/chrome-close.svg",
            include_bytes!("../../../vendor/velotype/assets/icon/titlebar/chrome-close.svg"),
        ),
        (
            "icon/titlebar/chrome-minimize.svg",
            include_bytes!("../../../vendor/velotype/assets/icon/titlebar/chrome-minimize.svg"),
        ),
        (
            "icon/titlebar/chrome-maximize.svg",
            include_bytes!("../../../vendor/velotype/assets/icon/titlebar/chrome-maximize.svg"),
        ),
        (
            "icon/titlebar/chrome-restore.svg",
            include_bytes!("../../../vendor/velotype/assets/icon/titlebar/chrome-restore.svg"),
        ),
    ];
}

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        // Runtime theme-colored empty-state illustrations, generated per theme and
        // registered by the UI (see `crate::ui::illustrations`).
        if path.starts_with("illustrations/themed/") {
            return Ok(crate::ui::illustrations::themed_svg(path).map(Cow::Owned));
        }

        // User-imported, monochrome project SVGs. The project visuals module
        // validates and registers their normalized bytes before rendering.
        if path.starts_with("project-icons/") {
            return Ok(crate::ui::project_visuals::custom_project_svg(path).map(Cow::Owned));
        }

        if let Some((_, bytes)) = Self::APP_ASSETS
            .iter()
            .find(|(asset_path, _)| *asset_path == path)
        {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }

        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = gpui_component_assets::Assets.list(path)?;
        assets.extend(
            Self::APP_ASSETS
                .iter()
                .filter(|(asset_path, _)| asset_path.starts_with(path))
                .map(|(asset_path, _)| SharedString::from(*asset_path)),
        );
        Ok(assets)
    }
}
