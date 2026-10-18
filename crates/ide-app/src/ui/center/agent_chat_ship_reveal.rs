//! Exact-Ship navigation for Actions routes (companion rows, platform
//! notifications and superseded Ship cards). Older history is paged in with the
//! same loader as scrolling and find, so loaded rows are never replaced.
use super::agent_chat_ship_actions::ship_actions_state;
use super::*;

/// Gives a just-opened conversation time to start hydrating before a missing
/// Ship is treated as gone.
const SHIP_REVEAL_GRACE: Duration = Duration::from_secs(3);

/// How long a banner click made before Actions are restored keeps its exact
/// Ship route. Restoring reads local storage, so this is a generous bound.
const ACTIONS_ROUTE_LIMIT: Duration = Duration::from_secs(60);

/// A notification route waiting for the watcher to restore its saved state.
#[derive(Clone, Debug)]
pub(super) struct PendingActionsRoute {
    watch_id: Uuid,
    project_id: ProjectId,
    agent_id: Uuid,
    requested_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActionsRouteStep {
    /// Scroll to the Ship and acknowledge this watch.
    Reveal,
    /// The watcher has not restored yet; keep the route.
    Wait,
    /// The user moved on, the route expired, or the watch is gone.
    Abandon,
}

/// `watch_known` means the watch exists and the routed conversation shipped it.
pub(super) fn actions_route_step(
    loaded: bool,
    watch_known: bool,
    still_viewing: bool,
    waited: Duration,
) -> ActionsRouteStep {
    if !still_viewing || waited >= ACTIONS_ROUTE_LIMIT {
        ActionsRouteStep::Abandon
    } else if watch_known {
        ActionsRouteStep::Reveal
    } else if loaded {
        ActionsRouteStep::Abandon
    } else {
        ActionsRouteStep::Wait
    }
}

#[derive(Clone, Debug)]
pub(super) struct PendingShipReveal {
    agent_id: Uuid,
    ship_id: String,
    requested_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ShipRevealHistory {
    pub has_more: bool,
    pub loading: bool,
    pub failed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShipRevealStep {
    Scroll(usize),
    LoadOlder,
    Wait,
    GiveUp,
}

pub(super) fn ship_reveal_step(
    list_index: Option<usize>,
    hydrating: bool,
    history: Option<ShipRevealHistory>,
    waited: Duration,
) -> ShipRevealStep {
    if let Some(index) = list_index {
        return ShipRevealStep::Scroll(index);
    }
    if hydrating {
        return ShipRevealStep::Wait;
    }
    match history {
        Some(history) if history.loading => ShipRevealStep::Wait,
        Some(history) if history.has_more && !history.failed => ShipRevealStep::LoadOlder,
        None if waited < SHIP_REVEAL_GRACE => ShipRevealStep::Wait,
        _ => ShipRevealStep::GiveUp,
    }
}

impl CenterArea {
    /// Scroll the conversation to one Ship once its row is loaded.
    pub(super) fn reveal_ship(&mut self, agent_id: Uuid, ship_id: String, cx: &mut Context<Self>) {
        self.pending_ship_reveal = Some(PendingShipReveal {
            agent_id,
            ship_id,
            requested_at: Instant::now(),
        });
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reconcile_ship_reveal(
        &mut self,
        agent: &AgentRecord,
        session: &AgentChatSession,
        rows: &[AgentChatRow],
        display_order: &[usize],
        list_state: &ListState,
        top_down: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.pending_ship_reveal.clone() else {
            return;
        };
        if target.agent_id != agent.id {
            // The user moved on; a late jump in another conversation would surprise them.
            if target.requested_at.elapsed() >= SHIP_REVEAL_GRACE {
                self.pending_ship_reveal = None;
            }
            return;
        }
        let list_index = session
            .timeline
            .iter()
            .position(|item| {
                matches!(item, AgentChatTimelineItem::ShipResult(ship) if ship.id == target.ship_id)
            })
            .and_then(|timeline_index| {
                rows.iter().position(|row| {
                    matches!(row, AgentChatRow::TimelineItem(index) if *index == timeline_index)
                })
            })
            .and_then(|row| display_order.iter().position(|source| *source == row));
        let history = self
            .agent_chat_history
            .get(&agent.id)
            .map(|history| ShipRevealHistory {
                has_more: history.has_more,
                loading: history.loading,
                failed: history.failed,
            });
        match ship_reveal_step(
            list_index,
            self.agent_chat_hydrating.contains(&agent.id),
            history,
            target.requested_at.elapsed(),
        ) {
            ShipRevealStep::Scroll(list_index) => {
                if list_index < list_state.item_count() {
                    list_state.splice(list_index..list_index + 1, 1);
                    list_state.scroll_to(gpui::ListOffset {
                        item_ix: list_index,
                        offset_in_item: px(0.),
                    });
                }
                let latest_index = if top_down {
                    0
                } else {
                    rows.len().saturating_sub(1)
                };
                self.agent_chat_scrolled_up
                    .insert(agent.id, list_index != latest_index);
                self.pending_ship_reveal = None;
            }
            ShipRevealStep::LoadOlder => self.load_older_agent_chat_history(agent.id, cx),
            ShipRevealStep::Wait => {}
            ShipRevealStep::GiveUp => self.pending_ship_reveal = None,
        }
    }

    /// Route for one Actions watch. Opens the latest Ship's conversation and
    /// acknowledges only this watch; the conversation's notices are untouched.
    /// Returns false when the watch or every conversation that shipped it is gone.
    pub(crate) fn open_ship_actions(
        &mut self,
        watch_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(state) = ship_actions_state(cx) else {
            return false;
        };
        let Some(watch) = state.read(cx).watches.get(&watch_id).cloned() else {
            return false;
        };
        let mut origins = watch.origins.clone();
        origins.sort_by_key(|origin| std::cmp::Reverse(origin.created_at));
        let Some(origin) = origins
            .into_iter()
            .find(|origin| self.agents.read(cx).agent(origin.agent_id).is_some())
        else {
            return false;
        };
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_active(watch.project_id, cx)
        });
        self.open_agent_without_acknowledgment(origin.agent_id, window, cx);
        self.reveal_ship_actions_origin(&state, watch_id, origin.agent_id, cx);
        true
    }

    /// Scrolls the open conversation to its latest Ship for this watch and
    /// acknowledges only that watch's notice.
    fn reveal_ship_actions_origin(
        &mut self,
        state: &Entity<crate::state::ship_actions::ShipActionsState>,
        watch_id: Uuid,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some(watch) = state.read(cx).watches.get(&watch_id).cloned() else {
            return;
        };
        let Some(origin) = super::ship_actions_model::latest_origin_for_agent(&watch, agent_id)
        else {
            return;
        };
        self.reveal_ship(agent_id, origin.ship_id.clone(), cx);
        if watch.unread() {
            state.update(cx, |state, cx| state.acknowledge(watch_id, cx));
        }
    }

    /// Entry point for an Actions platform banner. When the watcher has not
    /// restored yet, the conversation opens now (its notices untouched) and the
    /// exact Ship route is kept until restore, expiry, or the user moves on.
    pub(crate) fn route_ship_actions_notification(
        &mut self,
        watch_id: Uuid,
        project_id: ProjectId,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_actions_route = None;
        if self.open_ship_actions(watch_id, window, cx) {
            return;
        }
        if self.agents.read(cx).agent(agent_id).is_none() {
            return;
        }
        self.workspace
            .update(cx, |workspace, cx| workspace.set_active(project_id, cx));
        self.open_agent_without_acknowledgment(agent_id, window, cx);
        let loaded = ship_actions_state(cx).is_some_and(|state| state.read(cx).is_loaded());
        if !loaded {
            self.pending_actions_route = Some(PendingActionsRoute {
                watch_id,
                project_id,
                agent_id,
                requested_at: Instant::now(),
            });
        }
    }

    /// Called whenever the watcher changes; completes or drops a waiting route.
    pub(super) fn retry_pending_actions_route(&mut self, cx: &mut Context<Self>) {
        let Some(route) = self.pending_actions_route.clone() else {
            return;
        };
        let Some(state) = ship_actions_state(cx) else {
            self.pending_actions_route = None;
            return;
        };
        let still_viewing = self.workspace.read(cx).active == Some(route.project_id)
            && self.agents.read(cx).selected_agent_id(route.project_id) == Some(route.agent_id);
        let (loaded, watch_known) = {
            let state = state.read(cx);
            let known = state.watches.get(&route.watch_id).is_some_and(|watch| {
                super::ship_actions_model::latest_origin_for_agent(watch, route.agent_id).is_some()
            });
            (state.is_loaded(), known)
        };
        match actions_route_step(
            loaded,
            watch_known,
            still_viewing,
            route.requested_at.elapsed(),
        ) {
            ActionsRouteStep::Wait => {}
            ActionsRouteStep::Abandon => self.pending_actions_route = None,
            ActionsRouteStep::Reveal => {
                self.pending_actions_route = None;
                self.reveal_ship_actions_origin(&state, route.watch_id, route.agent_id, cx);
            }
        }
    }

    /// Puts an investigation request in the composer for the user to review.
    /// It is never sent from here, and an existing draft is kept above it.
    pub(super) fn draft_ship_actions_investigation(
        &mut self,
        agent_id: Uuid,
        watch_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(watch) =
            ship_actions_state(cx).and_then(|state| state.read(cx).watches.get(&watch_id).cloned())
        else {
            return;
        };
        let Some(input) = self.agent_chat_inputs.get(&agent_id).cloned() else {
            window.push_notification(
                Notification::info("Open the conversation's composer to draft an investigation"),
                cx,
            );
            return;
        };
        let draft = super::ship_actions_model::investigation_draft(&watch);
        let current = input.read(cx).value().to_string();
        let text = if current.trim().is_empty() {
            draft
        } else {
            format!("{}\n\n{draft}", current.trim_end())
        };
        input.update(cx, |input, cx| {
            input.set_value(text.clone(), window, cx);
            input.set_cursor_position(
                input_position_for_byte_offset(&text, text.len()),
                window,
                cx,
            );
            input.focus(window, cx);
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: ShipRevealHistory = ShipRevealHistory {
        has_more: false,
        loading: false,
        failed: false,
    };

    #[test]
    fn a_loaded_ship_is_scrolled_to_immediately() {
        assert_eq!(
            ship_reveal_step(Some(4), true, None, Duration::ZERO),
            ShipRevealStep::Scroll(4)
        );
    }

    #[test]
    fn missing_ships_page_older_history_until_exhausted() {
        let more = ShipRevealHistory {
            has_more: true,
            ..IDLE
        };
        assert_eq!(
            ship_reveal_step(None, false, Some(more), Duration::ZERO),
            ShipRevealStep::LoadOlder
        );
        let loading = ShipRevealHistory {
            loading: true,
            ..more
        };
        assert_eq!(
            ship_reveal_step(None, false, Some(loading), Duration::ZERO),
            ShipRevealStep::Wait
        );
        assert_eq!(
            ship_reveal_step(None, true, Some(more), Duration::ZERO),
            ShipRevealStep::Wait
        );
        assert_eq!(
            ship_reveal_step(None, false, Some(IDLE), Duration::ZERO),
            ShipRevealStep::GiveUp
        );
        let failed = ShipRevealHistory {
            failed: true,
            ..more
        };
        assert_eq!(
            ship_reveal_step(None, false, Some(failed), Duration::ZERO),
            ShipRevealStep::GiveUp
        );
    }

    #[test]
    fn early_banner_routes_wait_for_restore_then_reveal_the_exact_ship() {
        let soon = Duration::from_secs(1);
        assert_eq!(
            actions_route_step(false, false, true, soon),
            ActionsRouteStep::Wait
        );
        assert_eq!(
            actions_route_step(true, true, true, soon),
            ActionsRouteStep::Reveal
        );
    }

    #[test]
    fn early_banner_routes_end_on_navigation_expiry_or_a_missing_watch() {
        let soon = Duration::from_secs(1);
        // The user opened something else; never jump them back.
        assert_eq!(
            actions_route_step(true, true, false, soon),
            ActionsRouteStep::Abandon
        );
        assert_eq!(
            actions_route_step(false, false, false, soon),
            ActionsRouteStep::Abandon
        );
        assert_eq!(
            actions_route_step(false, false, true, ACTIONS_ROUTE_LIMIT),
            ActionsRouteStep::Abandon
        );
        // Restored without this watch (deleted or archived): stay in the chat.
        assert_eq!(
            actions_route_step(true, false, true, soon),
            ActionsRouteStep::Abandon
        );
    }

    #[test]
    fn an_unopened_conversation_gets_time_to_hydrate() {
        assert_eq!(
            ship_reveal_step(None, false, None, Duration::from_millis(200)),
            ShipRevealStep::Wait
        );
        assert_eq!(
            ship_reveal_step(None, false, None, SHIP_REVEAL_GRACE),
            ShipRevealStep::GiveUp
        );
    }
}
