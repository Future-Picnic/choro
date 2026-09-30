use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context as _, Result};
use ide_core::config::{GitStatusGroupMode, GitStatusViewMode};
use ide_core::local_store::LocalStore;
use ide_core::{
    AgentAccessMode, AgentChangedFile, AgentEffort, AgentKind, AgentModel, AgentRecord,
    AgentRuntimeKind, AgentStatus, AppConfig, PersonalTaskPriority, PersonalTaskRecord,
    PersonalTaskStatus, Project, ProjectId, ProjectReferenceKind, ProjectSection, ProjectSectionId,
    ScriptPreset, TaskTrackerClient, TaskTrackerConnection,
};
use uuid::Uuid;

const DEMO_BUILD_FILE: &str = "demo-build-id";
const DEMO_FIXTURE_VERSION: u32 = 3;
const DEMO_NOW: u64 = 1_784_420_400;

const PRODUCTS_SECTION_ID: ProjectSectionId =
    ProjectSectionId(Uuid::from_u128(0x43b9_0000_0000_0000_0000_0000_0000_0001));
const CLIENT_WORK_SECTION_ID: ProjectSectionId =
    ProjectSectionId(Uuid::from_u128(0x43b9_0000_0000_0000_0000_0000_0000_0002));
const SERVICES_SECTION_ID: ProjectSectionId =
    ProjectSectionId(Uuid::from_u128(0x43b9_0000_0000_0000_0000_0000_0000_0003));

const NORTHSTAR_ID: ProjectId =
    ProjectId(Uuid::from_u128(0x43b9_1000_0000_0000_0000_0000_0000_0001));
const MOMENTUM_ID: ProjectId =
    ProjectId(Uuid::from_u128(0x43b9_1000_0000_0000_0000_0000_0000_0002));
const EMBER_ID: ProjectId = ProjectId(Uuid::from_u128(0x43b9_1000_0000_0000_0000_0000_0000_0003));
const RELAY_ID: ProjectId = ProjectId(Uuid::from_u128(0x43b9_1000_0000_0000_0000_0000_0000_0004));

const NORTHSTAR_FILES: &[(&str, &[u8])] = &[
    (
        "README.md",
        include_bytes!("../assets/demo/northstar-client-portal/README.md"),
    ),
    (
        "AGENTS.md",
        include_bytes!("../assets/demo/northstar-client-portal/AGENTS.md"),
    ),
    (
        ".gitignore",
        include_bytes!("../assets/demo/northstar-client-portal/.gitignore"),
    ),
    (
        ".env.example",
        include_bytes!("../assets/demo/northstar-client-portal/.env.example"),
    ),
    (
        "index.html",
        include_bytes!("../assets/demo/northstar-client-portal/index.html"),
    ),
    (
        "styles.css",
        include_bytes!("../assets/demo/northstar-client-portal/styles.css"),
    ),
    (
        "scripts/serve.py",
        include_bytes!("../assets/demo/northstar-client-portal/scripts/serve.py"),
    ),
    (
        "scripts/verify.py",
        include_bytes!("../assets/demo/northstar-client-portal/scripts/verify.py"),
    ),
    (
        "choro_docs/product-brief.choro",
        include_bytes!("../assets/demo/northstar-client-portal/choro_docs/product-brief.choro"),
    ),
    (
        "choro_docs/activation-flow.choro",
        include_bytes!("../assets/demo/northstar-client-portal/choro_docs/activation-flow.choro"),
    ),
    (
        "assets/dashboard-wireframe.svg",
        include_bytes!("../assets/demo/northstar-client-portal/assets/dashboard-wireframe.svg"),
    ),
];

const MOMENTUM_FILES: &[(&str, &[u8])] = &[
    (
        "README.md",
        include_bytes!("../assets/demo/momentum-habit-tracker/README.md"),
    ),
    (
        "AGENTS.md",
        include_bytes!("../assets/demo/momentum-habit-tracker/AGENTS.md"),
    ),
    (
        ".gitignore",
        include_bytes!("../assets/demo/momentum-habit-tracker/.gitignore"),
    ),
    (
        ".env.example",
        include_bytes!("../assets/demo/momentum-habit-tracker/.env.example"),
    ),
    (
        "Package.swift",
        include_bytes!("../assets/demo/momentum-habit-tracker/Package.swift"),
    ),
    (
        "Sources/MomentumApp/Habit.swift",
        include_bytes!("../assets/demo/momentum-habit-tracker/Sources/MomentumApp/Habit.swift"),
    ),
    (
        "Sources/MomentumApp/HabitListView.swift",
        include_bytes!(
            "../assets/demo/momentum-habit-tracker/Sources/MomentumApp/HabitListView.swift"
        ),
    ),
    (
        "Tests/MomentumAppTests/HabitTests.swift",
        include_bytes!(
            "../assets/demo/momentum-habit-tracker/Tests/MomentumAppTests/HabitTests.swift"
        ),
    ),
    (
        "choro_docs/mobile-product-brief.choro",
        include_bytes!(
            "../assets/demo/momentum-habit-tracker/choro_docs/mobile-product-brief.choro"
        ),
    ),
    (
        "choro_docs/offline-behavior.choro",
        include_bytes!("../assets/demo/momentum-habit-tracker/choro_docs/offline-behavior.choro"),
    ),
    (
        "assets/streak-reference.svg",
        include_bytes!("../assets/demo/momentum-habit-tracker/assets/streak-reference.svg"),
    ),
];

