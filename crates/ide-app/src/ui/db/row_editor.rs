//! JSON editor for one relational row, matched back by its primary key.

use gpui::{
    div, prelude::FluentBuilder as _, px, AppContext as _, Context, Entity, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Window,
};
use gpui_component::{
    input::{Input, InputEvent, InputState},
    v_flex,
};
use serde_json::Value;

pub(super) struct RowEditor {
    pub(super) input: Entity<InputState>,
    pub(super) parse_error: Option<SharedString>,
    pub(super) prod_save_armed: bool,
}

impl RowEditor {
    pub(super) fn new(json: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pretty = serde_json::from_str::<Value>(&json)
            .ok()
            .and_then(|value| serde_json::to_string_pretty(&value).ok())
            .unwrap_or(json);
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .default_value(pretty.clone())
        });
        cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let value = this.input.read(cx).value().to_string();
                this.parse_error = validate_row_json(&value).err().map(Into::into);
                this.prod_save_armed = false;
                cx.notify();
            }
        })
        .detach();
        Self {
            input,
            parse_error: validate_row_json(&pretty).err().map(Into::into),
            prod_save_armed: false,
        }
    }
}

impl Render for RowEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w(px(720.))
            .h(px(520.))
            .gap_2()
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .child(
                        "The row is matched by its primary key. Changed columns are written back.",
                    ),
            )
            .when_some(self.parse_error.clone(), |view, error| {
                view.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .when(self.prod_save_armed, |view| {
                view.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child("Click Confirm Save to PROD to write this row."),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .child(Input::new(&self.input).h_full()),
            )
    }
}

fn validate_row_json(json: &str) -> Result<(), String> {
    match serde_json::from_str::<Value>(json) {
        Ok(Value::Object(_)) => Ok(()),
        Ok(_) => Err("Row must be a JSON object".into()),
        Err(error) => Err(format!("Invalid JSON: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_json_must_be_an_object() {
        assert!(validate_row_json(r#"{"id":1}"#).is_ok());
        assert!(validate_row_json("[1]").is_err());
        assert!(validate_row_json("{").is_err());
    }
}
