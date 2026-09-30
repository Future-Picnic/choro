use super::*;
use crate::local_store::LocalStore;
use std::fs;

#[test]
fn every_shipped_expert_resolves_a_bounded_portable_package() {
    let root = tempfile::tempdir().unwrap();
    let catalog = catalog::catalog();
    assert_eq!(catalog.experts.len(), 15);
    let mut names = std::collections::HashSet::new();
    let mut ids = std::collections::HashSet::new();
    for definition in &catalog.experts {
        let profile = definition.profile();
        profile.validate().unwrap();
        assert!(names.insert(normalized_expert_name(&profile.name)));
        assert!(ids.insert(profile.id));
        let snapshot = profile.snapshot_at(root.path()).unwrap();
        assert!(!snapshot.skills.is_empty());
        for skill in &snapshot.skills {
            resources::validate_files(&skill.files).unwrap();
        }
        let portable: ExpertSnapshot =
            serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
        let prompt = portable.runtime_instructions(&root.path().join("restored"));
        assert!(prompt.is_ok(), "{}: {:?}", profile.name, prompt.err());
    }
    assert!(names.contains("ai slop reviewer"));
}

#[test]
fn catalog_seeding_preserves_user_profiles_and_archive_tombstones() {
    let root = tempfile::tempdir().unwrap();
    let store = LocalStore::open(root.path().into()).unwrap();
    let mut existing = catalog::catalog().experts[1].profile();
    existing.id = Uuid::new_v4();
    existing.instructions = "My existing design setup".into();
    let saved = store.save_expert(existing, None).unwrap();
    store.ensure_default_experts().unwrap();
    assert_eq!(store.load_experts().unwrap().len(), 15);
    assert_eq!(
        store
            .load_experts()
            .unwrap()
            .into_iter()
            .find(|p| p.id == saved.id)
            .unwrap(),
        saved
    );
    let mut archived = store
        .load_experts()
        .unwrap()
        .into_iter()
        .find(|p| p.name == "AI Slop Reviewer")
        .unwrap();
    archived.name = "My Visual Reviewer".into();
    archived.instructions = "User edits".into();
    archived.enabled = false;
    archived.archived = true;
    let revision = archived.revision;
    let archived = store.save_expert(archived, Some(revision)).unwrap();
    store.ensure_default_experts().unwrap();
    let reopened = LocalStore::open(root.path().into()).unwrap();
    reopened.ensure_default_experts().unwrap();
    let profiles = reopened.load_experts().unwrap();
    assert_eq!(profiles.len(), 15);
    assert_eq!(
        profiles.into_iter().find(|p| p.id == archived.id).unwrap(),
        archived
    );
}

#[test]
fn custom_skills_and_riffs_are_frozen_but_future_tasks_follow_riff_edits() {
    let root = tempfile::tempdir().unwrap();
    let mut profile = tests::profile("Writer");
    let riff_id = Uuid::new_v4();
    profile.additions.riff_ids.push(riff_id);
    profile.additions.custom_skills.push(ExpertCustomSkill {
        id: Uuid::new_v4(),
        name: "Voice".into(),
        description: "For UI copy".into(),
        instructions: "Use the original voice".into(),
    });
    let write_riff = |instructions: &str, enabled: bool| {
        fs::write(root.path().join("choro_riffs.json"), serde_json::to_vec(&serde_json::json!({"riffs":[{"id":riff_id,"name":"Polish","instructions":instructions,"enabled":enabled}]})).unwrap()).unwrap()
    };
    write_riff("Original Riff", true);
    let first = profile.snapshot_at(root.path()).unwrap();
    profile.additions.custom_skills[0].instructions = "Changed voice".into();
    write_riff("Updated Riff", true);
    let second = profile.snapshot_at(root.path()).unwrap();
    assert!(first.instructions().contains("Original Riff"));
    assert!(!first.instructions().contains("Updated Riff"));
    assert!(second.instructions().contains("Updated Riff"));
    assert!(!first.instructions().contains("Changed voice"));
    write_riff("Updated Riff", false);
    assert!(profile.snapshot_at(root.path()).is_err());
    assert!(first
        .runtime_instructions(&root.path().join("cache"))
        .is_ok());
}

#[test]
fn linked_skill_resources_survive_source_changes_and_import() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("installed");
    fs::create_dir_all(source.join("reference")).unwrap();
    fs::write(source.join("SKILL.md"), "Read reference/contract.md").unwrap();
    fs::write(source.join("reference/contract.md"), "Original contract").unwrap();
    let mut profile = tests::profile("Backend");
    profile.skills.push(ExpertSkill {
        provider: profile.provider,
        name: "Contract".into(),
        source: source.join("SKILL.md"),
    });
    let snapshot = profile.snapshot_at(root.path()).unwrap();
    fs::write(source.join("reference/contract.md"), "New contract").unwrap();
    let cache = root.path().join("imported-cache");
    snapshot.runtime_instructions(&cache).unwrap();
    let entry = resources::materialize(&cache, &snapshot.skills[0].files).unwrap();
    assert_eq!(
        fs::read_to_string(entry.parent().unwrap().join("reference/contract.md")).unwrap(),
        "Original contract"
    );
    assert_eq!(
        profile.snapshot_at(root.path()).unwrap().skills[0].files["reference/contract.md"].content,
        "New contract"
    );
}