const EMBER_FILES: &[(&str, &[u8])] = &[
    (
        "README.md",
        include_bytes!("../assets/demo/ember-coffee-website/README.md"),
    ),
    (
        "AGENTS.md",
        include_bytes!("../assets/demo/ember-coffee-website/AGENTS.md"),
    ),
    (
        ".gitignore",
        include_bytes!("../assets/demo/ember-coffee-website/.gitignore"),
    ),
    (
        ".env.example",
        include_bytes!("../assets/demo/ember-coffee-website/.env.example"),
    ),
    (
        "index.html",
        include_bytes!("../assets/demo/ember-coffee-website/index.html"),
    ),
    (
        "styles.css",
        include_bytes!("../assets/demo/ember-coffee-website/styles.css"),
    ),
    (
        "content/menu.json",
        include_bytes!("../assets/demo/ember-coffee-website/content/menu.json"),
    ),
    (
        "content/locations.json",
        include_bytes!("../assets/demo/ember-coffee-website/content/locations.json"),
    ),
    (
        "scripts/serve.py",
        include_bytes!("../assets/demo/ember-coffee-website/scripts/serve.py"),
    ),
    (
        "choro_docs/launch-brief.choro",
        include_bytes!("../assets/demo/ember-coffee-website/choro_docs/launch-brief.choro"),
    ),
    (
        "choro_docs/writing-guidelines.choro",
        include_bytes!("../assets/demo/ember-coffee-website/choro_docs/writing-guidelines.choro"),
    ),
    (
        "assets/brand-board.svg",
        include_bytes!("../assets/demo/ember-coffee-website/assets/brand-board.svg"),
    ),
];

const RELAY_FILES: &[(&str, &[u8])] = &[
    (
        "README.md",
        include_bytes!("../assets/demo/relay-orders-api/README.md"),
    ),
    (
        "AGENTS.md",
        include_bytes!("../assets/demo/relay-orders-api/AGENTS.md"),
    ),
    (
        ".gitignore",
        include_bytes!("../assets/demo/relay-orders-api/.gitignore"),
    ),
    (
        ".env.example",
        include_bytes!("../assets/demo/relay-orders-api/.env.example"),
    ),
    (
        "package.json",
        include_bytes!("../assets/demo/relay-orders-api/package.json"),
    ),
    (
        "src/orders.js",
        include_bytes!("../assets/demo/relay-orders-api/src/orders.js"),
    ),
    (
        "src/webhooks.js",
        include_bytes!("../assets/demo/relay-orders-api/src/webhooks.js"),
    ),
    (
        "tests/orders.test.js",
        include_bytes!("../assets/demo/relay-orders-api/tests/orders.test.js"),
    ),
    (
        "tests/webhooks.test.js",
        include_bytes!("../assets/demo/relay-orders-api/tests/webhooks.test.js"),
    ),
    (
        "scripts/verify.js",
        include_bytes!("../assets/demo/relay-orders-api/scripts/verify.js"),
    ),
    (
        "choro_docs/api-contract.choro",
        include_bytes!("../assets/demo/relay-orders-api/choro_docs/api-contract.choro"),
    ),
    (
        "choro_docs/webhook-reliability.choro",
        include_bytes!("../assets/demo/relay-orders-api/choro_docs/webhook-reliability.choro"),
    ),
    (
        "assets/api-flow.svg",
        include_bytes!("../assets/demo/relay-orders-api/assets/api-flow.svg"),
    ),
];

struct ProjectSeed {
    id: ProjectId,
    name: &'static str,
    folder: &'static str,
    section_id: ProjectSectionId,
    icon: &'static str,
    color: &'static str,
    favorite: bool,
    files: &'static [(&'static str, &'static [u8])],
    scripts: &'static [(&'static str, &'static str)],
    preview_url: Option<&'static str>,
    asset_path: &'static str,
    asset_title: &'static str,
    dirty_path: Option<(&'static str, &'static [u8])>,
    branch: Option<&'static str>,
}

