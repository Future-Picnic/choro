use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub struct AppAssets;

impl AppAssets {
    const APP_ASSETS: [(&'static str, &'static [u8]); 119] = [
        (
            "avatar/choro-companion-idle.webp",
            include_bytes!("../assets/avatar/exports/v22/idle/choro-companion-idle.webp"),
        ),
        (
            "avatar/choro-companion-working.webp",
            include_bytes!("../assets/avatar/exports/v22/working/choro-companion-working.webp"),
        ),
        (
            "avatar/choro-companion-working-medium-0.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/working/choro-companion-working-medium-0.webp"),
        ),
        (
            "avatar/choro-companion-working-medium-1.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/working/choro-companion-working-medium-1.webp"),
        ),
        (
            "avatar/choro-companion-working-medium-2.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/working/choro-companion-working-medium-2.webp"),
        ),
        (
            "avatar/choro-companion-working-medium-3.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/working/choro-companion-working-medium-3.webp"),
        ),
        (
            "avatar/choro-companion-working-medium-4.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/working/choro-companion-working-medium-4.webp"),
        ),
        (
            "avatar/choro-companion-working-medium-5.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/working/choro-companion-working-medium-5.webp"),
        ),
        (
            "avatar/choro-companion-working-medium-6.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/working/choro-companion-working-medium-6.webp"),
        ),
        (
            "avatar/choro-companion-working-medium-7.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/working/choro-companion-working-medium-7.webp"),
        ),
        (
            "avatar/choro-companion-needs-attention.webp",
            include_bytes!(
                "../assets/avatar/exports/v22/needs-attention/choro-companion-needs-attention.webp"
            ),
        ),
        (
            "avatar/choro-companion-needs-attention-medium-0.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/needs-attention/choro-companion-needs-attention-medium-0.webp"),
        ),
        (
            "avatar/choro-companion-needs-attention-medium-1.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/needs-attention/choro-companion-needs-attention-medium-1.webp"),
        ),
        (
            "avatar/choro-companion-needs-attention-medium-2.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/needs-attention/choro-companion-needs-attention-medium-2.webp"),
        ),
        (
            "avatar/choro-companion-needs-attention-medium-3.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/needs-attention/choro-companion-needs-attention-medium-3.webp"),
        ),
        (
            "avatar/choro-companion-needs-attention-medium-4.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/needs-attention/choro-companion-needs-attention-medium-4.webp"),
        ),
        (
            "avatar/choro-companion-needs-attention-medium-5.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/needs-attention/choro-companion-needs-attention-medium-5.webp"),
        ),
        (
            "avatar/choro-companion-needs-attention-medium-6.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/needs-attention/choro-companion-needs-attention-medium-6.webp"),
        ),
        (
            "avatar/choro-companion-needs-attention-medium-7.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/needs-attention/choro-companion-needs-attention-medium-7.webp"),
        ),
        (
            "avatar/choro-companion-done.webp",
            include_bytes!("../assets/avatar/exports/v27/done/choro-companion-done.webp"),
        ),
        (
            "avatar/choro-companion-done-medium-0.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/done/choro-companion-done-medium-0.webp"),
        ),
        (
            "avatar/choro-companion-done-medium-1.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/done/choro-companion-done-medium-1.webp"),
        ),
        (
            "avatar/choro-companion-done-medium-2.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/done/choro-companion-done-medium-2.webp"),
        ),
        (
            "avatar/choro-companion-done-medium-3.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/done/choro-companion-done-medium-3.webp"),
        ),
        (
            "avatar/choro-companion-done-medium-4.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/done/choro-companion-done-medium-4.webp"),
        ),
        (
            "avatar/choro-companion-done-medium-5.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/done/choro-companion-done-medium-5.webp"),
        ),
        (
            "avatar/choro-companion-done-medium-6.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/done/choro-companion-done-medium-6.webp"),
        ),
        (
            "avatar/choro-companion-done-medium-7.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/done/choro-companion-done-medium-7.webp"),
        ),
        (
            "avatar/choro-companion-music-deep-focus.webp",
            include_bytes!(
                "../assets/avatar/exports/v25/deep-focus/choro-companion-music-deep-focus.webp"
            ),
        ),
        (
            "avatar/choro-companion-music-lofi-flow.webp",
            include_bytes!(
                "../assets/avatar/exports/v25/lofi-flow/choro-companion-music-lofi-flow.webp"
            ),
        ),
        (
            "avatar/choro-companion-music-calm.webp",
            include_bytes!("../assets/avatar/exports/v25/calm/choro-companion-music-calm.webp"),
        ),
        (
            "avatar/choro-companion-music-high-energy.webp",
            include_bytes!(
                "../assets/avatar/exports/v25/high-energy/choro-companion-music-high-energy.webp"
            ),
        ),
        (
            "avatar/choro-companion-idle-medium-0.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/idle/choro-companion-idle-medium-0.webp"),
        ),
        (
            "avatar/choro-companion-idle-medium-1.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/idle/choro-companion-idle-medium-1.webp"),
        ),
        (
            "avatar/choro-companion-idle-medium-2.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/idle/choro-companion-idle-medium-2.webp"),
        ),
        (
            "avatar/choro-companion-idle-medium-3.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/idle/choro-companion-idle-medium-3.webp"),
        ),
        (
            "avatar/choro-companion-idle-medium-4.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/idle/choro-companion-idle-medium-4.webp"),
        ),
        (
            "avatar/choro-companion-idle-medium-5.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/idle/choro-companion-idle-medium-5.webp"),
        ),
        (
            "avatar/choro-companion-idle-medium-6.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/idle/choro-companion-idle-medium-6.webp"),
        ),
        (
            "avatar/choro-companion-idle-medium-7.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/idle/choro-companion-idle-medium-7.webp"),
        ),
        (
            "avatar/choro-companion-music-deep-focus-medium-0.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/deep-focus/choro-companion-music-deep-focus-medium-0.webp"),
        ),
        (
            "avatar/choro-companion-music-deep-focus-medium-1.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/deep-focus/choro-companion-music-deep-focus-medium-1.webp"),
        ),
        (
            "avatar/choro-companion-music-deep-focus-medium-2.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/deep-focus/choro-companion-music-deep-focus-medium-2.webp"),
        ),
        (
            "avatar/choro-companion-music-deep-focus-medium-3.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/deep-focus/choro-companion-music-deep-focus-medium-3.webp"),
        ),
        (
            "avatar/choro-companion-music-deep-focus-medium-4.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/deep-focus/choro-companion-music-deep-focus-medium-4.webp"),
        ),
        (
            "avatar/choro-companion-music-deep-focus-medium-5.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/deep-focus/choro-companion-music-deep-focus-medium-5.webp"),
        ),
        (
            "avatar/choro-companion-music-deep-focus-medium-6.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/deep-focus/choro-companion-music-deep-focus-medium-6.webp"),
        ),
        (
            "avatar/choro-companion-music-deep-focus-medium-7.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/deep-focus/choro-companion-music-deep-focus-medium-7.webp"),
        ),
        (
            "avatar/choro-companion-music-lofi-flow-medium-0.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/lofi-flow/choro-companion-music-lofi-flow-medium-0.webp"),
        ),
        (
            "avatar/choro-companion-music-lofi-flow-medium-1.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/lofi-flow/choro-companion-music-lofi-flow-medium-1.webp"),
        ),
        (
            "avatar/choro-companion-music-lofi-flow-medium-2.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/lofi-flow/choro-companion-music-lofi-flow-medium-2.webp"),
        ),
        (
            "avatar/choro-companion-music-lofi-flow-medium-3.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/lofi-flow/choro-companion-music-lofi-flow-medium-3.webp"),
        ),
        (
            "avatar/choro-companion-music-lofi-flow-medium-4.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/lofi-flow/choro-companion-music-lofi-flow-medium-4.webp"),
        ),
        (
            "avatar/choro-companion-music-lofi-flow-medium-5.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/lofi-flow/choro-companion-music-lofi-flow-medium-5.webp"),
        ),
        (
            "avatar/choro-companion-music-lofi-flow-medium-6.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/lofi-flow/choro-companion-music-lofi-flow-medium-6.webp"),
        ),
        (
            "avatar/choro-companion-music-lofi-flow-medium-7.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/lofi-flow/choro-companion-music-lofi-flow-medium-7.webp"),
        ),
        (
            "avatar/choro-companion-music-calm-medium-0.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/calm/choro-companion-music-calm-medium-0.webp"),
        ),
        (
            "avatar/choro-companion-music-calm-medium-1.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/calm/choro-companion-music-calm-medium-1.webp"),
        ),
        (
            "avatar/choro-companion-music-calm-medium-2.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/calm/choro-companion-music-calm-medium-2.webp"),
        ),
        (
            "avatar/choro-companion-music-calm-medium-3.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/calm/choro-companion-music-calm-medium-3.webp"),
        ),
        (
            "avatar/choro-companion-music-calm-medium-4.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/calm/choro-companion-music-calm-medium-4.webp"),
        ),
        (
            "avatar/choro-companion-music-calm-medium-5.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/calm/choro-companion-music-calm-medium-5.webp"),
        ),
        (
            "avatar/choro-companion-music-calm-medium-6.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/calm/choro-companion-music-calm-medium-6.webp"),
        ),
        (
            "avatar/choro-companion-music-calm-medium-7.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/calm/choro-companion-music-calm-medium-7.webp"),
        ),
        (
            "avatar/choro-companion-music-high-energy-medium-0.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/high-energy/choro-companion-music-high-energy-medium-0.webp"),
        ),
        (
            "avatar/choro-companion-music-high-energy-medium-1.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/high-energy/choro-companion-music-high-energy-medium-1.webp"),
        ),
        (
            "avatar/choro-companion-music-high-energy-medium-2.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/high-energy/choro-companion-music-high-energy-medium-2.webp"),
        ),
        (
            "avatar/choro-companion-music-high-energy-medium-3.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/high-energy/choro-companion-music-high-energy-medium-3.webp"),
        ),
        (
            "avatar/choro-companion-music-high-energy-medium-4.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/high-energy/choro-companion-music-high-energy-medium-4.webp"),
        ),
        (
            "avatar/choro-companion-music-high-energy-medium-5.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/high-energy/choro-companion-music-high-energy-medium-5.webp"),
        ),
        (
            "avatar/choro-companion-music-high-energy-medium-6.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/high-energy/choro-companion-music-high-energy-medium-6.webp"),
        ),
        (
            "avatar/choro-companion-music-high-energy-medium-7.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/high-energy/choro-companion-music-high-energy-medium-7.webp"),
        ),
        (
            "avatar/choro-companion-agent-assistant.webp",
            include_bytes!(
                "../assets/avatar/exports/v28/agent-assistant/choro-companion-agent-assistant.webp"
            ),
        ),
        (
            "avatar/choro-companion-agent-assistant-medium-0.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/agent-assistant/choro-companion-agent-assistant-medium-0.webp"),
        ),
        (
            "avatar/choro-companion-agent-assistant-medium-1.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/agent-assistant/choro-companion-agent-assistant-medium-1.webp"),
        ),
        (
            "avatar/choro-companion-agent-assistant-medium-2.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/agent-assistant/choro-companion-agent-assistant-medium-2.webp"),
        ),
        (
            "avatar/choro-companion-agent-assistant-medium-3.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/agent-assistant/choro-companion-agent-assistant-medium-3.webp"),
        ),
        (
            "avatar/choro-companion-agent-assistant-medium-4.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/agent-assistant/choro-companion-agent-assistant-medium-4.webp"),
        ),
        (
            "avatar/choro-companion-agent-assistant-medium-5.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/agent-assistant/choro-companion-agent-assistant-medium-5.webp"),
        ),
        (
            "avatar/choro-companion-agent-assistant-medium-6.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/agent-assistant/choro-companion-agent-assistant-medium-6.webp"),
        ),
        (
            "avatar/choro-companion-agent-assistant-medium-7.webp",
            include_bytes!("../assets/avatar/exports/medium-motion/agent-assistant/choro-companion-agent-assistant-medium-7.webp"),
        ),
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
            "agent-icons/gemini.svg",
            include_bytes!("../assets/agent-icons/gemini.svg"),
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
            "brand/tumble.svg",
            include_bytes!("../assets/brand/tumble.svg"),
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
            "icons/pause.svg",
            include_bytes!("../assets/icons/pause.svg"),
        ),
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use gpui::AssetSource;
    use image::AnimationDecoder;

    use super::AppAssets;

    fn visible_bounds(image: &image::DynamicImage) -> Option<(u32, u32)> {
        let pixels = image.to_rgba8();
        let mut min_x = u32::MAX;
        let mut min_y = u32::MAX;
        let mut max_x = 0;
        let mut max_y = 0;
        let mut found = false;
        for (x, y, pixel) in pixels.enumerate_pixels() {
            if pixel.0[3] == 0 {
                continue;
            }
            found = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        found.then_some((max_x - min_x + 1, max_y - min_y + 1))
    }

    #[test]
    fn bundles_all_medium_motion_frames_as_static_poses() {
        let paths = AppAssets::APP_ASSETS
            .iter()
            .map(|(path, _)| *path)
            .filter(|path| path.contains("-medium-"))
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 72);
        for path in paths {
            let bytes = AppAssets
                .load(path)
                .expect("asset lookup should succeed")
                .expect("medium-motion pose should be embedded");
            let pose = image::load_from_memory(bytes.as_ref())
                .expect("medium-motion pose should be a valid static image");
            assert_eq!((pose.width(), pose.height()), (320, 320));
            if [
                "choro-companion-working-medium-",
                "choro-companion-needs-attention-medium-",
                "choro-companion-done-medium-",
                "choro-companion-agent-assistant-medium-",
            ]
            .iter()
            .any(|state| path.contains(state))
            {
                let (visible_width, visible_height) =
                    visible_bounds(&pose).expect("companion pose should contain visible pixels");
                assert!(
                    visible_width >= 175 && visible_height >= 225,
                    "{path} was accidentally shrunk to {visible_width}x{visible_height}"
                );
            }
        }
    }

    #[test]
    fn medium_motion_loops_reverse_smoothly_into_the_first_pose() {
        for prefix in [
            "avatar/choro-companion-idle-medium-",
            "avatar/choro-companion-working-medium-",
            "avatar/choro-companion-needs-attention-medium-",
            "avatar/choro-companion-done-medium-",
            "avatar/choro-companion-music-deep-focus-medium-",
            "avatar/choro-companion-music-lofi-flow-medium-",
            "avatar/choro-companion-music-calm-medium-",
            "avatar/choro-companion-music-high-energy-medium-",
            "avatar/choro-companion-agent-assistant-medium-",
        ] {
            for (forward, reverse) in [(1, 7), (2, 6), (3, 5)] {
                let forward_path = format!("{prefix}{forward}.webp");
                let reverse_path = format!("{prefix}{reverse}.webp");
                let forward_bytes = AppAssets
                    .load(&forward_path)
                    .expect("forward pose lookup should succeed")
                    .expect("forward pose should be embedded");
                let reverse_bytes = AppAssets
                    .load(&reverse_path)
                    .expect("reverse pose lookup should succeed")
                    .expect("reverse pose should be embedded");
                assert_eq!(
                    forward_bytes.as_ref(),
                    reverse_bytes.as_ref(),
                    "{prefix} should return through matching poses"
                );
            }
        }
    }

    #[test]
    fn bundles_all_animated_companion_states() {
        for (path, expected_frames, expected_duration_ms) in [
            ("avatar/choro-companion-working.webp", 49, 42),
            ("avatar/choro-companion-needs-attention.webp", 37, 42),
            ("avatar/choro-companion-done.webp", 37, 42),
            ("avatar/choro-companion-music-deep-focus.webp", 49, 42),
            ("avatar/choro-companion-music-lofi-flow.webp", 49, 42),
            ("avatar/choro-companion-music-calm.webp", 49, 42),
            ("avatar/choro-companion-music-high-energy.webp", 49, 42),
            ("avatar/choro-companion-agent-assistant.webp", 49, 42),
        ] {
            let bytes = AppAssets
                .load(path)
                .expect("asset lookup should succeed")
                .expect("companion asset should be embedded");
            let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes.as_ref()))
                .expect("companion asset should be a valid WebP");
            let frames = decoder
                .into_frames()
                .collect_frames()
                .expect("companion animation frames should decode");

            assert_eq!(
                frames.len(),
                expected_frames,
                "wrong frame count for {path}"
            );
            assert!(frames
                .iter()
                .all(|frame| { frame.buffer().width() == 320 && frame.buffer().height() == 320 }));
            assert!(frames
                .iter()
                .all(|frame| { frame.delay().numer_denom_ms() == (expected_duration_ms, 1) }));
        }
    }
}
