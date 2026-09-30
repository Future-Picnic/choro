//! Redaction for text that crosses a diagnostic, log, or persisted work-log boundary.
//!
//! This is deliberately a last line of defense. Callers should still avoid
//! including credentials in errors in the first place.

use std::sync::OnceLock;

use regex::Regex;

const REDACTED: &str = "[REDACTED]";

/// Removes common credential shapes and values from secret-like environment
/// variables before text is logged or persisted as diagnostic output.
pub fn redact_sensitive_text(input: &str) -> String {
    redact_sensitive_text_with(input, std::iter::empty::<&str>())
}

/// Redacts common credential shapes plus exact secret values known by the
/// caller. Empty values and `${ENV_VAR}` references are intentionally ignored.
pub fn redact_sensitive_text_with<'a>(
    input: &str,
    known_secrets: impl IntoIterator<Item = &'a str>,
) -> String {
    let mut output = redact_patterns(input);

    for secret in known_secrets {
        redact_exact_secret(&mut output, secret);
    }
    for secret in secret_environment_values() {
        redact_exact_secret(&mut output, &secret);
    }

    output
}

fn redact_exact_secret(output: &mut String, secret: &str) {
    let secret = secret.trim();
    if secret.len() >= 6 && !(secret.starts_with("${") && secret.ends_with('}')) {
        *output = output.replace(secret, REDACTED);
    }
}

fn redact_patterns(input: &str) -> String {
    static URL_CREDENTIALS: OnceLock<Regex> = OnceLock::new();
    static AUTH_HEADER: OnceLock<Regex> = OnceLock::new();
    static SECRET_FIELD: OnceLock<Regex> = OnceLock::new();
    static SECRET_FLAG: OnceLock<Regex> = OnceLock::new();

    let output = URL_CREDENTIALS
        .get_or_init(|| {
            Regex::new(
                r"(?i)\b((?:mongodb(?:\+srv)?|postgres(?:ql)?|mysql|redis|https?)://)([^/\s:@]+):([^@\s/]+)@",
            )
            .expect("valid credential URL regex")
        })
        .replace_all(input, "${1}${2}:[REDACTED]@")
        .into_owned();

    let output = AUTH_HEADER
        .get_or_init(|| {
            Regex::new(r#"(?i)(authorization\s*[:=]\s*["']?(?:bearer|basic)?\s*)[^\s,"';}]+"#)
                .expect("valid authorization regex")
        })
        .replace_all(&output, "${1}[REDACTED]")
        .into_owned();

    let output = SECRET_FIELD
        .get_or_init(|| {
            Regex::new(
                r#"(?i)(["']?(?:api[_-]?key|api[_-]?token|access[_-]?token|auth[_-]?token|token|password|passwd|secret|client[_-]?secret|private[_-]?key)["']?\s*[:=]\s*["']?)[^\s,"';}]+"#,
            )
            .expect("valid secret field regex")
        })
        .replace_all(&output, "${1}[REDACTED]")
        .into_owned();

    SECRET_FLAG
        .get_or_init(|| {
            Regex::new(
                r"(?i)(--(?:api[-_]?key|api[-_]?token|access[-_]?token|auth[-_]?token|token|password|secret)(?:=|\s+))\S+",
            )
            .expect("valid secret flag regex")
        })
        .replace_all(&output, "${1}[REDACTED]")
        .into_owned()
}

fn secret_environment_values() -> Vec<String> {
    std::env::vars()
        .filter(|(name, value)| is_secret_environment_name(name) && value.trim().len() >= 6)
        .map(|(_, value)| value)
        .collect()
}

fn is_secret_environment_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "API_KEY",
        "PRIVATE_KEY",
        "AUTHORIZATION",
    ]
    .iter()
    .any(|marker| name.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials_in_connection_urls() {
        let text = concat!(
            "mongodb+srv://liran:very-secret@cluster.example/app\n",
            "postgresql://postgres:supabase-password@pooler.supabase.com/postgres\n",
            "mysql://root:mysql-password@localhost/app"
        );
        let redacted = redact_sensitive_text(text);
        for secret in ["very-secret", "supabase-password", "mysql-password"] {
            assert!(!redacted.contains(secret));
        }
        assert_eq!(redacted.matches("[REDACTED]").count(), 3);
    }

    #[test]
    fn redacts_headers_fields_and_command_flags() {
        let text = concat!(
            "Authorization: Bearer abcdef123456\n",
            "api_token=tok_abcdefgh\n",
            "{\"password\":\"hunter-two\"}\n",
            "tool --api-key key_123456"
        );
        let redacted = redact_sensitive_text(text);
        for secret in ["abcdef123456", "tok_abcdefgh", "hunter-two", "key_123456"] {
            assert!(!redacted.contains(secret), "secret survived: {redacted}");
        }
    }

    #[test]
    fn redacts_exact_known_values_without_redacting_env_references() {
        let redacted = redact_sensitive_text_with(
            "remote echoed literal-token-123 but ${SAFE_TOKEN} is configuration",
            ["literal-token-123", "${SAFE_TOKEN}"],
        );
        assert_eq!(
            redacted,
            "remote echoed [REDACTED] but ${SAFE_TOKEN} is configuration"
        );
    }

    #[test]
    fn leaves_normal_diagnostics_readable() {
        assert_eq!(
            redact_sensitive_text("request timed out while loading board KAN"),
            "request timed out while loading board KAN"
        );
    }
}
