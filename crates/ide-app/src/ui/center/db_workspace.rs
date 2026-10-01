//! The center database workspace. A pinned Connections tab (the home)
//! leads a strip of object tabs; the open object sits under a context bar
//! naming its connection, namespace and live access posture. The explorer
//! in the side panel and the home share one source of connection state.

use super::*;
use crate::ui::db::db_panel::DbPanel;
use crate::ui::db::workspace::{self as db_workspace, DbTabMeta};

/// Stored in `selected_db` while a project's Connections home is showing.
const DB_HOME_KEY: &str = "\u{0}connections-home";

/// The object tab on screen, given the project's tabs in strip order and
/// the stored selection. `None` means the Connections home.
pub(super) fn resolve_db_selection(keys: &[String], selected: Option<&str>) -> Option<String> {
    if selected == Some(DB_HOME_KEY) || keys.is_empty() {
        return None;
    }
    selected
        .filter(|selected| keys.iter().any(|key| key == selected))
        .or_else(|| keys.first().map(String::as_str))
        .map(str::to_string)
}

/// The selection to store after closing `closed`. Closing a background tab
/// keeps the current tab (or the home); closing the tab on screen moves to
/// its right-hand neighbour, else its left, else the home.
pub(super) fn db_selection_after_close(
    keys: &[String],
    selected: Option<&str>,
    closed: &str,
) -> Option<String> {
    if resolve_db_selection(keys, selected).as_deref() != Some(closed) {
        return selected.map(str::to_string);
    }
    let index = keys.iter().position(|key| key == closed)?;
    keys.get(index + 1)
        .or_else(|| index.checked_sub(1).and_then(|left| keys.get(left)))
        .cloned()
}

impl CenterArea {
    /// The project's object tabs in strip order.
    pub(super) fn db_tab_keys(&self, project: ProjectId) -> Vec<String> {
        self.db_views
            .iter()
            .filter(|view| view.project == project)
            .map(|view| view.key.clone())
            .collect()
    }

    /// Links the side-panel explorer so the home can read and drive it.
    pub fn attach_db_explorer(&mut self, panel: Entity<DbPanel>, cx: &mut Context<Self>) {
        cx.observe(&panel, |_, _, cx| cx.notify()).detach();
        self.db_explorer = Some(panel);
    }

    /// Lets the explorer re-mark the open object. Deferred because tab
    /// changes are often requested from inside the explorer's own update.
    pub(super) fn notify_db_explorer(&self, cx: &mut Context<Self>) {
        if let Some(panel) = self.db_explorer.clone() {
            cx.defer(move |cx| panel.update(cx, |_, cx| cx.notify()));
        }
    }

    /// Identity of the object tab on screen in the active project, if any.
    pub fn active_db_object_key(&self, cx: &App) -> Option<String> {
        let (project, _) = self.active_project(cx)?;
        self.active_db_key(project)
    }

    fn show_db_home(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        self.selected_db.insert(project, DB_HOME_KEY.into());
        self.notify_db_explorer(cx);
        cx.notify();
    }

    fn select_db_tab(&mut self, project: ProjectId, key: String, cx: &mut Context<Self>) {
        self.selected_db.insert(project, key);
        self.notify_db_explorer(cx);
        cx.notify();
    }

    pub(super) fn render_db_section(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let tabs: Vec<(String, DbTabMeta)> = self
            .db_views
            .iter()
            .filter(|view| view.project == project)
            .map(|view| (view.key.clone(), view.meta.clone()))
            .collect();
        let active_key = self.active_db_key(project);
        let active = active_key.as_ref().and_then(|key| {
            self.db_views
                .iter()
                .find(|view| view.project == project && &view.key == key)
        });

        let strip = self.render_db_tab_strip(project, &tabs, active_key.as_deref(), cx);

        let body = match active {
            Some(item) => {
                let read_only = crate::ui::db::chrome::handle_read_only(&item.connection);
                v_flex()
                    .size_full()
                    .child(db_workspace::context_bar(&item.meta, read_only, cx))
                    .child(
                        div()
                            .debug_selector(|| "db-document".into())
                            .flex_1()
                            .min_h(px(0.))
                            .child(item.view.clone()),
                    )
                    .into_any_element()
            }
            None => {
                let connections = self
                    .workspace
                    .read(cx)
                    .projects
                    .iter()
                    .find(|item| item.id == project)
                    .map(|item| item.db_connections.clone())
                    .unwrap_or_default();
                crate::ui::db::home::render_home(
                    self.db_explorer.clone(),
                    self.workspace.clone(),
                    connections,
                    cx,
                )
            }
        };

        v_flex()
            .debug_selector(|| "db-workspace".into())
            .size_full()
            .bg(crate::ui::design::base(cx))
            .child(strip)
            .child(div().flex_1().min_h(px(0.)).child(body))
            .into_any_element()
    }