const PROJECTS: &[ProjectSeed] = &[
    ProjectSeed {
        id: NORTHSTAR_ID,
        name: "Northstar Client Portal",
        folder: "northstar-client-portal",
        section_id: PRODUCTS_SECTION_ID,
        icon: "layout-dashboard",
        color: "purple",
        favorite: true,
        files: NORTHSTAR_FILES,
        scripts: &[("Preview", "python3 scripts/serve.py"), ("Verify", "python3 scripts/verify.py")],
        preview_url: Some("http://127.0.0.1:4310"),
        asset_path: "assets/dashboard-wireframe.svg",
        asset_title: "Dashboard wireframe",
        dirty_path: Some(("src/empty-state-notes.md", b"# First workspace empty state\n\n- Lead with the next useful client action.\n- Keep sample content visually distinct from real project activity.\n")),
        branch: None,
    },
    ProjectSeed {
        id: MOMENTUM_ID,
        name: "Momentum Habit Tracker",
        folder: "momentum-habit-tracker",
        section_id: PRODUCTS_SECTION_ID,
        icon: "heart",
        color: "pink",
        favorite: false,
        files: MOMENTUM_FILES,
        scripts: &[("Tests", "swift test")],
        preview_url: None,
        asset_path: "assets/streak-reference.svg",
        asset_title: "Streak celebration reference",
        dirty_path: None,
        branch: None,
    },
    ProjectSeed {
        id: EMBER_ID,
        name: "Ember Coffee Website",
        folder: "ember-coffee-website",
        section_id: CLIENT_WORK_SECTION_ID,
        icon: "coffee",
        color: "orange",
        favorite: false,
        files: EMBER_FILES,
        scripts: &[("Preview", "python3 scripts/serve.py")],
        preview_url: Some("http://127.0.0.1:4330"),
        asset_path: "assets/brand-board.svg",
        asset_title: "Summer brand board",
        dirty_path: Some(("customer-story.html", b"<article>\n  <p>\"The first quiet hour at Ember became part of my week.\"</p>\n  <cite>Yael, Florentin</cite>\n</article>\n")),
        branch: Some("feature/customer-stories"),
    },
    ProjectSeed {
        id: RELAY_ID,
        name: "Relay Orders API",
        folder: "relay-orders-api",
        section_id: SERVICES_SECTION_ID,
        icon: "database",
        color: "blue",
        favorite: false,
        files: RELAY_FILES,
        scripts: &[("Tests", "npm test"), ("Verify", "npm run verify")],
        preview_url: None,
        asset_path: "assets/api-flow.svg",
        asset_title: "Orders delivery flow",
        dirty_path: None,
        branch: Some("fix/webhook-deduplication"),
    },
];

struct TaskSeed {
    id: u128,
    title: &'static str,
    description: &'static str,
    status: PersonalTaskStatus,
    priority: PersonalTaskPriority,
    labels: &'static [&'static str],
}

struct AgentSeed {
    id: u128,
    title: &'static str,
    prompt: &'static str,
    result: &'static str,
    linked_doc: &'static str,
    changed_files: &'static [(&'static str, usize, usize)],
    task_index: usize,
    provider: AgentKind,
    status: AgentStatus,
}

pub fn prepare() -> Result<()> {
    if std::env::var_os("CHORO_DEMO").is_none() {
        return Ok(());
    }

    let root = AppConfig::config_root();
    let build_id = std::env::var("CHORO_DEMO_BUILD_ID").unwrap_or_else(|_| "development".into());
    let seed_id = format!("{build_id}:{DEMO_FIXTURE_VERSION}");
    let current = fs::read_to_string(root.join(DEMO_BUILD_FILE)).ok();
    let forced = std::env::var_os("CHORO_DEMO_RESET").is_some();
    let complete = root.join("state.db").is_file() && root.join("config.json").is_file();

    if !needs_reseed(current.as_deref(), &seed_id, forced, complete) {
        return Ok(());
    }
    if root.exists() {
        fs::remove_dir_all(&root)
            .with_context(|| format!("failed to reset Demo data at {}", root.display()))?;
    }
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create Demo data root {}", root.display()))?;
    seed_demo(&root)?;
    fs::write(root.join(DEMO_BUILD_FILE), seed_id)
        .context("failed to record the Choro Demo build id")?;
    Ok(())
}

fn needs_reseed(current: Option<&str>, expected: &str, forced: bool, complete: bool) -> bool {
    forced || !complete || current != Some(expected)
}

