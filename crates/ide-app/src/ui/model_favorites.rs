use gpui::{
    div, px, App, AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Window,
};
use gpui_component::{h_flex, menu::PopupMenuItem};
use ide_core::model_favorites::ModelFavorite;

use crate::state::Workspace;

pub(crate) fn grouped_choices<T>(
    mut rows: Vec<T>,
    favorites: &[ModelFavorite],
    key: impl Fn(&T) -> ModelFavorite,
) -> Vec<(Option<&'static str>, T, ModelFavorite)> {
    ide_core::model_favorites::favorites_first(&mut rows, favorites, &key);
    let has_favorites = rows.iter().any(|row| favorites.contains(&key(row)));
    let mut previous = None;
    rows.into_iter()
        .map(|row| {
            let key = key(&row);
            let favorite = favorites.contains(&key);
            let heading = (has_favorites && previous != Some(favorite)).then_some(if favorite {
                "Favorites"
            } else {
                "All models"
            });
            previous = Some(favorite);
            (heading, row, key)
        })
        .collect()
}

pub(crate) struct ModelFavoriteToggle {
    workspace: Entity<Workspace>,
    model: ModelFavorite,
}

pub(crate) fn favorite_toggle(
    workspace: Entity<Workspace>,
    model: ModelFavorite,
    cx: &mut App,
) -> Entity<ModelFavoriteToggle> {
    cx.new(|cx| {
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        ModelFavoriteToggle { workspace, model }
    })
}

impl Render for ModelFavoriteToggle {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let favorite = self
            .workspace
            .read(cx)
            .favorite_models
            .contains(&self.model);
        super::style::model_favorite_button("model-favorite", favorite, cx).on_click(cx.listener(
            |this, _, _, cx| {
                // A star must not select its containing model or dismiss a menu.
                cx.stop_propagation();
                this.workspace.update(cx, |workspace, cx| {
                    workspace.toggle_model_favorite(this.model.clone(), cx)
                });
            },
        ))
    }
}

/// Preserve the native dropdown's keyboard selection and check mark while
/// adding an independently live star that keeps the menu open when toggled.
pub(crate) fn model_menu_item(
    label: impl Into<SharedString>,
    model: ModelFavorite,
    checked: bool,
    workspace: Entity<Workspace>,
    cx: &mut App,
) -> PopupMenuItem {
    let label = label.into();
    let toggle = favorite_toggle(workspace, model, cx);
    PopupMenuItem::element(move |_, _| {
        h_flex()
            .w_full()
            .min_w(px(0.))
            .gap_2()
            .child(div().flex_1().min_w(px(0.)).truncate().child(label.clone()))
            .child(toggle.clone())
    })
    .checked(checked)
}