    fn render_db_tab_strip(
        &self,
        project: ProjectId,
        tabs: &[(String, DbTabMeta)],
        active_key: Option<&str>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let tab_elements = tabs
            .iter()
            .enumerate()
            .map(|(ix, (key, meta))| {
                let selected = active_key == Some(key.as_str());
                let group_name: SharedString = format!("db-tab-{ix}").into();
                let select_key = key.clone();
                let close_key = key.clone();
                let tooltip = meta.tooltip();
                db_workspace::object_tab(ix, meta, selected, cx)
                    .group(group_name.clone())
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_db_tab(project, select_key.clone(), cx);
                    }))
                    .child(
                        div()
                            .flex_none()
                            .invisible()
                            .when(selected, |slot| slot.visible())
                            .group_hover(group_name, |slot| slot.visible())
                            .child(
                                style::strip_tab_close_button(("db-tab-close", ix), cx)
                                    .tooltip("Close tab")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.close_db_view(project, &close_key, cx);
                                    })),
                            ),
                    )
            })
            .collect::<Vec<_>>();

        let menu_items = tabs
            .iter()
            .map(|(key, meta)| {
                (
                    key.clone(),
                    SharedString::from(format!("{}.{}", meta.namespace, meta.object)),
                )
            })
            .collect::<Vec<_>>();
        let menu_center = cx.entity().downgrade();
        let menu_active = active_key.map(str::to_string);

        db_workspace::tab_strip(cx)
            .debug_selector(|| "db-tab-strip".into())
            .child(
                db_workspace::home_tab(active_key.is_none(), cx).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.show_db_home(project, cx);
                    },
                )),
            )
            .when(!tabs.is_empty(), |strip| {
                strip.child(
                    div()
                        .flex_none()
                        .w(px(1.))
                        .h(px(16.))
                        .mx_1()
                        .mb(px(8.))
                        .bg(crate::ui::design::line(cx).opacity(0.5)),
                )
            })
            .child(
                h_flex()
                    .id("db-tab-scroll")
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .gap_0p5()
                    .items_end()
                    .overflow_x_scroll()
                    .children(tab_elements),
            )
            .when(tabs.len() > 1, |strip| {
                strip.child(
                    div().flex_none().mb(px(4.)).child(
                        style::header_icon_button("db-tab-list", IconName::ChevronDown, cx)
                            .tooltip("Open tabs")
                            .dropdown_menu(move |mut menu, window, _| {
                                for (key, label) in menu_items.iter().cloned() {
                                    let Some(center) = menu_center.upgrade() else {
                                        break;
                                    };
                                    let checked = menu_active.as_deref() == Some(key.as_str());
                                    menu = menu.item(
                                        PopupMenuItem::new(label).checked(checked).on_click(
                                            window.listener_for(
                                                &center,
                                                move |this: &mut Self, _, _, cx| {
                                                    this.select_db_tab(project, key.clone(), cx);
                                                },
                                            ),
                                        ),
                                    );
                                }
                                menu
                            }),
                    ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn selection_resolves_to_home_a_valid_tab_or_the_first_tab() {
        let tabs = keys(&["a", "b", "c"]);
        assert_eq!(resolve_db_selection(&[], Some("a")), None);
        assert_eq!(resolve_db_selection(&tabs, Some(DB_HOME_KEY)), None);
        assert_eq!(resolve_db_selection(&tabs, Some("b")).as_deref(), Some("b"));
        assert_eq!(
            resolve_db_selection(&tabs, Some("gone")).as_deref(),
            Some("a")
        );
        assert_eq!(resolve_db_selection(&tabs, None).as_deref(), Some("a"));
    }

    #[test]
    fn closing_tabs_keeps_or_moves_the_selection_predictably() {
        let tabs = keys(&["a", "b", "c"]);
        // A background tab closes without moving the view.
        assert_eq!(
            db_selection_after_close(&tabs, Some("b"), "c").as_deref(),
            Some("b")
        );
        assert_eq!(
            db_selection_after_close(&tabs, Some(DB_HOME_KEY), "a").as_deref(),
            Some(DB_HOME_KEY)
        );
        // The tab on screen hands over to its right, then its left neighbour.
        assert_eq!(
            db_selection_after_close(&tabs, Some("b"), "b").as_deref(),
            Some("c")
        );
        assert_eq!(
            db_selection_after_close(&tabs, Some("c"), "c").as_deref(),
            Some("b")
        );
        // With no stored choice the first tab is on screen.
        assert_eq!(
            db_selection_after_close(&tabs, None, "a").as_deref(),
            Some("b")
        );
        // The last tab closes to the home.
        let last = keys(&["a"]);
        let next = db_selection_after_close(&last, Some("a"), "a");
        assert_eq!(next, None);
        assert_eq!(resolve_db_selection(&[], next.as_deref()), None);
    }

    #[test]
    fn explorer_highlight_matches_the_resolved_tab_key() {
        let id = uuid::Uuid::new_v4();
        let users = crate::ui::db::db_panel::object_key(id, "public", "users");
        let orders = crate::ui::db::db_panel::object_key(id, "public", "orders");
        let tabs = vec![users.clone(), orders.clone()];
        assert_eq!(resolve_db_selection(&tabs, Some(&orders)), Some(orders));
        assert_eq!(resolve_db_selection(&tabs, Some(DB_HOME_KEY)), None);
    }
}
