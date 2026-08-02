use super::*;

pub(super) fn copy_project_reference_source_file(
    store: &LocalStore,
    project_id: ProjectId,
    reference_id: Uuid,
    source: &Path,
) -> Result<PathBuf> {
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.trim_start_matches('.'))
        .filter(|extension| !extension.is_empty())
        .unwrap_or("image");
    let relative_path = PathBuf::from("data")
        .join("projects")
        .join(project_id.0.to_string())
        .join("references")
        .join(reference_id.to_string())
        .join(format!("source.{extension}"));
    let target = store.root.join(&relative_path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create reference directory {}", parent.display())
        })?;
    }
    fs::copy(source, &target).with_context(|| {
        format!(
            "failed to copy reference image {} to {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(relative_path)
}

pub(super) fn write_project_reference_preview(
    store: &LocalStore,
    project_id: ProjectId,
    reference_id: Uuid,
    source: &Path,
) -> Result<PathBuf> {
    let image = image::ImageReader::open(source)
        .with_context(|| format!("failed to open reference preview {}", source.display()))?
        .with_guessed_format()
        .context("failed to detect reference preview format")?
        .decode()
        .context("failed to decode reference preview image")?;
    let preview = image.resize(
        PROJECT_REFERENCE_PREVIEW_MAX_SIZE,
        PROJECT_REFERENCE_PREVIEW_MAX_SIZE,
        image::imageops::FilterType::Lanczos3,
    );
    let relative_path = PathBuf::from("data")
        .join("projects")
        .join(project_id.0.to_string())
        .join("references")
        .join(reference_id.to_string())
        .join("preview.png");
    let target = store.root.join(&relative_path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create reference directory {}", parent.display())
        })?;
    }
    preview
        .save_with_format(&target, image::ImageFormat::Png)
        .with_context(|| format!("failed to save reference preview {}", target.display()))?;
    Ok(relative_path)
}
