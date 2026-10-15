//! Terminal-only cleanup. Preview by default; --apply uses prior user consent.
use anyhow::{ensure, Result};
use ide_core::{AppConfig, local_store::{preview_legacy_bandmate_storage, cleanup_legacy_bandmate_storage}};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.is_empty() || args == ["--dry-run"] || args == ["--apply"], "Usage: cleanup_legacy_bandmates [--dry-run|--apply]");
    let root = AppConfig::config_root();
    let apply = args == ["--apply"];
    if apply {
        println!("Checking approved cleanup at {}. Previous partial cleanup will resume.", root.join("data/delegation").display());
    }
    let result = if apply { cleanup_legacy_bandmate_storage(&root)? }
        else { preview_legacy_bandmate_storage(&root)? };
    println!("{}", serde_json::to_string_pretty(&result)?);
    if apply { println!("Reclaimed {:.2} GiB from the remaining legacy copies.", result.allocated_bytes as f64 / (1024.0 * 1024.0 * 1024.0)); }
    println!("{}", if apply { "Removed legacy copies. Conversations, reports, profiles, and real projects were preserved." }
        else { "Preview only. --apply permanently removes legacy copies, including unmerged work. Close Choro and legacy Bandmate processes first." });
    Ok(())
}
