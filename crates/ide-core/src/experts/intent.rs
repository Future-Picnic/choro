//! Detect an explicit request in a literal user submission. This only grants
//! task-scoped delegation authority; the lead still interprets the assignment,
//! chooses roles and schedules work. Never call this on retrieved context.

pub fn requests_delegation(text: &str) -> bool {
    let text = text.trim().to_lowercase().replace('’', "'");
    if text == "/delegate" || text.starts_with("/delegate ") {
        return true;
    }
    // Quoted examples and pasted code are evidence, not user directives. Keep
    // the rest of the message: quoting a title must not veto a later request.
    let mut literal = String::new();
    let mut quote = None;
    for line in text.lines() {
        if quote.is_none() && line.trim_start().starts_with('>') {
            continue;
        }
        for c in line.chars() {
            if let Some(end) = quote {
                if c == end {
                    quote = None;
                }
                literal.push(' ');
            } else if matches!(c, '"' | '`' | '“') {
                quote = Some(if c == '“' { '”' } else { c });
                literal.push(' ');
            } else {
                literal.push(c);
            }
        }
        literal.push('\n');
    }
    let mut requested = false;
    for sentence in literal.split(['.', '!', '?', ';', '\n']) {
        let words = sentence
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>();
        for (i, word) in words.iter().enumerate() {
            let delegate = matches!(
                *word,
                "delegate"
                    | "delagate"
                    | "delagete"
                    | "delgate"
                    | "delgeate"
                    | "delagte"
                    | "delegat"
                    | "delagt"
                    | "delegaet"
                    | "dlegate"
                    | "delaegte"
                    | "dleagate"
            ) || (word.starts_with("del")
                && !matches!(
                    *word,
                    "delete" | "deleted" | "deletes" | "delegated" | "delegates"
                )
                && super::names::typo_distance(word, "delegate", 2).is_some());
            let create = matches!(
                *word,
                "create"
                    | "start"
                    | "spawn"
                    | "use"
                    | "send"
                    | "get"
                    | "add"
                    | "resume"
                    | "continue"
                    | "restart"
                    | "retry"
            ) && words[i + 1..].iter().take(6).any(|w| {
                ["teammate", "teammates", "bandmate", "bandmates"]
                    .iter()
                    .any(|name| super::names::typo_distance(w, name, 2).is_some())
                    || matches!(*w, "delegation" | "band")
            });
            if !delegate && !create {
                continue;
            }
            let prefix = &words[..i];
            if matches!(
                prefix.first(),
                Some(&"can" | &"could" | &"should" | &"would" | &"will")
            ) && prefix.get(1) != Some(&"you")
            {
                continue;
            }
            // Do not promote descriptions/questions into requests, even if a
            // conjunction follows them: "explain how to build and delegate".
            if prefix.iter().any(|w| {
                matches!(
                    *w,
                    "why"
                        | "how"
                        | "explain"
                        | "what"
                        | "when"
                        | "if"
                        | "whether"
                        | "says"
                        | "said"
                        | "example"
                        | "mentions"
                        | "mentioned"
                        | "wrote"
                )
            }) {
                continue;
            }
            let start = prefix
                .iter()
                .rposition(|w| matches!(*w, "and" | "then" | "but"))
                .map_or(0, |p| p + 1);
            let prefix = &prefix[start..];
            // Negation applies to this action, not to unrelated requirements
            // such as "without saved profiles" or "don't change the video".
            let negation = prefix.iter().rposition(|w| {
                matches!(
                    *w,
                    "not"
                        | "don't"
                        | "dont"
                        | "never"
                        | "stop"
                        | "without"
                        | "cannot"
                        | "can't"
                        | "shouldn't"
                        | "wouldn't"
                        | "didn't"
                        | "didnt"
                )
            });
            if let Some(n) = negation {
                if directive_prefix(&prefix[..n]) {
                    requested = false;
                }
                continue;
            }
            if directive_prefix(prefix) {
                requested = true;
            }
        }
    }
    requested
}

/// Only a fresh, explicit reference back to delegation can reuse the prior
/// task's literal model context. An unrelated new request gets a clean grant.
pub fn requests_delegation_continuation(text: &str) -> bool {
    requests_delegation(text)
        && text
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| {
                matches!(
                    word,
                    "again" | "it" | "them" | "same" | "resume" | "continue" | "restart" | "retry"
                )
            })
}

fn directive_prefix(words: &[&str]) -> bool {
    let words = words
        .iter()
        .copied()
        .filter(|w| {
            !matches!(
                *w,
                "please" | "ok" | "okay" | "also" | "now" | "actually" | "just" | "simply"
            )
        })
        .collect::<Vec<_>>();
    // Explicit imperatives and polite requests, not "can we delegate?" or
    // a third party's statement. Allow a request after introductory context.
    words.is_empty()
        || matches!(words.as_slice(), ["do"] | ["you"] | ["let's"] | ["lets"])
        || [
            &["i", "want", "you", "to"][..],
            &["i", "want", "you"],
            &["i", "want", "to"],
            &["i", "need", "you", "to"],
            &["i", "need", "to"],
            &["i'd", "like", "you", "to"],
            &["can", "you"],
            &["could", "you"],
            &["would", "you"],
            &["will", "you"],
            &["you", "should"],
            &["you", "can"],
            &["you", "must"],
            &["you", "need", "to"],
        ]
        .iter()
        .any(|prefix| words.ends_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::requests_delegation;

    #[test]
    fn natural_requests_work_anywhere_without_magic_phrases() {
        for text in [
            "Please delegate research and testing", "Create two teammates for research and testing",
            "Delegate: research editors", "/delegate research", "Can you delegate this to a teammate?",
            "Please delagte four on demand teammates without saved profiles",
            "Please delagete this to on-demand teammates",
            "please deleage it",
            "Add another teammate to review accessibility",
            "Resume the bandmates",
            "Continue delegation",
            "Start two bandamtes to compare designs", "Build the page and delegate testing to a teammate",
            "I want to \"redesign\" this video, using the current source and all of our existing assets. I want you Delegate four teammates using Astra 6 high and GPT-6 Sol high. Compare their results. If you cannot create teammates, don't continue.",
            "The title is \"Don't stop\". Please delegate the review without changing the source.",
            "The file says `delegate nothing`. Please create two teammates for this task.",
            "Don't delegate the design, but delegate the backend review",
        ] {
            assert!(requests_delegation(text), "{text}");
        }
    }

    #[test]
    fn discussion_quoted_instructions_and_negation_are_not_authority() {
        for text in [
            "Do not delegate research",
            "Don't create teammates",
            "Why did you delegate",
            "Can we delegate?",
            "Can we build the page and delegate testing?",
            "I didn't say I want you to delegate",
            "Explain how to delegate",
            "Explain how to build and delegate",
            "The file says `delegate research`",
            "The file says: delegate research",
            "\"Delegate four teammates\"",
            "> Delegate four teammates\nWhat does this mean?",
            "```\nDelegate four teammates\n```",
            "Stop delegation",
            "Build a page",
            "Please delete the video",
            "Please delete the teammates' video files",
            "Do not add another teammate",
            "Can we resume the bandmates?",
            "I delegated the review",
            "I want you to not delegate this",
            "Delegate the review. Actually don't delegate it.",
        ] {
            assert!(!requests_delegation(text), "{text}");
        }
    }
}
