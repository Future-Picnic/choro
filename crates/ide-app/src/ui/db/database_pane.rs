use gpui::{div, Context, Entity, IntoElement, ParentElement, Render, Styled, Window};

use super::{collection_pane::CollectionPane, table_pane::TablePane};

pub enum DatabasePaneContent {
    Documents(Entity<CollectionPane>),
    Table(Entity<TablePane>),
}

/// Type-erased center tab that lets document and relational backends coexist
/// in the same DB tab collection without discarding their specialized UIs.
pub struct DatabasePane {
    content: DatabasePaneContent,
}

impl DatabasePane {
    pub fn documents(view: Entity<CollectionPane>) -> Self {
        Self {
            content: DatabasePaneContent::Documents(view),
        }
    }

    pub fn table(view: Entity<TablePane>) -> Self {
        Self {
            content: DatabasePaneContent::Table(view),
        }
    }
}

impl Render for DatabasePane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.content {
            DatabasePaneContent::Documents(view) => view.clone().into_any_element(),
            DatabasePaneContent::Table(view) => view.clone().into_any_element(),
        };
        div().size_full().child(content)
    }
}