fn seed_demo(root: &Path) -> Result<()> {
    let store = LocalStore::open_default()?;
    let project_parent = root.join("demo-projects");
    let sections = vec![
        ProjectSection {
            id: PRODUCTS_SECTION_ID,
            name: "Products".into(),
            collapsed: false,
        },
        ProjectSection {
            id: CLIENT_WORK_SECTION_ID,
            name: "Client Work".into(),
            collapsed: false,
        },
        ProjectSection {
            id: SERVICES_SECTION_ID,
            name: "Services".into(),
            collapsed: false,
        },
    ];

    let mut projects = Vec::new();
    for seed in PROJECTS {
        let project_root = project_parent.join(seed.folder);
        write_project_files(&project_root, seed.files)?;
        initialize_git_repository(&project_root, seed)?;

        let mut project = Project::from_path(project_root.clone());
        project.id = seed.id;
        project.name = seed.name.into();
        project.section_id = Some(seed.section_id);
        project.icon = seed.icon.into();
        project.icon_color = seed.color.into();
        project.is_favorite = seed.favorite;
        project.presets = seed
            .scripts
            .iter()
            .map(|(name, command)| ScriptPreset::new(*name, *command))
            .collect();
        if seed.id == NORTHSTAR_ID {
            project
                .task_tracker_connections
                .push(demo_jira_connection());
        }
        projects.push(project);
    }

    let mut config = AppConfig::default();
    config.project_sections = sections;
    config.projects = projects;
    config.active_project = Some(NORTHSTAR_ID);
    config.expanded_projects = PROJECTS.iter().map(|project| project.id).collect();
    config.git_status_view = GitStatusViewMode::Tree;
    config.git_status_group = GitStatusGroupMode::Status;
    store.save_workspace_config(&config)?;
    config.save()?;

    let mut agents = Vec::new();
    for seed in PROJECTS {
        let project_root = project_parent.join(seed.folder);
        let tasks = seed_tasks(&store, seed.id)?;
        seed_references(&store, seed, &project_root)?;
        for agent_seed in agent_seeds(seed.id) {
            let mut agent = AgentRecord::new(
                seed.id,
                project_root.clone(),
                agent_seed.title,
                agent_seed.prompt,
                agent_seed.provider,
                AgentModel::default_for(agent_seed.provider),
                AgentEffort::High,
                AgentAccessMode::AutoAcceptEdits,
            );
            agent.id = Uuid::from_u128(agent_seed.id);
            agent.runtime = AgentRuntimeKind::Chat;
            agent.status = agent_seed.status;
            agent.notes = agent_seed.result.into();
            agent.linked_docs = vec![PathBuf::from(agent_seed.linked_doc)];
            agent.source_doc = Some(PathBuf::from(agent_seed.linked_doc));
            if let Some(task) = tasks.get(agent_seed.task_index) {
                agent.linked_tasks = vec![task.task_ref()];
                agent.source_task = Some(task.task_ref());
            }
            agent.changed_files = agent_seed
                .changed_files
                .iter()
                .map(|(path, additions, deletions)| AgentChangedFile {
                    path: PathBuf::from(path),
                    additions: *additions,
                    deletions: *deletions,
                })
                .collect();
            agent.created_at = DEMO_NOW - 86_400 * (agent_seed.task_index as u64 + 3);
            agent.started_at = Some(agent.created_at + 90);
            agent.updated_at = agent.created_at + 1_200;
            agents.push((agent, agent_seed));
        }
    }

    let records = agents
        .iter()
        .map(|(agent, _)| agent.clone())
        .collect::<Vec<_>>();
    store.save_agents(&records)?;
    for (agent, seed) in agents {
        store.append_chat_message(agent.id, "user", seed.prompt, agent.created_at + 90, None)?;
        store.append_chat_message(agent.id, "assistant", seed.result, agent.updated_at, None)?;
    }
    Ok(())
}

fn seed_tasks(store: &LocalStore, project_id: ProjectId) -> Result<Vec<PersonalTaskRecord>> {
    task_seeds(project_id)
        .iter()
        .enumerate()
        .map(|(index, seed)| {
            let task = PersonalTaskRecord {
                id: Uuid::from_u128(seed.id),
                project_id,
                key_number: index as i64 + 1,
                title: seed.title.into(),
                description_markdown: seed.description.into(),
                status: seed.status,
                priority: seed.priority,
                labels: seed.labels.iter().map(|label| (*label).into()).collect(),
                created_at: DEMO_NOW - 86_400 * (index as u64 + 6),
                updated_at: DEMO_NOW - 3_600 * index as u64,
                archived: false,
            };
            store.upsert_personal_task(&task)?;
            if seed.status == PersonalTaskStatus::Done {
                store.add_personal_task_comment(
                    task.id,
                    "Demo Agent",
                    "Implemented and verified against the linked project Doc.",
                )?;
            }
            Ok(task)
        })
        .collect()
}

fn seed_references(store: &LocalStore, seed: &ProjectSeed, root: &Path) -> Result<()> {
    store.create_project_reference(
        seed.id,
        ProjectReferenceKind::File,
        seed.asset_title,
        root.join(seed.asset_path).to_string_lossy(),
        "Bundled visual direction for this Demo project.",
        None,
    )?;
    store.create_project_reference(
        seed.id,
        ProjectReferenceKind::Url,
        "Product notes",
        format!("https://example.invalid/{}", seed.folder),
        "A safe placeholder for an external product reference.",
        None,
    )?;
    if let Some(url) = seed.preview_url {
        store.create_project_reference(
            seed.id,
            ProjectReferenceKind::Url,
            "Local product preview",
            url,
            "Run the project Preview script, then open this reference.",
            None,
        )?;
    }
    Ok(())
}

