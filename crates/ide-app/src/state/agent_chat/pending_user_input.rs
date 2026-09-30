#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingUserInput {
    pub request_id: String,
    pub questions: Vec<PendingUserInputQuestion>,
    pub answers: Vec<Option<PendingUserInputAnswer>>,
    pub question_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingUserInputQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<PendingUserInputOption>,
    pub multi_select: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingUserInputOption {
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingUserInputAnswer {
    pub selected_option_labels: Vec<String>,
    pub custom_answer: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingUserInputProgress {
    pub question_index: usize,
    pub answered_count: usize,
    pub total_count: usize,
    pub is_last_question: bool,
    pub is_complete: bool,
    pub can_advance: bool,
}

impl PendingUserInput {
    pub fn new(request_id: impl Into<String>, questions: Vec<PendingUserInputQuestion>) -> Self {
        let answers = vec![None; questions.len()];
        Self {
            request_id: request_id.into(),
            questions,
            answers,
            question_index: 0,
        }
    }

    pub fn active_question(&self) -> Option<&PendingUserInputQuestion> {
        self.questions.get(self.question_index)
    }

    pub fn active_answer(&self) -> Option<&PendingUserInputAnswer> {
        self.answers
            .get(self.question_index)
            .and_then(Option::as_ref)
    }

    pub fn progress(&self) -> PendingUserInputProgress {
        let total_count = self.questions.len();
        let question_index = self.question_index.min(total_count.saturating_sub(1));
        let answered_count = self
            .answers
            .iter()
            .filter(|answer| answer.is_some())
            .count();
        let is_last_question = question_index + 1 >= total_count;
        let is_complete = total_count > 0 && answered_count == total_count;
        let can_advance = self
            .answers
            .get(question_index)
            .is_some_and(|answer| answer.is_some());
        PendingUserInputProgress {
            question_index,
            answered_count,
            total_count,
            is_last_question,
            is_complete,
            can_advance,
        }
    }

    pub fn previous(&mut self) {
        self.question_index = self.question_index.saturating_sub(1);
    }

    pub fn next(&mut self) {
        if self.question_index + 1 < self.questions.len() {
            self.question_index += 1;
        }
    }

    pub fn select_option(&mut self, option_label: &str) {
        let Some(question) = self.active_question() else {
            return;
        };
        let question = question.clone();
        let answer = self
            .answers
            .get_mut(self.question_index)
            .and_then(Option::as_mut);
        if question.multi_select {
            let mut selected = answer
                .map(|answer| answer.selected_option_labels.clone())
                .unwrap_or_default();
            if let Some(index) = selected.iter().position(|label| label == option_label) {
                selected.remove(index);
            } else {
                selected.push(option_label.to_string());
            }
            self.answers[self.question_index] = if selected.is_empty() {
                None
            } else {
                Some(PendingUserInputAnswer {
                    selected_option_labels: selected,
                    custom_answer: None,
                })
            };
        } else {
            self.answers[self.question_index] = Some(PendingUserInputAnswer {
                selected_option_labels: vec![option_label.to_string()],
                custom_answer: None,
            });
        }
    }

    pub fn set_custom_answer(&mut self, answer: impl Into<String>) {
        let answer = answer.into();
        self.answers[self.question_index] = if answer.trim().is_empty() {
            None
        } else {
            Some(PendingUserInputAnswer {
                selected_option_labels: Vec::new(),
                custom_answer: Some(answer.trim().to_string()),
            })
        };
    }

    pub fn build_answers(&self) -> Option<Vec<String>> {
        if !self.progress().is_complete {
            return None;
        }
        self.answers
            .iter()
            .map(|answer| resolve_answer(answer.as_ref()?))
            .collect()
    }
}

impl PendingUserInputQuestion {
    pub fn pick_one(
        id: impl Into<String>,
        header: impl Into<String>,
        question: impl Into<String>,
        options: Vec<PendingUserInputOption>,
    ) -> Self {
        Self {
            id: id.into(),
            header: header.into(),
            question: question.into(),
            options,
            multi_select: false,
        }
    }

    pub fn pick_many(
        id: impl Into<String>,
        header: impl Into<String>,
        question: impl Into<String>,
        options: Vec<PendingUserInputOption>,
    ) -> Self {
        Self {
            id: id.into(),
            header: header.into(),
            question: question.into(),
            options,
            multi_select: true,
        }
    }
}

impl PendingUserInputOption {
    pub fn new(label: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
        }
    }
}

fn resolve_answer(answer: &PendingUserInputAnswer) -> Option<String> {
    if let Some(custom) = answer
        .custom_answer
        .as_deref()
        .map(str::trim)
        .filter(|custom| !custom.is_empty())
    {
        return Some(custom.to_string());
    }
    if answer.selected_option_labels.is_empty() {
        None
    } else {
        Some(answer.selected_option_labels.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_select_answers_and_advances() {
        let mut pending = PendingUserInput::new(
            "r1",
            vec![PendingUserInputQuestion::pick_one(
                "q1",
                "Pick one",
                "Which page?",
                vec![PendingUserInputOption::new("Menu", "Cafe menu")],
            )],
        );

        assert!(!pending.progress().can_advance);
        pending.select_option("Menu");
        assert_eq!(pending.build_answers(), Some(vec!["Menu".to_string()]));
    }

    #[test]
    fn custom_answer_wins() {
        let mut pending = PendingUserInput::new(
            "r1",
            vec![PendingUserInputQuestion::pick_one(
                "q1",
                "Pick one",
                "Which page?",
                vec![PendingUserInputOption::new("Menu", "Cafe menu")],
            )],
        );

        pending.select_option("Menu");
        pending.set_custom_answer("Wholesale");
        assert_eq!(pending.build_answers(), Some(vec!["Wholesale".to_string()]));
    }
}