#[test]
fn unsafe_paths_and_modified_snapshots_cannot_be_materialized() {
    let root = tempfile::tempdir().unwrap();
    for path in [
        "../outside.md",
        "/absolute.md",
        "a/../../escape",
        "C:\\escape",
        "a\\escape",
    ] {
        let files = BTreeMap::from([(
            path.into(),
            FrozenSkillFile {
                content: "x".into(),
                executable: false,
            },
        )]);
        assert!(resources::materialize(root.path(), &files).is_err());
    }
    let mut snapshot = catalog::catalog().experts[1]
        .profile()
        .snapshot_at(root.path())
        .unwrap();
    snapshot.skills[0].content.push_str("tampered");
    assert!(snapshot.runtime_instructions(root.path()).is_err());
}

#[test]
fn old_profile_and_snapshot_json_still_deserialize() {
    let profile = tests::profile("Legacy");
    let mut json = serde_json::to_value(&profile).unwrap();
    json.as_object_mut().unwrap().remove("additions");
    let restored: ExpertProfile = serde_json::from_value(json).unwrap();
    assert_eq!(restored.additions, ExpertAdditions::default());
    let skill: ResolvedExpertSkill = serde_json::from_value(serde_json::json!({"reference":{"provider":"codex","name":"Legacy","source":"/old/SKILL.md"},"content":"old text","sha256":format!("{:x}",Sha256::digest(b"old text"))})).unwrap();
    assert!(skill.files.is_empty());
}

#[cfg(unix)]
#[test]
fn symlinks_and_binary_dependencies_fail_with_a_repairable_error() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("SKILL.md"), "Read references").unwrap();
    fs::create_dir(root.path().join("references")).unwrap();
    symlink("/etc/passwd", root.path().join("references/escape.md")).unwrap();
    assert!(resources::capture(&root.path().join("SKILL.md"))
        .unwrap_err()
        .to_string()
        .contains("symlink"));
    let binary = tempfile::tempdir().unwrap();
    fs::write(binary.path().join("SKILL.md"), "Run scripts/tool").unwrap();
    fs::create_dir(binary.path().join("scripts")).unwrap();
    fs::write(binary.path().join("scripts/tool"), [0xff, 0xfe]).unwrap();
    assert!(resources::capture(&binary.path().join("SKILL.md")).is_err());
}

#[test]
fn concurrent_experts_share_frozen_packages_without_partial_reads() {
    let root = tempfile::tempdir().unwrap();
    let files = catalog::files("impeccable").unwrap();
    std::thread::scope(|scope| {
        let workers = (0..6)
            .map(|_| scope.spawn(|| resources::materialize(root.path(), &files).unwrap()))
            .collect::<Vec<_>>();
        let paths = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert!(paths.windows(2).all(|pair| pair[0] == pair[1]));
    });
}

#[test]
fn importing_a_legacy_workspace_into_a_seeded_store_restores_defaults() {
    let source = tempfile::tempdir().unwrap();
    let source_store = LocalStore::open(source.path().into()).unwrap();
    let archive = source.path().join("legacy.zip");
    source_store.export_workspace(&archive).unwrap();
    let target = tempfile::tempdir().unwrap();
    let target_store = LocalStore::open(target.path().into()).unwrap();
    target_store.ensure_default_experts().unwrap();
    target_store.import_workspace_replace(&archive).unwrap();
    assert_eq!(target_store.load_experts().unwrap().len(), 15);
}

#[test]
fn deeply_nested_empty_resource_directories_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("SKILL.md"), "Reference a bounded package").unwrap();
    let mut nested = root.path().join("references");
    for _ in 0..34 {
        nested.push("nested");
    }
    fs::create_dir_all(&nested).unwrap();
    assert!(resources::capture(&root.path().join("SKILL.md"))
        .unwrap_err()
        .to_string()
        .contains("nested too deeply"));
}

#[cfg(unix)]
#[test]
fn installation_folder_aliases_work_but_redirected_entrypoints_do_not() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("installed");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("SKILL.md"), "Legitimate skill").unwrap();
    symlink(&folder, root.path().join("alias")).unwrap();
    assert_eq!(
        resources::capture(&root.path().join("alias/SKILL.md")).unwrap()["SKILL.md"].content,
        "Legitimate skill"
    );
    let redirected = root.path().join("redirected");
    fs::create_dir(&redirected).unwrap();
    symlink(folder.join("SKILL.md"), redirected.join("SKILL.md")).unwrap();
    assert!(resources::capture(&redirected.join("SKILL.md"))
        .unwrap_err()
        .to_string()
        .contains("symlink"));
}