fn demo_jira_connection() -> TaskTrackerConnection {
    let mut connection = base_demo_jira_connection();

    if let Ok(board_id) = std::env::var("CHORO_DEMO_JIRA_BOARD_ID")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or(())
    {
        apply_jira_board(&mut connection, board_id);
    } else if std::env::var_os("CHORO_DEMO_JIRA_TOKEN").is_some() {
        if let Ok(client) = TaskTrackerClient::new(connection.clone()) {
            if let Ok(sources) = client.list_sources() {
                if let Some(source) = sources
                    .into_iter()
                    .find(|source| source.name.eq_ignore_ascii_case("Kan board"))
                {
                    if let Ok(board_id) = source.id.parse::<i64>() {
                        apply_jira_board(&mut connection, board_id);
                    }
                }
            }
        }
    }

    if connection.board_id.is_some() && std::env::var_os("CHORO_DEMO_JIRA_TOKEN").is_some() {
        if let Ok(client) = TaskTrackerClient::new(connection.clone()) {
            if let Ok(users) = client.list_assignees() {
                if let Some(user) = users.into_iter().find(|user| {
                    user.display_name.eq_ignore_ascii_case("Liran Gabai")
                        || user.email.as_deref() == Some("liran@ritmus.studio")
                }) {
                    connection.assignee_account_id = Some(user.account_id);
                    connection.assignee_display_name = Some(user.display_name);
                }
            }
        }
    }
    connection
}

fn base_demo_jira_connection() -> TaskTrackerConnection {
    let mut connection = TaskTrackerConnection::new_jira(
        "Ritmus Jira · Kan board",
        "https://ritmus-41374047.atlassian.net",
        "liran@ritmus.studio",
        "${CHORO_DEMO_JIRA_TOKEN}",
    );
    connection.id = Uuid::from_u128(0x43b9_2000_0000_0000_0000_0000_0000_0001);
    connection.source_name = Some("Kan board".into());
    connection.source_kind = Some("kanban".into());
    connection.board_name = Some("Kan board".into());
    connection.assignee_filter = Some("Liran Gabai".into());
    connection.assignee_display_name = Some("Liran Gabai".into());
    connection
}

fn apply_jira_board(connection: &mut TaskTrackerConnection, board_id: i64) {
    connection.source_id = Some(board_id.to_string());
    connection.source_name = Some("Kan board".into());
    connection.board_id = Some(board_id);
    connection.board_name = Some("Kan board".into());
}

fn write_project_files(root: &Path, files: &[(&str, &[u8])]) -> Result<()> {
    for (relative, contents) in files {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&path, contents)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    let env_example = root.join(".env.example");
    let env = root.join(".env");
    fs::copy(&env_example, &env).with_context(|| {
        format!(
            "failed to create {} from {}",
            env.display(),
            env_example.display()
        )
    })?;
    Ok(())
}

fn initialize_git_repository(root: &Path, seed: &ProjectSeed) -> Result<()> {
    run_git(root, &["-c", "init.defaultBranch=main", "init", "-q"])?;
    run_git(
        root,
        &[
            "add",
            "README.md",
            "AGENTS.md",
            ".gitignore",
            ".env.example",
        ],
    )?;
    commit(root, "Initial project setup")?;
    run_git(root, &["add", "."])?;
    commit(root, "Add product demo workflow")?;

    if let Some(branch) = seed.branch {
        run_git(root, &["checkout", "-qb", branch])?;
        let note = root.join("BRANCH_NOTES.md");
        fs::write(
            &note,
            format!(
                "# {}\n\nPrepared feature branch for the Choro Demo workspace.\n",
                branch
            ),
        )?;
        run_git(root, &["add", "BRANCH_NOTES.md"])?;
        let message = if seed.id == RELAY_ID {
            "Document webhook deduplication rollout"
        } else {
            "Start customer stories branch"
        };
        commit(root, message)?;
    }
    if let Some((path, contents)) = seed.dirty_path {
        let path = root.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
    }
    Ok(())
}

fn commit(root: &Path, message: &str) -> Result<()> {
    run_git(
        root,
        &[
            "-c",
            "user.name=Choro Demo",
            "-c",
            "user.email=demo@choro.local",
            "commit",
            "-qm",
            message,
        ],
    )
}

fn run_git(root: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to run git in {}", root.display()))?;
    if !status.success() {
        bail!(
            "git command failed in {}: git {}",
            root.display(),
            args.join(" ")
        );
    }
    Ok(())
}

fn task_seeds(project: ProjectId) -> &'static [TaskSeed] {
    if project == NORTHSTAR_ID {
        &NORTHSTAR_TASKS
    } else if project == MOMENTUM_ID {
        &MOMENTUM_TASKS
    } else if project == EMBER_ID {
        &EMBER_TASKS
    } else {
        &RELAY_TASKS
    }
}

fn agent_seeds(project: ProjectId) -> &'static [AgentSeed] {
    if project == NORTHSTAR_ID {
        &NORTHSTAR_AGENTS
    } else if project == MOMENTUM_ID {
        &MOMENTUM_AGENTS
    } else if project == EMBER_ID {
        &EMBER_AGENTS
    } else {
        &RELAY_AGENTS
    }
}

