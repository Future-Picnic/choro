use std::{fs, path::PathBuf, time::Duration};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, IntoElement, ParentElement,
    PathPromptOptions, Render, Styled, Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputState},
    v_flex, Disableable, Icon, IconName, WindowExt,
};
use serde::{Deserialize, Serialize};

use crate::ui::style;

const MAX_ATTACHMENT_BYTES: u64 = 5 * 1024 * 1024;

pub struct FeedbackModal {
    name: Entity<InputState>,
    issue: Entity<InputState>,
    attachment: Option<PathBuf>,
    error: Option<String>,
    sending: bool,
    submitted: bool,
}

impl FeedbackModal {
    pub fn open(window: &mut Window, cx: &mut App) {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Your name"));
        let issue = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(5)
                .placeholder("What happened? What were you trying to do?")
        });
        let modal = cx.new(|cx| {
            cx.observe(&name, |_, _, cx| cx.notify()).detach();
            cx.observe(&issue, |_, _, cx| cx.notify()).detach();
            Self {
                name: name.clone(),
                issue,
                attachment: None,
                error: None,
                sending: false,
                submitted: false,
            }
        });

        let content = modal.clone();
        let footer_modal = modal.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let footer_modal = footer_modal.clone();
            dialog
                .w(px(520.))
                .title("Send feedback")
                .overlay_closable(false)
                .child(content.clone())
                .footer(move |_, _, _, cx| {
                    if footer_modal.read(cx).submitted {
                        return vec![style::primary_button_compact("feedback-done", "Done", cx)
                            .on_click(|_, window, cx| window.close_dialog(cx))];
                    }

                    let sending = footer_modal.read(cx).sending;
                    let submit_modal = footer_modal.clone();
                    vec![
                        style::dialog_neutral_button("feedback-cancel", "Cancel", cx)
                            .disabled(sending)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        style::primary_button_compact(
                            "feedback-send",
                            if sending { "Sending…" } else { "Send" },
                            cx,
                        )
                        .disabled(sending)
                        .on_click(move |_, _, cx| {
                            submit_modal.update(cx, |modal, cx| modal.submit(cx));
                        }),
                    ]
                })
        });

        name.update(cx, |input, cx| input.focus(window, cx));
    }

    fn choose_attachment(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Attach a file".into()),
        });
        cx.spawn(async move |this, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else {
                return;
            };
            this.update(cx, |this, cx| {
                this.attachment = Some(path);
                this.error = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.sending {
            return;
        }

        let name = self.name.read(cx).value().to_string();
        let issue = self.issue.read(cx).value().to_string();
        if !feedback_form_is_valid(&name, &issue) {
            self.error = Some("Add your name and describe the issue before sending.".into());
            cx.notify();
            return;
        }

        if let Some(path) = &self.attachment {
            match fs::metadata(path) {
                Ok(metadata) if metadata.len() > MAX_ATTACHMENT_BYTES => {
                    self.error = Some("Choose an attachment smaller than 5 MB.".into());
                    cx.notify();
                    return;
                }
                Err(_) => {
                    self.error = Some("The attachment is no longer available.".into());
                    cx.notify();
                    return;
                }
                _ => {}
            }
        }

        let attachment = self.attachment.clone();
        self.error = None;
        self.sending = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { send_feedback(name, issue, attachment) })
                .await;
            this.update(cx, |this, cx| {
                this.sending = false;
                match result {
                    Ok(()) => {
                        this.error = None;
                        this.submitted = true;
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for FeedbackModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let attachment_name = self.attachment.as_ref().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });

        v_flex()
            .w_full()
            .min_h(px(250.))
            .when(self.submitted, |content| {
                content.items_center().justify_center().gap_3().child(
                    v_flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .size(px(44.))
                                .rounded_full()
                                .bg(crate::ui::design::sage(cx).opacity(0.14))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::new(IconName::CircleCheck)
                                        .size(crate::ui::design::icon_xl())
                                        .text_color(crate::ui::design::sage(cx)),
                                ),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_head())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::t1(cx))
                                .child("Thank you"),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("Your feedback helps us improve Choro."),
                        ),
                )
            })
            .when(!self.submitted, |content| {
                content.gap_4().child(
                    v_flex()
                        .gap_4()
                        .child(form_field("Name", Input::new(&self.name), cx))
                        .child(form_field("Issue", Input::new(&self.issue).h(px(132.)), cx))
                        .child(
                            v_flex()
                                .gap_1p5()
                                .child(field_label("Attachment", cx))
                                .child(
                                    h_flex()
                                        .w_full()
                                        .min_h(px(40.))
                                        .px_2()
                                        .gap_2()
                                        .items_center()
                                        .rounded(crate::ui::design::r_sm())
                                        .border_1()
                                        .border_color(crate::ui::design::line_2(cx))
                                        .bg(crate::ui::design::base(cx).opacity(0.45))
                                        .child(
                                            Icon::new(IconName::File)
                                                .size(crate::ui::design::icon())
                                                .text_color(crate::ui::design::t3(cx)),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .truncate()
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(if attachment_name.is_some() {
                                                    crate::ui::design::t2(cx)
                                                } else {
                                                    crate::ui::design::t3(cx)
                                                })
                                                .child(
                                                    attachment_name.clone().unwrap_or_else(|| {
                                                        "No file attached".into()
                                                    }),
                                                ),
                                        )
                                        .child(
                                            style::dialog_neutral_button(
                                                "feedback-choose-attachment",
                                                if attachment_name.is_some() {
                                                    "Replace"
                                                } else {
                                                    "Choose file"
                                                },
                                                cx,
                                            )
                                            .disabled(self.sending)
                                            .on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    this.choose_attachment(cx);
                                                }),
                                            ),
                                        )
                                        .when(self.attachment.is_some(), |row| {
                                            row.child(
                                                style::dialog_neutral_button(
                                                    "feedback-remove-attachment",
                                                    "Remove",
                                                    cx,
                                                )
                                                .disabled(self.sending)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.attachment = None;
                                                    this.error = None;
                                                    cx.notify();
                                                })),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("Optional. Up to 5 MB. Uploaded only when you press Send."),
                                ),
                        )
                        .when_some(self.error.clone(), |form, error| {
                            form.child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::rose(cx))
                                    .child(error),
                            )
                        }),
                )
            })
    }
}

