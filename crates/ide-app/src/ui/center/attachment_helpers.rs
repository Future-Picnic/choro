use super::*;

pub(super) fn clipboard_image_extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Webp => "webp",
        ImageFormat::Gif => "gif",
        ImageFormat::Svg => "svg",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Tiff => "tiff",
    }
}

pub(super) fn clipboard_image_mime_type(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Webp => "image/webp",
        ImageFormat::Gif => "image/gif",
        ImageFormat::Svg => "image/svg+xml",
        ImageFormat::Bmp => "image/bmp",
        ImageFormat::Tiff => "image/tiff",
    }
}

pub(super) fn clipboard_image_from_item(item: gpui::ClipboardItem) -> Option<gpui::Image> {
    item.into_entries().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(image),
        ClipboardEntry::String(_) => None,
    })
}

pub(super) fn image_format_for_path(path: &Path) -> Option<ImageFormat> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Some(ImageFormat::Png),
        Some("jpg" | "jpeg") => Some(ImageFormat::Jpeg),
        Some("webp") => Some(ImageFormat::Webp),
        Some("gif") => Some(ImageFormat::Gif),
        Some("svg") => Some(ImageFormat::Svg),
        Some("bmp") => Some(ImageFormat::Bmp),
        Some("tif" | "tiff") => Some(ImageFormat::Tiff),
        _ => None,
    }
}

pub(super) fn clipboard_item_for_image_path(path: &Path) -> anyhow::Result<ClipboardItem> {
    let format = image_format_for_path(path)
        .ok_or_else(|| anyhow::anyhow!("unsupported image format: {}", path.display()))?;
    let bytes = fs::read(path)?;
    let image = gpui::Image::from_bytes(format, bytes);
    Ok(ClipboardItem::new_image(&image))
}

pub(super) fn materialize_project_clipboard_image(
    project: ProjectId,
    image: &gpui::Image,
) -> anyhow::Result<PathBuf> {
    let dir = AppConfig::project_chat_attachments_dir(project);
    fs::create_dir_all(&dir)?;
    let extension = clipboard_image_extension(image.format);
    let target = dir.join(format!(
        "pasted-image-{}.{}",
        Uuid::new_v4().simple(),
        extension
    ));
    fs::write(&target, &image.bytes)?;
    Ok(target)
}

pub(super) fn materialize_agent_clipboard_image(
    agent_id: Uuid,
    image: &gpui::Image,
) -> anyhow::Result<PathBuf> {
    let extension = clipboard_image_extension(image.format);
    let store = ide_core::local_store::LocalStore::open_default()?;
    let attachment = store.materialize_attachment_bytes(
        agent_id,
        format!("pasted-image-{}.{}", Uuid::new_v4().simple(), extension),
        Some(clipboard_image_mime_type(image.format).to_string()),
        extension,
        &image.bytes,
    )?;
    Ok(store.root().join(attachment.relative_path))
}

pub(super) fn prompt_with_attached_files(prompt: &str, attached_files: &[PathBuf]) -> String {
    if attached_files.is_empty() {
        return prompt.to_string();
    }

    let mut result = prompt.trim_end().to_string();
    result.push_str("\n\nAttached files:\n");
    for path in attached_files {
        result.push_str("- ");
        result.push_str(&path.to_string_lossy());
        result.push('\n');
    }
    result
}

pub(super) fn split_prompt_attached_files(prompt: &str) -> (String, Vec<PathBuf>) {
    let Some((body, attachment_block)) = prompt
        .rsplit_once("\n\nAttached files:\n")
        .or_else(|| prompt.rsplit_once("\n\nAttached images:\n"))
    else {
        return (prompt.to_string(), Vec::new());
    };

    let paths = attachment_block
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- "))
        .map(PathBuf::from)
        .collect::<Vec<_>>();

    if paths.is_empty() {
        (prompt.to_string(), Vec::new())
    } else {
        (body.trim_end().to_string(), paths)
    }
}

pub(super) fn queued_turn_composer_draft(turn: &QueuedChatTurn) -> (String, Vec<PathBuf>) {
    let visible_text = turn
        .display_text
        .clone()
        .unwrap_or_else(|| visible_agent_chat_submission_text(&turn.text).to_string());
    let (text, _) = split_prompt_attached_files(&visible_text);
    let (_, attached_files) = split_prompt_attached_files(&turn.text);
    (text, attached_files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_clipboard_image_extraction_preserves_payload() {
        let expected = gpui::Image::from_bytes(ImageFormat::Png, vec![1, 2, 3, 4]);
        let item = ClipboardItem::new_image(&expected);

        let extracted = clipboard_image_from_item(item).expect("clipboard image");

        assert_eq!(extracted.format, expected.format);
        assert_eq!(extracted.bytes, expected.bytes);
    }
}
