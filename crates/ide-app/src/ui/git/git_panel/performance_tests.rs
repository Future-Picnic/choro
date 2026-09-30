use super::*;
use crate::ui::performance::probes;
use ide_core::git::{ChangeKind, GitSnapshot, HeadInfo, LineStats, StatusEntry};

struct Fixture {
    panel: Entity<GitPanel>,
}

impl Render for Fixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(360.)).h(px(600.)).child(self.panel.clone())
    }
}

fn renders() -> u64 {
    probes::snapshot()
        .get("git_panel.render")
        .map_or(0, |sample| sample.count)
}

#[gpui::test]
fn git_panel_redraws_without_blocking_git_and_keeps_inputs_and_updates_live(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(gpui_component::init);
    let mut fixture = None;
    let mut state = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let project = ide_core::Project::from_path(PathBuf::from("/in-memory/git-panel"));
        let project_id = project.id;
        let config = ide_core::AppConfig {
            projects: vec![project.clone()],
            active_project: Some(project_id),
            ..Default::default()
        };
        let workspace = cx.new(|_| Workspace::in_memory(config));
        let git = cx.new(|_| {
            let mut state = GitState::in_memory(
                project.path.clone(),
                GitSnapshot {
                    head: HeadInfo {
                        branch: Some("main".into()),
                        ..Default::default()
                    },
                    origin_default_branch: Some("main".into()),
                    entries: (0..1_000)
                        .map(|i| StatusEntry {
                            path: PathBuf::from(format!("src/module{}/file{i}.rs", i / 10)),
                            staged: None,
                            unstaged: Some(ChangeKind::Modified),
                            staged_stats: LineStats::default(),
                            unstaged_stats: LineStats {
                                insertions: 1,
                                deletions: 0,
                            },
                        })
                        .collect(),
                    ..Default::default()
                },
            );
            state.last_message = Some("Pushed main to origin".into());
            state
        });
        let states = cx.new(|_| GitStates::in_memory(workspace.clone()));
        states.update(cx, |states, cx| {
            states.insert_in_memory(project_id, git.clone(), cx)
        });
        let agents = cx.new(|_| AgentRecords::in_memory(Vec::new()));
        let input = cx.new(|cx| InputState::new(window, cx).multi_line(true));
        let query = cx.new(|cx| InputState::new(window, cx));
        let panel = cx.new(|cx| {
            cx.observe(&states, |_, _, cx| cx.notify()).detach();
            GitPanel::new(
                workspace,
                states,
                agents,
                gpui::WeakEntity::new_invalid(),
                input,
                query,
            )
        });
        let view = cx.new(|_| Fixture { panel });
        fixture = Some(view.clone());
        state = Some(git);
        gpui_component::Root::new(view, window, cx)
    });
    let fixture = fixture.unwrap();
    let git = state.unwrap();
    cx.run_until_parked();
    let start = renders();
    let calls = generation::GIT_OUTPUT_CALLS.with(|calls| calls.get());
    for _ in 0..30 {
        cx.update(|_, cx| fixture.update(cx, |_, cx| cx.notify()));
        cx.run_until_parked();
    }
    let redraws = renders() - start;
    assert!(redraws >= 30, "the fixture must really render the panel");
    assert_eq!(
        generation::GIT_OUTPUT_CALLS.with(|calls| calls.get()),
        calls,
        "a persistent push notice must not run Git on the UI thread"
    );

    // External Git changes still cause the panel to redraw.
    let start = renders();
    cx.update(|_, cx| {
        git.update(cx, |git, cx| {
            git.snapshot.as_mut().unwrap().entries.pop();
            cx.notify();
        })
    });
    cx.run_until_parked();
    assert!(renders() > start);
    let panel = cx.update(|_, cx| fixture.read(cx).panel.clone());
    let before_typing = renders();
    cx.update(|window, cx| {
        let input = panel.read(cx).commit_input.clone();
        input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
    });
    cx.simulate_input("Typing after Git refresh");
    cx.run_until_parked();
    assert!(renders() > before_typing, "editing must redraw the panel");
    cx.update(|_, cx| {
        assert_eq!(
            panel.read(cx).commit_input.read(cx).value().as_ref(),
            "Typing after Git refresh"
        );
    });

    // Repeated push notices must never invoke the blocking Git helper on render.
    let calls = generation::GIT_OUTPUT_CALLS.with(|calls| calls.get());
    for _ in 0..10 {
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.last_push_notice_message = None;
                panel.sync_push_notice(git.clone(), window, cx);
                cx.notify();
            })
        });
        cx.run_until_parked();
    }
    assert_eq!(
        generation::GIT_OUTPUT_CALLS.with(|calls| calls.get()),
        calls
    );
    let samples = probes::snapshot();
    let sample = &samples["git_panel.render"];
    eprintln!(
        "Git panel, 1,000 files: redraws={redraws}, blocking Git calls=0, render p95={}us",
        sample.percentile(95)
    );
}

#[gpui::test]
fn pull_request_responses_require_current_request_and_scope(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    cx.add_window_view(|window, cx| {
        let workspace = cx.new(|_| Workspace::in_memory(Default::default()));
        let states = cx.new(|_| GitStates::in_memory(workspace.clone()));
        let agents = cx.new(|_| AgentRecords::in_memory(Vec::new()));
        let input = cx.new(|cx| InputState::new(window, cx));
        let query = cx.new(|cx| InputState::new(window, cx));
        let mut panel = GitPanel::new(
            workspace,
            states,
            agents,
            gpui::WeakEntity::new_invalid(),
            input,
            query,
        );
        let key = PullRequestLookupKey {
            repo_path: PathBuf::from("/in-memory/a"),
            branch: "feature/a".into(),
        };
        panel.branch_pr_key = Some(key.clone());
        panel.branch_pr_generation = 3;
        panel.repo_pr_key = Some(key.repo_path.clone());
        panel.repo_pr_generation = 3;
        // Returning to A must not accept the earlier request for A.
        assert!(!panel.branch_pr_result_is_current(&key, 1));
        assert!(!panel.repo_pr_result_is_current(&key.repo_path, 1));
        assert!(panel.branch_pr_result_is_current(&key, 3));
        assert!(panel.repo_pr_result_is_current(&key.repo_path, 3));
        let other = PullRequestLookupKey {
            branch: "feature/b".into(),
            ..key
        };
        assert!(!panel.branch_pr_result_is_current(&other, 3));
        assert!(!panel.repo_pr_result_is_current(Path::new("/in-memory/b"), 3));
        gpui_component::Root::new(cx.new(|_| panel), window, cx)
    });
}
