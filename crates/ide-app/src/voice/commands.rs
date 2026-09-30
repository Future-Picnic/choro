#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum VoiceCommand {
    CreateProjectPlan(Option<String>),
    StartWriting,
    Write(String),
    WriteAndSend(String),
    SendDraft,
    KeepDraft,
    CancelDraft,
    StopListening,
    Help,
}

fn normalize(utterance: &str) -> String {
    utterance
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character.is_whitespace() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_send_suffix(content: &str) -> Option<&str> {
    let content = content.trim_end_matches(['.', '!', '?']).trim_end();
    let lowered = content.to_ascii_lowercase();
    [" and send it", " and send", " then send it", " then send"]
        .into_iter()
        .find_map(|suffix| {
            lowered
                .strip_suffix(suffix)
                .map(|message| &content[..message.len()])
                .map(str::trim_end)
                .filter(|message| !message.is_empty())
        })
}

fn command_word_ranges(content: &str) -> Vec<(usize, usize, String)> {
    let mut words = Vec::new();
    let mut start = None;
    for (index, character) in content.char_indices() {
        if character.is_alphanumeric() {
            start.get_or_insert(index);
        } else if let Some(start) = start.take() {
            words.push((start, index, content[start..index].to_ascii_lowercase()));
        }
    }
    if let Some(start) = start {
        words.push((start, content.len(), content[start..].to_ascii_lowercase()));
    }
    words
}

fn is_negated_send(content: &str, words: &[(usize, usize, String)], send_index: usize) -> bool {
    let send_start = words[send_index].0;
    let clause_start = content[..send_start]
        .char_indices()
        .rev()
        .find(|(_, character)| matches!(*character, '.' | '!' | '?' | ';' | '\n' | '\r'))
        .map_or(0, |(index, character)| index + character.len_utf8());

    words[..send_index].iter().any(|(start, _, word)| {
        *start >= clause_start && matches!(word.as_str(), "not" | "never" | "no" | "dont" | "t")
    })
}

/// Treat a final spoken "send" as an explicit dictation action while keeping
/// the preceding transcript editable and preserving its original casing.
/// Moonshine may place a final command on a new line, so command matching is
/// word-based rather than dependent on a literal ASCII space.
pub(super) fn split_dictation_send_command(utterance: &str) -> (String, bool) {
    let original = utterance.trim();
    let words = command_word_ranges(original);
    let suffixes: &[&[&str]] = &[
        &["and", "then", "send", "it"],
        &["and", "then", "send"],
        &["then", "send", "it"],
        &["and", "send", "it"],
        &["then", "send"],
        &["and", "send"],
        &["send", "it"],
        &["send"],
    ];

    for suffix in suffixes {
        if words.len() < suffix.len() {
            continue;
        }
        let suffix_start = words.len() - suffix.len();
        if !words[suffix_start..]
            .iter()
            .zip(*suffix)
            .all(|((_, _, word), expected)| word == expected)
        {
            continue;
        }
        let send_index = suffix_start
            + suffix
                .iter()
                .position(|word| *word == "send")
                .expect("send command suffix always contains send");
        if is_negated_send(original, &words, send_index) {
            return (original.to_string(), false);
        }

        let command_start = words[suffix_start].0;
        let message = original[..command_start]
            .trim_end()
            .trim_end_matches([',', ';', ':', '-', '—', '“', '‘'])
            .trim_end();
        return (message.to_string(), true);
    }

    (original.to_string(), false)
}

pub(super) fn parse_voice_command(utterance: &str) -> Option<VoiceCommand> {
    let trimmed = utterance.trim();
    let normalized = normalize(utterance);

    if normalized == "please write" {
        return Some(VoiceCommand::StartWriting);
    }

    // Keep the dictated message exactly as recognized. Normalization is useful
    // for command matching, but applying it to the payload would destroy case,
    // punctuation, and non-English text embedded in the message.
    let lowered = trimmed.to_ascii_lowercase();
    if let Some(remainder) = lowered
        .strip_prefix("please write")
        .and_then(|_| trimmed.get("please write".len()..))
        .filter(|remainder| {
            remainder.chars().next().is_some_and(|character| {
                character.is_whitespace() || matches!(character, ',' | ':' | '-' | '—')
            })
        })
    {
        let content = remainder
            .trim_start_matches(|character: char| {
                character.is_whitespace() || matches!(character, ',' | ':' | '-' | '—')
            })
            .trim();
        if !content.is_empty() {
            if let Some(message) = split_send_suffix(content) {
                return Some(VoiceCommand::WriteAndSend(message.to_string()));
            }
            return Some(VoiceCommand::Write(content.to_string()));
        }
    }

    match normalized.as_str() {
        "start writing" | "start write mode" | "write mode" => Some(VoiceCommand::StartWriting),
        "send" | "send it" | "yes send it" | "go ahead send it" | "go ahead and send it" => {
            Some(VoiceCommand::SendDraft)
        }
        "cancel" | "cancel draft" | "discard draft" | "don t send" | "do not send" => {
            Some(VoiceCommand::CancelDraft)
        }
        "stop listening" | "end voice" | "end voice session" | "stop voice" | "goodbye choro" => {
            Some(VoiceCommand::StopListening)
        }
        "voice help" | "help with voice" | "what can i say" | "show voice commands" => {
            Some(VoiceCommand::Help)
        }
        _ => None,
    }
}

/// Project Talk deliberately has a much smaller command vocabulary than the
/// legacy voice director. Everything else is discussion, so phrases such as
/// “write a cache” cannot unexpectedly modify an open chat.
pub(super) fn parse_project_conversation_command(utterance: &str) -> Option<VoiceCommand> {
    let normalized = normalize(utterance);
    match normalized.as_str() {
        "stop listening" | "end voice" | "end voice session" | "stop voice" | "goodbye choro" => {
            return Some(VoiceCommand::StopListening);
        }
        "voice help" | "help with voice" | "what can i say" | "show voice commands" => {
            return Some(VoiceCommand::Help);
        }
        "create a plan"
        | "create a plan for this"
        | "create a plan for that"
        | "please create a plan"
        | "please create a plan for this"
        | "please create a plan for that"
        | "make a plan"
        | "make a plan for this"
        | "make a plan for that"
        | "turn this into a plan"
        | "turn that into a plan" => return Some(VoiceCommand::CreateProjectPlan(None)),
        _ => {}
    }

    let trimmed = utterance.trim();
    let lowered = trimmed.to_ascii_lowercase();
    for prefix in [
        "please create a plan for",
        "create a plan for",
        "make a plan for",
    ] {
        let Some(remainder) = lowered
            .strip_prefix(prefix)
            .and_then(|_| trimmed.get(prefix.len()..))
        else {
            continue;
        };
        let topic = remainder
            .trim_start_matches(|character: char| {
                character.is_whitespace() || matches!(character, ',' | ':' | '-' | '—')
            })
            .trim()
            .trim_end_matches(['.', '!', '?'])
            .trim();
        if topic.is_empty() || matches!(normalize(topic).as_str(), "this" | "that" | "it") {
            return Some(VoiceCommand::CreateProjectPlan(None));
        }
        return Some(VoiceCommand::CreateProjectPlan(Some(topic.to_string())));
    }

    None
}

/// Confirmation mode intentionally recognizes only the tiny answer set needed
/// to resolve a pending draft. Ordinary speech must never become a second
/// dictated message or a Project Talk question while this mode is active.
pub(super) fn parse_draft_confirmation(utterance: &str) -> Option<VoiceCommand> {
    match normalize(utterance).as_str() {
        "yes"
        | "yes please"
        | "yeah"
        | "yeah send it"
        | "yep"
        | "sure"
        | "ok"
        | "okay"
        | "ok send it"
        | "okay send it"
        | "send"
        | "send it"
        | "yes send"
        | "yes send it"
        | "go ahead"
        | "go ahead send it"
        | "go ahead and send it" => Some(VoiceCommand::SendDraft),
        "no" | "nope" | "no thanks" | "no keep it" | "not now" | "don t send" | "don t send it"
        | "do not send" | "do not send it" | "keep it" | "leave it" => {
            Some(VoiceCommand::KeepDraft)
        }
        "cancel" | "cancel it" | "cancel draft" | "discard draft" | "delete draft" => {
            Some(VoiceCommand::CancelDraft)
        }
        "stop listening" | "end voice" | "end voice session" | "stop voice" | "goodbye choro" => {
            Some(VoiceCommand::StopListening)
        }
        _ => None,
    }
}

pub(super) fn command_label(command: &VoiceCommand) -> &'static str {
    match command {
        VoiceCommand::CreateProjectPlan(_) => "Create plan",
        VoiceCommand::StartWriting | VoiceCommand::Write(_) | VoiceCommand::WriteAndSend(_) => {
            "Please write"
        }
        VoiceCommand::SendDraft => "Send",
        VoiceCommand::KeepDraft => "Keep draft",
        VoiceCommand::CancelDraft => "Cancel draft",
        VoiceCommand::StopListening => "Stop listening",
        VoiceCommand::Help => "Voice help",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_commands_without_treating_normal_speech_as_commands() {
        assert_eq!(
            parse_voice_command("Please write."),
            Some(VoiceCommand::StartWriting)
        );
        assert_eq!(
            parse_voice_command("Yes, send it!"),
            Some(VoiceCommand::SendDraft)
        );
        assert_eq!(
            parse_voice_command("stop listening"),
            Some(VoiceCommand::StopListening)
        );
        assert_eq!(
            parse_voice_command("Please write a summary of this discussion"),
            Some(VoiceCommand::Write(
                "a summary of this discussion".to_string()
            ))
        );
        assert_eq!(
            parse_voice_command("Please write: Fix OAuth, then add 日本語 tests."),
            Some(VoiceCommand::Write(
                "Fix OAuth, then add 日本語 tests.".to_string()
            ))
        );
        assert_eq!(
            parse_voice_command("Please write fix the failing test and send it"),
            Some(VoiceCommand::WriteAndSend(
                "fix the failing test".to_string()
            ))
        );
        assert_eq!(parse_voice_command("please writer"), None);
        assert_eq!(parse_voice_command("How should we fix this?"), None);
    }

    #[test]
    fn confirmation_mode_accepts_only_draft_decisions() {
        assert_eq!(
            parse_draft_confirmation("yes"),
            Some(VoiceCommand::SendDraft)
        );
        assert_eq!(
            parse_draft_confirmation("No, keep it"),
            Some(VoiceCommand::KeepDraft)
        );
        assert_eq!(
            parse_draft_confirmation("cancel draft"),
            Some(VoiceCommand::CancelDraft)
        );
        assert_eq!(parse_draft_confirmation("and another thing"), None);
        assert_eq!(
            parse_draft_confirmation("please write something else"),
            None
        );
    }

    #[test]
    fn trailing_send_is_removed_from_direct_dictation() {
        assert_eq!(
            split_dictation_send_command("Fix the failing test, send."),
            ("Fix the failing test".to_string(), true)
        );
        assert_eq!(
            split_dictation_send_command("Run the focused tests and then send it"),
            ("Run the focused tests".to_string(), true)
        );
        assert_eq!(split_dictation_send_command("send"), (String::new(), true));
        assert_eq!(split_dictation_send_command("Send."), (String::new(), true));
        assert_eq!(
            split_dictation_send_command("Are you okay?\nSend."),
            ("Are you okay?".to_string(), true)
        );
        assert_eq!(
            split_dictation_send_command("Run the tests.\n“Send it!”"),
            ("Run the tests.".to_string(), true)
        );
        assert_eq!(
            split_dictation_send_command("Do not send."),
            ("Do not send.".to_string(), false)
        );
        assert_eq!(
            split_dictation_send_command("I don't want you to send."),
            ("I don't want you to send.".to_string(), false)
        );
        assert_eq!(
            split_dictation_send_command("Never ever send it."),
            ("Never ever send it.".to_string(), false)
        );
        assert_eq!(
            split_dictation_send_command("Do not change the tests. Then send."),
            ("Do not change the tests.".to_string(), true)
        );
        assert_eq!(
            split_dictation_send_command("Explain the failing test"),
            ("Explain the failing test".to_string(), false)
        );
    }

    #[test]
    fn project_talk_has_only_safe_session_commands() {
        assert_eq!(
            parse_project_conversation_command("Please create a plan for that."),
            Some(VoiceCommand::CreateProjectPlan(None))
        );
        assert_eq!(
            parse_project_conversation_command("Create a plan for the voice architecture."),
            Some(VoiceCommand::CreateProjectPlan(Some(
                "the voice architecture".to_string()
            )))
        );
        assert_eq!(
            parse_project_conversation_command("stop listening"),
            Some(VoiceCommand::StopListening)
        );
        assert_eq!(
            parse_project_conversation_command("Please write a cache layer"),
            None
        );
        assert_eq!(parse_project_conversation_command("Send it"), None);
    }
}