fn field_label(label: &'static str, cx: &App) -> impl IntoElement {
    div()
        .text_size(crate::ui::design::text_ui())
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(crate::ui::design::t2(cx))
        .child(label)
}

fn form_field(label: &'static str, input: impl IntoElement, cx: &App) -> impl IntoElement {
    v_flex()
        .gap_1p5()
        .child(field_label(label, cx))
        .child(input)
}

fn feedback_form_is_valid(name: &str, issue: &str) -> bool {
    !name.trim().is_empty() && !issue.trim().is_empty()
}

#[derive(Serialize)]
struct FeedbackRequest {
    schema_version: u8,
    name: String,
    issue: String,
    app_version: &'static str,
    platform: String,
    submitted_at: String,
    attachment: Option<FeedbackAttachment>,
}

#[derive(Serialize)]
struct FeedbackAttachment {
    name: String,
    content_type: &'static str,
    data_base64: String,
}

#[derive(Deserialize)]
struct FeedbackResponse {
    ok: bool,
    error: Option<String>,
}

fn send_feedback(name: String, issue: String, attachment: Option<PathBuf>) -> Result<(), String> {
    let endpoint = feedback_endpoint().ok_or_else(|| {
        "Feedback is not configured in this build yet. Please try again after the next update."
            .to_string()
    })?;
    let attachment = attachment
        .map(|path| {
            let bytes = fs::read(&path)
                .map_err(|_| "The attachment could not be read. Choose it again.".to_string())?;
            if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
                return Err("Choose an attachment smaller than 5 MB.".to_string());
            }
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "attachment".into());
            Ok(FeedbackAttachment {
                name,
                content_type: "application/octet-stream",
                data_base64: BASE64.encode(bytes),
            })
        })
        .transpose()?;
    let body = serde_json::to_vec(&FeedbackRequest {
        schema_version: 1,
        name: name.trim().to_string(),
        issue: issue.trim().to_string(),
        app_version: env!("CARGO_PKG_VERSION"),
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        submitted_at: chrono::Utc::now().to_rfc3339(),
        attachment,
    })
    .map_err(|_| "Choro could not prepare the feedback.".to_string())?;

    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .and_then(|client| {
            client
                .post(endpoint)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body)
                .send()
        })
        .map_err(|_| "Could not send feedback. Check your connection and try again.".to_string())?;
    if !response.status().is_success() {
        return Err("Could not send feedback. Please try again in a moment.".into());
    }
    let response_text = response
        .text()
        .map_err(|_| "The feedback service returned an unexpected response.".to_string())?;
    let response: FeedbackResponse = serde_json::from_str(&response_text)
        .map_err(|_| "The feedback service returned an unexpected response.".to_string())?;
    if response.ok {
        Ok(())
    } else {
        Err(response
            .error
            .unwrap_or_else(|| "Could not save feedback. Please try again.".into()))
    }
}

fn feedback_endpoint() -> Option<String> {
    const DEFAULT_FEEDBACK_ENDPOINT: &str = "https://script.google.com/macros/s/AKfycbyUKfwGX2aoiFJejVSPIPSwUbhk77d6dpEO860CG-X26Tm66naw6_csLK_Ej0X-aCl5nQ/exec";

    std::env::var("CHORO_FEEDBACK_ENDPOINT")
        .ok()
        .filter(|endpoint| !endpoint.trim().is_empty())
        .or_else(|| {
            option_env!("CHORO_FEEDBACK_ENDPOINT")
                .filter(|endpoint| !endpoint.trim().is_empty())
                .map(str::to_string)
        })
        .or_else(|| Some(DEFAULT_FEEDBACK_ENDPOINT.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{feedback_form_is_valid, MAX_ATTACHMENT_BYTES};

    #[test]
    fn feedback_requires_a_name_and_issue() {
        assert!(feedback_form_is_valid("Ada", "The preview did not open"));
        assert!(!feedback_form_is_valid("", "The preview did not open"));
        assert!(!feedback_form_is_valid("Ada", "   "));
    }

    #[test]
    fn attachment_limit_is_five_megabytes() {
        assert_eq!(MAX_ATTACHMENT_BYTES, 5 * 1024 * 1024);
    }
}
