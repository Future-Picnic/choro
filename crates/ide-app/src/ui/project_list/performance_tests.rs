use super::*;
use crate::state::agent_chat::AgentChatStatus;
use gpui::{AnyView, EntityInputHandler, StyleRefinement};
use ide_core::AgentRecord;
use std::{cell::Cell, rc::Rc};

struct Foreground(Rc<Cell<usize>>);
impl Render for Foreground {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.set(self.0.get() + 1);
        div()
            .size_full()
            .child("Independent transcript / Studio content boundary")
    }
}

struct Fixture {
    sidebar: Entity<ProjectList>,
    foreground: Entity<Foreground>,
    input: Entity<InputState>,
    shown: bool,
}
impl Render for Fixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.set_visible(self.shown, cx));
        h_flex()
            .size_full()
            .when(self.shown, |root| {
                root.child(
                    div()
                        .relative()
                        .w(px(280.))
                        .h_full()
                        .child(self.sidebar.clone())
                        .child(self.sidebar.read(cx).activity_layer.clone()),
                )
            })
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .child(
                        AnyView::from(self.foreground.clone())
                            .cached(StyleRefinement::default().flex_1().w_full()),
                    )
                    .child(Input::new(&self.input)),
            )
    }
}

fn count(label: &str) -> u64 {
    crate::ui::performance::probes::snapshot()
        .get(label)
        .map_or(0, |sample| sample.count)
}