const NORTHSTAR_TASKS: [TaskSeed; 5] = [
    TaskSeed {
        id: 0x43b9_3001_0000_0000_0000_0000_0000_0001,
        title: "Build the client overview",
        description:
            "Create the weekly progress, milestone, and decision summary from the product brief.",
        status: PersonalTaskStatus::Done,
        priority: PersonalTaskPriority::High,
        labels: &["frontend", "client"],
    },
    TaskSeed {
        id: 0x43b9_3001_0000_0000_0000_0000_0000_0002,
        title: "Add project status filters",
        description: "Let clients narrow updates without exposing internal workflow states.",
        status: PersonalTaskStatus::Done,
        priority: PersonalTaskPriority::Medium,
        labels: &["frontend"],
    },
    TaskSeed {
        id: 0x43b9_3001_0000_0000_0000_0000_0000_0003,
        title: "Design the first-workspace empty state",
        description:
            "Give a newly invited client one clear next action and useful example content.",
        status: PersonalTaskStatus::InProgress,
        priority: PersonalTaskPriority::High,
        labels: &["design", "activation"],
    },
    TaskSeed {
        id: 0x43b9_3001_0000_0000_0000_0000_0000_0004,
        title: "Add keyboard navigation",
        description: "Support the primary portal navigation and update list from the keyboard.",
        status: PersonalTaskStatus::Todo,
        priority: PersonalTaskPriority::Medium,
        labels: &["accessibility"],
    },
    TaskSeed {
        id: 0x43b9_3001_0000_0000_0000_0000_0000_0005,
        title: "Improve the compact dashboard",
        description: "Keep the overview useful below 760 pixels without hiding decisions.",
        status: PersonalTaskStatus::Todo,
        priority: PersonalTaskPriority::Low,
        labels: &["responsive"],
    },
];

const MOMENTUM_TASKS: [TaskSeed; 4] = [
    TaskSeed {
        id: 0x43b9_3002_0000_0000_0000_0000_0000_0001,
        title: "Build today's habit list",
        description: "Create the primary SwiftUI list with comfortable completion controls.",
        status: PersonalTaskStatus::Done,
        priority: PersonalTaskPriority::High,
        labels: &["swiftui"],
    },
    TaskSeed {
        id: 0x43b9_3002_0000_0000_0000_0000_0000_0002,
        title: "Add a gentle streak celebration",
        description: "Celebrate consistency without punishing a missed day.",
        status: PersonalTaskStatus::Done,
        priority: PersonalTaskPriority::Medium,
        labels: &["motion", "design"],
    },
    TaskSeed {
        id: 0x43b9_3002_0000_0000_0000_0000_0000_0003,
        title: "Filter unfinished habits",
        description: "Add an accessible filter for the remaining habits today.",
        status: PersonalTaskStatus::InProgress,
        priority: PersonalTaskPriority::Medium,
        labels: &["swiftui", "accessibility"],
    },
    TaskSeed {
        id: 0x43b9_3002_0000_0000_0000_0000_0000_0004,
        title: "Queue offline completions",
        description: "Persist completion operations and retry with stable identifiers.",
        status: PersonalTaskStatus::Todo,
        priority: PersonalTaskPriority::High,
        labels: &["offline", "data"],
    },
];

const EMBER_TASKS: [TaskSeed; 4] = [
    TaskSeed {
        id: 0x43b9_3003_0000_0000_0000_0000_0000_0001,
        title: "Build the summer landing page",
        description: "Launch the summer blend story, menu highlights, and shop details.",
        status: PersonalTaskStatus::Done,
        priority: PersonalTaskPriority::High,
        labels: &["website", "launch"],
    },
    TaskSeed {
        id: 0x43b9_3003_0000_0000_0000_0000_0000_0002,
        title: "Update menu and opening hours",
        description: "Move seasonal content into the shared JSON files.",
        status: PersonalTaskStatus::Done,
        priority: PersonalTaskPriority::Medium,
        labels: &["content"],
    },
    TaskSeed {
        id: 0x43b9_3003_0000_0000_0000_0000_0000_0003,
        title: "Add a customer story",
        description: "Add one specific neighborhood story below the featured coffee.",
        status: PersonalTaskStatus::InProgress,
        priority: PersonalTaskPriority::Medium,
        labels: &["content", "design"],
    },
    TaskSeed {
        id: 0x43b9_3003_0000_0000_0000_0000_0000_0004,
        title: "Improve mobile navigation",
        description: "Keep ordering and location information reachable on small screens.",
        status: PersonalTaskStatus::Todo,
        priority: PersonalTaskPriority::High,
        labels: &["responsive", "accessibility"],
    },
];

const RELAY_TASKS: [TaskSeed; 4] = [
    TaskSeed {
        id: 0x43b9_3004_0000_0000_0000_0000_0000_0001,
        title: "Validate order creation",
        description: "Reject incomplete items and calculate prices from the server catalog.",
        status: PersonalTaskStatus::Done,
        priority: PersonalTaskPriority::Urgent,
        labels: &["api", "validation"],
    },
    TaskSeed {
        id: 0x43b9_3004_0000_0000_0000_0000_0000_0002,
        title: "Prevent duplicate webhooks",
        description: "Make each event and destination pair an idempotent delivery.",
        status: PersonalTaskStatus::InProgress,
        priority: PersonalTaskPriority::High,
        labels: &["webhooks", "reliability"],
    },
    TaskSeed {
        id: 0x43b9_3004_0000_0000_0000_0000_0000_0003,
        title: "Add request correlation IDs",
        description: "Carry a stable identifier through logs and error responses.",
        status: PersonalTaskStatus::Todo,
        priority: PersonalTaskPriority::Medium,
        labels: &["observability"],
    },
    TaskSeed {
        id: 0x43b9_3004_0000_0000_0000_0000_0000_0004,
        title: "Document retry policy",
        description: "Describe retryable status codes, backoff, and terminal failure handling.",
        status: PersonalTaskStatus::Done,
        priority: PersonalTaskPriority::Low,
        labels: &["docs"],
    },
];

