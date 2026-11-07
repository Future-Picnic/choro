//! Provider-owned connections retain their OAuth storage. Choro only selects
//! supported transport identities and narrows their callable tools.
use super::*;
use ide_core::studio::StudioIntegrations;

pub(super) fn settings(agent: &AgentRecord) -> anyhow::Result<StudioIntegrations> {
    if agent.studio_context.as_ref().is_some_and(|context| context.target == ide_core::studio::StudioAgentTarget::DesignSystemImport) {
        return Ok(StudioIntegrations::disabled());
    }
    Ok(ide_core::studio::StudioStore::for_project(&agent.project_path)?.scope(agent.id)?.integrations)
}

pub(super) fn codex_server_overrides(entries: &[Value], policy: &StudioIntegrations) -> Vec<String> {
    let mut overrides = Vec::new();
    for entry in entries {
        let Some(name) = entry.get("name").and_then(Value::as_str) else { continue; };
        if !entry.get("enabled").and_then(Value::as_bool).unwrap_or(false) { continue; }
        let allowed = entry.pointer("/transport/url").and_then(Value::as_str)
            .and_then(|url| policy.tools_for_url(url));
        if let Some(tools) = allowed {
            // Respect an existing, narrower provider tool allowlist too.
            let inherited = entry.get("enabled_tools").and_then(Value::as_array);
            let tools: Vec<_> = tools.iter().filter(|tool| inherited.is_none_or(|list| list.iter().any(|item| item.as_str() == Some(**tool)))).collect();
            overrides.push(format!("mcp_servers.{name}.enabled_tools={}", serde_json::to_string(&tools).unwrap()));
            overrides.push(format!("mcp_servers.{name}.default_tools_approval_mode=\"approve\""));
            overrides.push(format!("mcp_servers.{name}.required=false"));
        } else {
            overrides.push(format!("mcp_servers.{name}.enabled=false"));
        }
    }
    overrides
}

/// Only copy supported MCP transport definitions, never OAuth credential files,
/// user settings, hooks, plugins or permission rules. Claude keeps token refresh.
pub(super) fn claude_servers(cwd: &Path, policy: &StudioIntegrations) -> serde_json::Map<String, Value> {
    let mut servers = serde_json::Map::new();
    let Some(home) = home_dir() else { return servers; };
    let user_path = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from)
        .map(|root| root.join(".claude.json")).unwrap_or_else(|| home.join(".claude.json"));
    let user = fs::read(user_path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).unwrap_or(Value::Null);
    let local = &user["projects"][cwd.to_string_lossy().as_ref()];
    collect_claude_servers(&user["mcpServers"], policy, &mut servers);
    if let Ok(bytes) = fs::read(cwd.join(".mcp.json")) {
        if let Ok(config) = serde_json::from_slice::<Value>(&bytes) {
            collect_claude_servers(&config["mcpServers"], policy, &mut servers);
        }
    }
    // Local project definitions have precedence over checked-in definitions.
    collect_claude_servers(&local["mcpServers"], policy, &mut servers);
    for config in [&user, local] {
        if let Some(disabled) = config["disabledMcpServers"].as_array() {
            for name in disabled.iter().filter_map(Value::as_str) { servers.remove(name); }
        }
    }
    servers
}

fn collect_claude_servers(value: &Value, policy: &StudioIntegrations, servers: &mut serde_json::Map<String, Value>) {
    let Some(entries) = value.as_object() else { return; };
    for (name, config) in entries {
        if matches!(name.as_str(), "choro" | "ide") || !safe_name(name) { continue; }
        if matches!(config["type"].as_str(), Some("http" | "sse"))
            && config["url"].as_str().and_then(|url| policy.tools_for_url(url)).is_some() {
            servers.insert(name.clone(), config.clone());
        }
    }
}

pub(super) fn safe_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_keeps_only_selected_reference_tools_and_respects_existing_limits() {
        let entries = vec![
            json!({"name":"reference","enabled":true,"transport":{"url":"https://api.mobbin.com/mcp"},"enabled_tools":["search_screens","save_content"]}),
            json!({"name":"mobbin","enabled":true,"transport":{"url":"https://unrelated.example/mcp"}}),
            json!({"name":"off","enabled":false,"transport":{"url":"https://api.mobbin.com/mcp"}}),
        ];
        let overrides = codex_server_overrides(&entries, &StudioIntegrations::default());
        assert!(overrides.contains(&"mcp_servers.reference.enabled_tools=[\"search_screens\"]".into()));
        assert!(overrides.contains(&"mcp_servers.mobbin.enabled=false".into()));
        assert!(!overrides.iter().any(|arg| arg.contains("off")));
        let mut policy = StudioIntegrations::default(); policy.mobbin = false;
        assert!(codex_server_overrides(&entries, &policy).contains(&"mcp_servers.reference.enabled=false".into()));
    }

    #[test]
    fn claude_does_not_import_settings_or_unrelated_or_spoofed_servers() {
        let mut servers = serde_json::Map::new();
        collect_claude_servers(&json!({
            "research":{"type":"http","url":"https://api.mobbin.com/mcp"},
            "mobbin":{"type":"stdio","command":"anything"},
            "choro":{"type":"http","url":"https://api.mobbin.com/mcp"},
            "other":{"type":"http","url":"https://example.test/mcp"}
        }), &StudioIntegrations::default(), &mut servers);
        assert_eq!(servers.keys().collect::<Vec<_>>(), vec!["research"]);
    }
}