#[gpui::test]
fn real_sidebar_isolates_streaming_animation_and_typing(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    virtual_rows::RENDERED_AGENTS.with(|ids| ids.borrow_mut().clear());
    let renders = Rc::new(Cell::new(0));
    let mut fixture = None;
    let mut state = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let project = ide_core::Project::from_path(PathBuf::from("/in-memory/sidebar-stress"));
        let project_id = project.id;
        let mut config = ide_core::AppConfig::default();
        config.projects = vec![project.clone()];
        config.active_project = Some(project_id);
        config.expanded_projects = vec![project_id];
        config.sidebar_active_work = false;
        let workspace = cx.new(|_| Workspace::in_memory(config));
        let agents = (0..1_000)
            .map(|i| {
                let provider = ide_core::AgentKind::Codex;
                let model = ide_core::AgentModel::default_for(provider);
                let mut agent = AgentRecord::new(
                    project_id,
                    project.path.clone(),
                    format!("Agent {i}"),
                    "",
                    provider,
                    model,
                    model.default_effort(),
                    ide_core::AgentAccessMode::FullAccess,
                );
                agent.status = AgentStatus::InProgress;
                agent.runtime = ide_core::AgentRuntimeKind::Chat;
                agent
            })
            .collect::<Vec<_>>();
        let chats = cx.new(|_| AgentChatState::new());
        chats.update(cx, |chats, cx| {
            for agent in &agents {
                chats.fixture_change(agent.id, AgentChatStatus::Running, "", cx);
            }
        });
        let records = cx.new(|_| AgentRecords::in_memory(agents));
        let terminals = cx.new(|_| TerminalManager::new());
        let activity = cx.new(|_| {
            AgentActivityCache::in_memory(records.clone(), chats.clone(), terminals.clone())
        });
        let git = cx.new(|_| GitStates::in_memory(workspace.clone()));
        let sidebar = ProjectList::view(
            workspace,
            git,
            terminals,
            chats.clone(),
            records,
            activity,
            None,
            cx,
        );
        sidebar.update(cx, |list, _| {
            list.expanded_agent_lists.insert(project_id);
        });
        state = Some(chats);
        let view = cx.new(|cx| Fixture {
            sidebar,
            foreground: cx.new(|_| Foreground(renders.clone())),
            input: cx.new(|cx| InputState::new(window, cx)),
            shown: true,
        });
        fixture = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let view = fixture.unwrap();
    let chats = state.unwrap();
    cx.run_until_parked();
    let id = virtual_rows::RENDERED_AGENTS.with(|ids| {
        *ids.borrow()
            .first()
            .expect("at least one agent must be painted")
    });
    let model = count("sidebar.model");
    let rows = count("sidebar.row");
    let projection = count("sidebar.projection");
    let foreground = renders.get();
    assert!(rows > 0, "the fixture must actually paint sidebar rows");
    eprintln!("initial model={model} projection={projection} rows={rows}");
    assert!(
        rows < 100,
        "1,000 records must render only visible rows: {rows}"
    );
    let shells = count("sidebar.shell");
    for _ in 0..100 {
        cx.update(|_, cx| {
            chats.update(cx, |chats, cx| {
                chats.fixture_change(id, AgentChatStatus::Running, "background text", cx)
            });
        });
        cx.run_until_parked();
    }
    assert_eq!(
        count("sidebar.shell"),
        shells,
        "background text must not schedule sidebar frames"
    );
    for _ in 0..100 {
        cx.update(|_, cx| {
            chats.update(cx, |chats, cx| {
                chats.fixture_change(id, AgentChatStatus::Running, "streaming text", cx)
            });
            let layer = view.read(cx).sidebar.read(cx).activity_layer.clone();
            layer.update(cx, |_, cx| cx.notify());
        });
        cx.run_until_parked();
    }
    for _ in 0..50 {
        cx.update(|window, cx| {
            let input = view.read(cx).input.clone();
            input.update(cx, |input, cx| {
                input.replace_text_in_range(None, "a", window, cx)
            });
        });
        cx.run_until_parked();
    }
    assert_eq!(
        count("sidebar.model"),
        model,
        "text and animation cannot recompute navigation"
    );
    assert_eq!(
        count("sidebar.projection"),
        projection,
        "unchanged presentation must stay cached"
    );
    assert_eq!(
        count("sidebar.row"),
        rows,
        "animation and typing cannot rebuild rows"
    );
    assert_eq!(
        renders.get(),
        foreground,
        "the foreground render boundary must remain independent"
    );

    // A status change affects its visible row on the next eligible frame.
    cx.update(|_, cx| {
        chats.update(cx, |chats, cx| {
            chats.fixture_change(id, AgentChatStatus::Failed, "", cx)
        })
    });
    cx.run_until_parked();
    eprintln!(
        "after failure model={} projection={} rows={}",
        count("sidebar.model"),
        count("sidebar.projection"),
        count("sidebar.row")
    );
    assert_eq!(
        count("sidebar.row"),
        rows + 1,
        "only the changed row rebuilds"
    );
    cx.update(|_, cx| {
        let sidebar = &view.read(cx).sidebar;
        assert_eq!(
            sidebar.read(cx).model.read(cx).agents[&id].runtime,
            ProjectAgentRuntime::Ended
        );
    });

    // Measure an actual series of transitions, not a percentile of one event.
    for index in 0..100 {
        cx.update(|_, cx| {
            chats.update(cx, |chats, cx| {
                chats.fixture_change(
                    id,
                    if index % 2 == 0 {
                        AgentChatStatus::Running
                    } else {
                        AgentChatStatus::Failed
                    },
                    "",
                    cx,
                )
            })
        });
        cx.run_until_parked();
    }
    assert_eq!(count("sidebar.row"), rows + 101);
    assert_eq!(
        renders.get(),
        foreground,
        "background status must preserve foreground content"
    );

    // Hiding stops painting, but tracking still receives completion. Reopening
    // reads the current projection without waiting for another provider event.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.shown = false;
            cx.notify();
        })
    });
    cx.run_until_parked();
    let hidden_rows = count("sidebar.row");
    let hidden_frames = count("sidebar.animation");
    let hidden_projection = count("sidebar.projection");
    cx.update(|_, cx| {
        chats.update(cx, |chats, cx| {
            chats.fixture_change(id, AgentChatStatus::Idle, "", cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(count("sidebar.row"), hidden_rows);
    assert_eq!(count("sidebar.animation"), hidden_frames);
    assert_eq!(count("sidebar.projection"), hidden_projection);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.shown = true;
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).sidebar.read(cx).model.read(cx).agents[&id].runtime,
            ProjectAgentRuntime::Idle
        )
    });
    cx.simulate_resize(gpui::size(px(840.), px(700.)));
    cx.run_until_parked();
    let samples = crate::ui::performance::probes::snapshot();
    for label in [
        "sidebar.model",
        "sidebar.projection",
        "sidebar.row",
        "sidebar.animation",
        "sidebar.event_to_paint",
    ] {
        if let Some(sample) = samples.get(label) {
            eprintln!(
                "fixture {label}: count={} p95={}us",
                sample.count,
                sample.percentile(95)
            );
        }
    }
}