const NORTHSTAR_AGENTS: [AgentSeed; 3] = [
    AgentSeed {
        id: 0x43b9_4001_0000_0000_0000_0000_0000_0001,
        title: "Build client overview",
        prompt: "Implement the client overview from the linked product brief. Reuse the existing visual language and keep pending decisions prominent.",
        result: "Built the responsive overview with project health, next milestone, open decisions, recent progress, and upcoming dates. Verified the local preview and narrow layout.",
        linked_doc: "choro_docs/product-brief.choro",
        changed_files: &[("index.html", 78, 9), ("styles.css", 41, 6)],
        task_index: 0,
        provider: AgentKind::Codex,
        status: AgentStatus::Done,
    },
    AgentSeed {
        id: 0x43b9_4001_0000_0000_0000_0000_0000_0002,
        title: "Review activation flow",
        prompt: "Review the activation flow for clarity, accessibility, and unnecessary setup. Return concrete changes.",
        result: "I mapped the current journey and found two setup steps that can move later. I’m now checking keyboard and error states before updating the acceptance criteria.",
        linked_doc: "choro_docs/activation-flow.choro",
        changed_files: &[("choro_docs/activation-flow.choro", 16, 7)],
        task_index: 2,
        provider: AgentKind::Claude,
        status: AgentStatus::InProgress,
    },
    AgentSeed {
        id: 0x43b9_4001_0000_0000_0000_0000_0000_0003,
        title: "Polish dashboard responsiveness",
        prompt: "Audit the portal below 850 pixels and fix hierarchy or overflow issues without redesigning the desktop page.",
        result: "Collapsed the sidebar at the compact breakpoint, stacked metrics and progress panels, and tightened page padding while retaining every client decision.",
        linked_doc: "choro_docs/product-brief.choro",
        changed_files: &[("styles.css", 19, 4)],
        task_index: 4,
        provider: AgentKind::Codex,
        status: AgentStatus::Done,
    },
];

const MOMENTUM_AGENTS: [AgentSeed; 2] = [
    AgentSeed {
        id: 0x43b9_4002_0000_0000_0000_0000_0000_0001,
        title: "Create today's habit list",
        prompt: "Build the SwiftUI habit list from the mobile brief with Dynamic Type and VoiceOver support.",
        result: "Added a focused habit model and SwiftUI list with one-tap completion, clear accessibility values, and a preview using fictional habits.",
        linked_doc: "choro_docs/mobile-product-brief.choro",
        changed_files: &[
            ("Sources/MomentumApp/Habit.swift", 28, 0),
            ("Sources/MomentumApp/HabitListView.swift", 39, 0),
        ],
        task_index: 0,
        provider: AgentKind::Codex,
        status: AgentStatus::Done,
    },
    AgentSeed {
        id: 0x43b9_4002_0000_0000_0000_0000_0000_0002,
        title: "Improve unfinished habits filter",
        prompt: "Add an accessible filter for unfinished habits and keep the empty state encouraging.",
        result: "The filter state and VoiceOver label are in place. I’m testing the completed-day empty state and Dynamic Type layout now.",
        linked_doc: "choro_docs/mobile-product-brief.choro",
        changed_files: &[("Sources/MomentumApp/HabitListView.swift", 22, 4)],
        task_index: 2,
        provider: AgentKind::Claude,
        status: AgentStatus::InProgress,
    },
];

const EMBER_AGENTS: [AgentSeed; 3] = [
    AgentSeed {
        id: 0x43b9_4003_0000_0000_0000_0000_0000_0001,
        title: "Build summer launch page",
        prompt: "Implement the summer launch brief as a warm, dependency-free website using the supplied brand board.",
        result: "Built the editorial hero, featured blend story, seasonal menu, and shop footer. The page runs through the local Preview script with no dependencies.",
        linked_doc: "choro_docs/launch-brief.choro",
        changed_files: &[("index.html", 64, 0), ("styles.css", 53, 0)],
        task_index: 0,
        provider: AgentKind::Codex,
        status: AgentStatus::Done,
    },
    AgentSeed {
        id: 0x43b9_4003_0000_0000_0000_0000_0000_0002,
        title: "Edit summer menu copy",
        prompt: "Review the menu and hero against the writing guidelines. Remove generic marketing language and keep the sensory details.",
        result: "Tightened the hero, replaced broad claims with ingredients and neighborhood details, and aligned every menu description with the Ember voice.",
        linked_doc: "choro_docs/writing-guidelines.choro",
        changed_files: &[("index.html", 18, 14), ("content/menu.json", 7, 7)],
        task_index: 1,
        provider: AgentKind::Claude,
        status: AgentStatus::Done,
    },
    AgentSeed {
        id: 0x43b9_4003_0000_0000_0000_0000_0000_0003,
        title: "Add customer story",
        prompt: "Add one specific neighborhood customer story below the featured coffee and match the Ember writing guidelines.",
        result: "I drafted the story and placed the new section below the featured blend. I’m refining the mobile spacing and final attribution treatment.",
        linked_doc: "choro_docs/writing-guidelines.choro",
        changed_files: &[("customer-story.html", 6, 0), ("styles.css", 14, 0)],
        task_index: 2,
        provider: AgentKind::Codex,
        status: AgentStatus::InProgress,
    },
];

