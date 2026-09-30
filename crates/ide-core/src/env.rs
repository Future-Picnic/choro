use anyhow::{anyhow, bail, Context, Result};

/// Expand `${NAME}` placeholders, including placeholders inside environment
/// values. Missing variables, malformed placeholders, and reference cycles are
/// configuration errors rather than empty strings or unbounded replacement.
pub fn expand_env_vars(value: &str) -> Result<String> {
    expand_value(value, &mut Vec::new())
}

fn expand_value(value: &str, stack: &mut Vec<String>) -> Result<String> {
    let mut expanded = String::with_capacity(value.len());
    let mut remainder = value;

    while let Some(start) = remainder.find("${") {
        expanded.push_str(&remainder[..start]);
        let placeholder = &remainder[start + 2..];
        let end = placeholder
            .find('}')
            .ok_or_else(|| anyhow!("Unclosed environment variable placeholder in configuration"))?;
        let name = &placeholder[..end];
        if name.is_empty() {
            bail!("Environment variable placeholder cannot be empty");
        }
        if let Some(cycle_start) = stack.iter().position(|entry| entry == name) {
            let mut cycle = stack[cycle_start..].to_vec();
            cycle.push(name.to_string());
            bail!(
                "Environment variable reference cycle detected: {}",
                cycle.join(" → ")
            );
        }

        let replacement = std::env::var(name)
            .with_context(|| format!("Environment variable {name} is not set"))?;
        stack.push(name.to_string());
        let replacement = expand_value(&replacement, stack)?;
        stack.pop();
        expanded.push_str(&replacement);
        remainder = &placeholder[end + 1..];
    }

    expanded.push_str(remainder);
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEnv(Vec<String>);

    impl TestEnv {
        fn new() -> Self {
            Self(Vec::new())
        }

        fn set(&mut self, name: String, value: String) {
            std::env::set_var(&name, value);
            self.0.push(name);
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            for name in &self.0 {
                std::env::remove_var(name);
            }
        }
    }

    fn unique_name(label: &str) -> String {
        format!("CHORO_{label}_{}", uuid::Uuid::new_v4().simple())
    }

    #[test]
    fn expands_multiple_and_nested_variables() {
        let mut env = TestEnv::new();
        let host = unique_name("HOST");
        let port = unique_name("PORT");
        let address = unique_name("ADDRESS");
        env.set(host.clone(), "localhost".to_string());
        env.set(port.clone(), "27017".to_string());
        env.set(address.clone(), format!("${{{host}}}:${{{port}}}"));

        assert_eq!(
            expand_env_vars(&format!("mongodb://${{{address}}}/app")).unwrap(),
            "mongodb://localhost:27017/app"
        );
    }

    #[test]
    fn leaves_literal_values_unchanged() {
        assert_eq!(
            expand_env_vars("literal-api-token").unwrap(),
            "literal-api-token"
        );
    }

    #[test]
    fn rejects_reference_cycles() {
        let mut env = TestEnv::new();
        let first = unique_name("FIRST");
        let second = unique_name("SECOND");
        env.set(first.clone(), format!("${{{second}}}"));
        env.set(second.clone(), format!("${{{first}}}"));

        let error = expand_env_vars(&format!("${{{first}}}")).unwrap_err();
        assert!(error.to_string().contains("reference cycle"));
        assert!(error.to_string().contains(&first));
        assert!(error.to_string().contains(&second));
    }

    #[test]
    fn rejects_missing_and_unclosed_variables() {
        let missing = unique_name("MISSING");
        assert!(expand_env_vars(&format!("${{{missing}}}"))
            .unwrap_err()
            .to_string()
            .contains("is not set"));
        assert!(expand_env_vars("${NOT_CLOSED")
            .unwrap_err()
            .to_string()
            .contains("Unclosed"));
    }
}
