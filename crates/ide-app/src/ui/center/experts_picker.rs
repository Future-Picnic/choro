//! A recipient selector; choosing a recipient never submits or edits the draft.
use super::*;
use crate::ui::{design, style};
use gpui::{AnyElement, Focusable};
use gpui_component::{
    list::{List, ListDelegate, ListItem, ListState},
    popover::Popover,
    IndexPath,
};

#[derive(Clone)]
struct Recipient {
    id: Option<Uuid>,
    name: String,
    description: String,
}

pub(super) struct DelegatePicker {
    owner: gpui::WeakEntity<CenterArea>,
    parent: Uuid,
    choices: Vec<Recipient>,
    filtered: Vec<Recipient>,
    selected: Option<IndexPath>,
    committed: Option<Uuid>,
}

impl DelegatePicker {
    fn recipient(&self, ix: IndexPath) -> Option<&Recipient> {
        let offset = if ix.section == 1 { 1 } else { 0 };
        self.filtered.get(ix.row + offset)
    }
    fn split_sections(&self) -> bool {
        self.filtered.first().is_some_and(|c| c.id.is_none()) && self.filtered.len() > 1
    }
    fn close(&self, choice: Option<Option<Uuid>>, window: &mut Window, cx: &mut App) {
        let _ = self.owner.update(cx, |this, cx| {
            // A dismissed picker must not re-enable a cancelled assignment.
            if let Some(choice) = choice {
                if let Some(selection) = this.delegation_selection.get_mut(&self.parent) {
                    *selection = choice;
                }
            }
            this.delegation_picker = None;
            if let Some(input) = this.agent_chat_inputs.get(&self.parent) {
                input.update(cx, |input, cx| input.focus(window, cx));
            }
            cx.notify();
        });
    }
}

impl ListDelegate for DelegatePicker {
    type Item = ListItem;
    fn sections_count(&self, _: &App) -> usize {
        if self.split_sections() {
            2
        } else {
            1
        }
    }
    fn items_count(&self, section: usize, _: &App) -> usize {
        if self.split_sections() {
            if section == 0 {
                1
            } else {
                self.filtered.len() - 1
            }
        } else {
            self.filtered.len()
        }
    }
    fn render_section_header(
        &mut self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        Some(
            div()
                .px_3()
                .py_1()
                .text_size(design::text_label())
                .text_color(design::t3(cx))
                .child(
                    if self.split_sections() && section == 0
                        || self.filtered.first().is_some_and(|c| c.id.is_none())
                            && !self.split_sections()
                    {
                        "For this task"
                    } else {
                        "Saved bandmates"
                    },
                ),
        )
    }
    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
    }
    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> gpui::Task<()> {
        let query = query.to_lowercase();
        self.filtered = self
            .choices
            .iter()
            .filter(|choice| {
                format!("{} {}", choice.name, choice.description)
                    .to_lowercase()
                    .contains(&query)
            })
            .cloned()
            .collect();
        gpui::Task::ready(())
    }
    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let choice = self.recipient(ix)?;
        Some(
            ListItem::new(ix)
                .h(px(56.))
                .confirmed(choice.id == self.committed)
                .check_icon(Icon::new(IconName::Check))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap_1()
                        .child(
                            div()
                                .truncate()
                                .text_size(design::text_ui())
                                .text_color(design::t1(cx))
                                .child(choice.name.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(design::text_label())
                                .text_color(design::t3(cx))
                                .child(choice.description.clone()),
                        ),
                ),
        )
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .p_4()
            .text_size(design::text_ui())
            .text_color(design::t3(cx))
            .child("No matching bandmates")
    }
    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        if let Some(choice) = self
            .selected
            .and_then(|ix| self.recipient(ix))
            .map(|c| c.id)
        {
            self.close(Some(choice), window, cx);
        }
    }
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        self.close(None, window, cx);
    }
}

impl CenterArea {
    fn open_delegation_picker(
        &mut self,
        parent: Uuid,
        selection: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut choices = vec![Recipient {
            id: None,
            name: "On-demand teammate".into(),
            description: "Uses this chat’s model · role comes from your brief".into(),
        }];
        choices.extend(super::experts::profiles().into_iter().map(|p| Recipient {
            id: Some(p.id),
            name: p.name,
            description: p.description,
        }));
        let selected = choices.iter().position(|c| c.id == selection).unwrap_or(0);
        let delegate = DelegatePicker {
            owner: cx.entity().downgrade(),
            parent,
            filtered: choices.clone(),
            choices,
            selected: None,
            committed: selection,
        };
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            list.set_selected_index(
                Some(if selected == 0 {
                    IndexPath::new(0)
                } else {
                    IndexPath {
                        section: 1,
                        row: selected - 1,
                        column: 0,
                    }
                }),
                window,
                cx,
            );
            list.focus(window, cx);
        });
        self.delegation_picker = Some((parent, list));
        cx.notify();
    }

    pub(super) fn render_delegation_recipient(
        &mut self,
        parent: Uuid,
        selection: Option<Uuid>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let profiles = super::experts::profiles();
        let label = selection
            .map(|id| {
                profiles
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "Unavailable bandmate".into())
            })
            .unwrap_or_else(|| "On-demand teammate".into());
        let list = self
            .delegation_picker
            .as_ref()
            .filter(|(id, _)| *id == parent)
            .map(|(_, list)| list.clone());
        let owner = cx.entity().downgrade();
        let content_list = list.clone();
        let mut popover = Popover::new(("delegation-recipient", parent.as_u128() as u64))
            .anchor(gpui::Corner::BottomLeft)
            .open(list.is_some())
            .on_open_change(move |open, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if *open {
                        this.open_delegation_picker(parent, selection, window, cx);
                    } else {
                        this.delegation_picker = None;
                    }
                    cx.notify();
                });
            })
            .trigger(
                style::delegation_recipient_chip(
                    ("delegate-choice", parent.as_u128() as u64),
                    label,
                    cx,
                )
                .on_click(cx.listener(move |this, event, window, cx| {
                    if matches!(event, gpui::ClickEvent::Keyboard(_)) {
                        this.open_delegation_picker(parent, selection, window, cx);
                    }
                })),
            )
            .content(move |_, window, cx| {
                v_flex()
                    .w(px(390.).min(window.viewport_size().width - px(32.)))
                    .when_some(content_list.clone(), |panel, list| {
                        panel.child(
                            List::new(&list)
                                .search_placeholder("Find a bandmate…")
                                .max_h(px(320.)),
                        )
                    })
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .border_t_1()
                            .border_color(design::line_2(cx))
                            .text_size(design::text_label())
                            .text_color(design::t3(cx))
                            .child("↑ ↓ Navigate    Enter Select    Esc Close"),
                    )
            });
        if let Some(list) = list {
            popover = popover.track_focus(&list.focus_handle(cx));
        }
        h_flex()
            .gap_1()
            .min_w(px(0.))
            .child(popover)
            .child(
                style::header_icon_button(
                    ("cancel-delegate", parent.as_u128() as u64),
                    IconName::Close,
                    cx,
                )
                .tooltip("Cancel delegation")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.delegation_selection.remove(&parent);
                    this.delegation_picker = None;
                    if let Some(input) = this.agent_chat_inputs.get(&parent) {
                        input.update(cx, |input, cx| input.focus(window, cx));
                    }
                    cx.notify();
                })),
            )
            .into_any_element()
    }
}