const RELAY_AGENTS: [AgentSeed; 3] = [
    AgentSeed {
        id: 0x43b9_4004_0000_0000_0000_0000_0000_0001,
        title: "Validate order payloads",
        prompt: "Implement the create-order validation rules from the API contract and add focused tests.",
        result: "Added boundary validation for customer IDs, items, SKUs, and positive integer quantities, plus total calculation from the server catalog and passing Node tests.",
        linked_doc: "choro_docs/api-contract.choro",
        changed_files: &[("src/orders.js", 21, 0), ("tests/orders.test.js", 15, 0)],
        task_index: 0,
        provider: AgentKind::Codex,
        status: AgentStatus::Done,
    },
    AgentSeed {
        id: 0x43b9_4004_0000_0000_0000_0000_0000_0002,
        title: "Make webhook delivery idempotent",
        prompt: "Implement the idempotent delivery rule from the webhook reliability Doc and test duplicate attempts.",
        result: "The delivery key and duplicate guard are implemented. I’m running the retry regression cases and checking concurrent attempts now.",
        linked_doc: "choro_docs/webhook-reliability.choro",
        changed_files: &[
            ("src/webhooks.js", 17, 3),
            ("tests/webhooks.test.js", 16, 0),
        ],
        task_index: 1,
        provider: AgentKind::Codex,
        status: AgentStatus::InProgress,
    },
    AgentSeed {
        id: 0x43b9_4004_0000_0000_0000_0000_0000_0003,
        title: "Review retry policy",
        prompt: "Review the webhook policy for unsafe retry behavior, missing observability, and secret leakage.",
        result: "Separated transient and terminal failures, bounded the backoff window, retained non-secret attempt history, and added correlation requirements.",
        linked_doc: "choro_docs/webhook-reliability.choro",
        changed_files: &[("choro_docs/webhook-reliability.choro", 11, 5)],
        task_index: 3,
        provider: AgentKind::Claude,
        status: AgentStatus::Done,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_projects_have_unique_ids_folders_and_names() {
        for (index, project) in PROJECTS.iter().enumerate() {
            assert!(!project.files.is_empty());
            assert!(project
                .files
                .iter()
                .any(|(path, _)| *path == ".env.example"));
            assert!(project.files.iter().all(|(path, _)| *path != ".env"));
            assert!(PROJECTS[..index].iter().all(|other| {
                other.id != project.id
                    && other.folder != project.folder
                    && other.name != project.name
            }));
        }
    }

    #[test]
    fn every_demo_project_has_docs_tasks_agents_and_an_asset() {
        for project in PROJECTS {
            let agents = agent_seeds(project.id);
            assert!(project
                .files
                .iter()
                .any(|(path, _)| path.ends_with(".choro")));
            assert!(project
                .files
                .iter()
                .any(|(path, _)| *path == project.asset_path));
            assert!(task_seeds(project.id).len() >= 4);
            assert!(agents.len() >= 2);
            assert!(agents
                .iter()
                .any(|agent| agent.status == AgentStatus::InProgress));
            assert!(agents.iter().any(|agent| agent.status == AgentStatus::Done));
        }
    }

    #[test]
    fn reseed_decision_preserves_only_a_complete_matching_build() {
        let expected = "build-123:3";

        assert!(!needs_reseed(Some(expected), expected, false, true));
        assert!(needs_reseed(None, expected, false, true));
        assert!(needs_reseed(Some("corrupt"), expected, false, true));
        assert!(needs_reseed(Some("build-122:3"), expected, false, true));
        assert!(needs_reseed(Some(expected), expected, true, true));
        assert!(needs_reseed(Some(expected), expected, false, false));
    }

    #[test]
    fn project_env_is_generated_from_the_tracked_example() {
        let root = tempfile::tempdir().expect("temporary Demo project");
        let example = b"SAFE_DEMO_VALUE=replace-me\n";

        write_project_files(root.path(), &[(".env.example", example)])
            .expect("write Demo project files");

        assert_eq!(
            fs::read(root.path().join(".env")).expect("generated .env"),
            example
        );
    }

    #[test]
    fn jira_connection_persists_only_the_environment_placeholder() {
        let connection = base_demo_jira_connection();
        let serialized = serde_json::to_string(&connection).expect("serialize Jira connection");

        assert_eq!(connection.api_token, "${CHORO_DEMO_JIRA_TOKEN}");
        assert!(serialized.contains("${CHORO_DEMO_JIRA_TOKEN}"));
    }
}
