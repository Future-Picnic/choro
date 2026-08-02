use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context as _, Result};
use ide_core::{AgentKind, AppConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

const AGENT_CAPABILITY_CACHE_SCHEMA_VERSION: u32 = 1;
pub const CHORO_RIFFS_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCapabilitySource {
    Preview,
    ChoroRiff,
    Command,
    Skill,
    #[serde(other)]
    Legacy,
}

impl AgentCapabilitySource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Preview => "Preview",
            Self::ChoroRiff => "Riff",
            Self::Command => "Command",
            Self::Skill => "Skill",
            Self::Legacy => "Legacy",
        }
    }

    pub fn priority(self) -> u8 {
        match self {
            Self::Preview => 0,
            Self::ChoroRiff => 1,
            Self::Skill => 2,
            Self::Command => 3,
            Self::Legacy => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCapability {
    pub provider: AgentKind,
    pub source: AgentCapabilitySource,
    pub name: String,
    pub title: String,
    pub invocation: String,
    pub description: Option<String>,
    /// Choro Riffs carry their reusable prompt privately instead of exposing a
    /// provider-specific invocation token in the composer.
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl AgentCapability {
    pub fn is_choro_preview(&self) -> bool {
        self.source == AgentCapabilitySource::Preview
    }

    pub fn is_choro_riff(&self) -> bool {
        self.source == AgentCapabilitySource::ChoroRiff
    }

    pub fn is_legacy(&self) -> bool {
        self.source == AgentCapabilitySource::Legacy
    }

    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().trim_start_matches('/').to_ascii_lowercase();
        query.is_empty()
            || self.name.to_ascii_lowercase().contains(&query)
            || self.title.to_ascii_lowercase().contains(&query)
            || self.invocation.to_ascii_lowercase().contains(&query)
            || self
                .description
                .as_ref()
                .is_some_and(|description| description.to_ascii_lowercase().contains(&query))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoroRiff {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub instructions: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl ChoroRiff {
    pub fn capability_for(&self, provider: AgentKind) -> AgentCapability {
        AgentCapability {
            provider,
            source: AgentCapabilitySource::ChoroRiff,
            name: self.name.clone(),
            title: self.name.clone(),
            invocation: String::new(),
            description: self.description.clone(),
            instructions: Some(self.instructions.clone()),
            enabled: self.enabled,
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().trim_start_matches('/').to_ascii_lowercase();
        query.is_empty()
            || self.name.to_ascii_lowercase().contains(&query)
            || self
                .description
                .as_ref()
                .is_some_and(|description| description.to_ascii_lowercase().contains(&query))
            || self.instructions.to_ascii_lowercase().contains(&query)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChoroRiffStore {
    pub schema_version: u32,
    pub riffs: Vec<ChoroRiff>,
}

impl Default for ChoroRiffStore {
    fn default() -> Self {
        Self {
            schema_version: CHORO_RIFFS_SCHEMA_VERSION,
            riffs: default_choro_riffs(),
        }
    }
}

impl ChoroRiffStore {
    pub fn path() -> PathBuf {
        AppConfig::config_path()
            .parent()
            .map(|path| path.join("choro_riffs.json"))
            .unwrap_or_else(|| PathBuf::from("choro_riffs.json"))
    }

    pub fn load() -> Self {
        let path = Self::path();
        let Ok(text) = fs::read_to_string(&path) else {
            let store = Self::default();
            let _ = store.save();
            return store;
        };
        let Ok(mut store) = serde_json::from_str::<Self>(&text) else {
            return Self::default();
        };

        if store.schema_version < CHORO_RIFFS_SCHEMA_VERSION {
            for riff in default_choro_riffs() {
                if !store.riffs.iter().any(|existing| {
                    existing.id == riff.id || existing.name.eq_ignore_ascii_case(&riff.name)
                }) {
                    store.riffs.push(riff);
                }
            }
            store.schema_version = CHORO_RIFFS_SCHEMA_VERSION;
            store.riffs.sort_by(|left, right| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            });
            let _ = store.save();
        }

        store
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        let dir = path
            .parent()
            .context("Choro Riffs path has no parent directory")?;
        fs::create_dir_all(dir).context("failed to create Choro Riffs directory")?;
        let json = serde_json::to_string_pretty(self).context("failed to serialize Choro Riffs")?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json).context("failed to write Choro Riffs")?;
        fs::rename(&tmp, path).context("failed to move Choro Riffs into place")?;
        Ok(())
    }

    pub fn capabilities_for(&self, provider: AgentKind) -> Vec<AgentCapability> {
        self.riffs
            .iter()
            .filter(|riff| riff.enabled)
            .map(|riff| riff.capability_for(provider))
            .collect()
    }
}

fn default_choro_riffs() -> Vec<ChoroRiff> {
    vec![
        ChoroRiff {
            id: Uuid::from_u128(0x9d6d_4a7d_a087_4728_9105_0000_0000_0001),
            name: "UI Designer".into(),
            description: Some("Design or refine clear, polished, accessible interfaces.".into()),
            instructions: r#"Design or refine the interface as a senior product designer who can also work in the codebase.

1. Understand the user goal, primary task, platform, constraints, and existing product language before changing UI. Inspect the current screen, surrounding flows, shared components, tokens, and established interaction patterns.
2. Turn the task into a small set of user states and decisions. Cover the happy path plus empty, loading, error, disabled, destructive, keyboard-focus, overflow, and narrow-window states that are relevant.
3. Establish hierarchy before decoration: make the primary action and key information obvious; group related controls; use consistent alignment, spacing, typography, density, and progressive disclosure. Remove redundant explanation and visual noise.
4. Reuse the product design system and platform conventions. Do not introduce one-off controls, arbitrary colors, icons, spacing, or interaction patterns when a shared treatment exists.
5. Meet WCAG 2.2 expectations: sufficient contrast, visible focus, keyboard operation, meaningful labels, non-color status cues, sensible reading order, and usable target sizes. Prefer familiar, semantic controls.
6. Preserve product behavior and data while refining the presentation. Make responsive and theme-aware choices that work with real content, localization, and long values.
7. Implement the smallest coherent solution. Explain important design decisions and verify the affected states with the most appropriate available checks. Do not claim visual quality you did not inspect."#.into(),
            enabled: true,
        },
        ChoroRiff {
            id: Uuid::from_u128(0x9d6d_4a7d_a087_4728_9105_0000_0000_0002),
            name: "Frontend".into(),
            description: Some("Build maintainable, accessible, fast frontend experiences.".into()),
            instructions: r#"Build or improve the frontend with production-quality engineering practices.

1. Read the existing stack, architecture, design system, routing, state, data-fetching, testing, and repository conventions before choosing an approach.
2. Model the UI as focused components with clear responsibilities. Keep state minimal and canonical; derive values instead of duplicating them, keep ownership close to where state changes, and make data flow explicit.
3. Use semantic platform elements and accessible interactions first. Support keyboard use, focus states, labels, screen readers, reduced motion where relevant, responsive layouts, and all meaningful loading, empty, error, success, and disabled states.
4. Keep types and component APIs precise. Validate untrusted data at boundaries, handle async races and cancellation, avoid stale state, and surface actionable failures without leaking sensitive details.
5. Preserve performance by default: avoid unnecessary dependencies and renders, defer noncritical work, optimize resource loading and media, and measure before making complex optimizations. Consider LCP, INP, and CLS for web work.
6. Reuse shared components and tokens. Match the codebase's styling and testing approach instead of creating parallel abstractions.
7. Add or update focused tests for behavior and edge cases. Run proportionate type, lint, unit, integration, build, and accessibility checks available in the project, and report exactly what was verified."#.into(),
            enabled: true,
        },
        ChoroRiff {
            id: Uuid::from_u128(0x9d6d_4a7d_a087_4728_9105_0000_0000_0003),
            name: "Backend".into(),
            description: Some("Design secure, reliable, observable backend systems.".into()),
            instructions: r#"Work as a senior backend engineer focused on correctness, security, operability, and simple system boundaries.

1. Inspect the existing architecture, contracts, data ownership, deployment model, and conventions. Clarify the caller, inputs, outputs, invariants, scale, consistency needs, and failure behavior before coding.
2. Keep responsibilities and interfaces small. Prefer explicit contracts, dependency direction, and simple synchronous flows unless the requirements justify queues, distributed coordination, caching, or new services.
3. Validate and normalize untrusted input at the boundary. Enforce authentication and object-level authorization server-side, use least privilege, protect secrets and sensitive data, and consider OWASP API risks for every exposed endpoint.
4. Design failure behavior intentionally: atomic writes, idempotency where retries can occur, bounded timeouts, safe retry/backoff only for transient failures, concurrency control, and graceful partial failure. Never hide consistency tradeoffs.
5. Make changes operable with structured logs, useful metrics/traces, correlation context, health signals, and errors that help operators without exposing internals. Consider capacity, rate limits, recovery, backups, and rollback.
6. Keep API and data migrations backward-compatible when possible. Stage risky changes so old and new versions can coexist during rollout.
7. Test domain behavior, authorization, invalid input, boundaries, concurrency, and failure paths at the appropriate levels. Run the repository's checks and clearly state remaining risks or unverified assumptions."#.into(),
            enabled: true,
        },
        ChoroRiff {
            id: Uuid::from_u128(0x9d6d_4a7d_a087_4728_9105_0000_0000_0004),
            name: "DB Expert".into(),
            description: Some("Model sound schemas, queries, migrations, and database operations.".into()),
            instructions: r#"Handle database work from the workload and invariants outward, not from preferred schema patterns inward.

1. Inspect the current database engine, schema, migrations, representative data volume, access patterns, consistency requirements, retention rules, and operational constraints. Ask for missing facts that materially change the design.
2. Model entities, relationships, ownership, lifecycle, and invariants explicitly. Prefer a normalized relational design by default, then denormalize only for a measured access or reliability need with a clear synchronization strategy.
3. Enforce correctness in the database with appropriate primary keys, foreign keys, unique, not-null, check, and exclusion constraints. Choose precise data types and define timestamp, timezone, money, identifier, and null semantics deliberately.
4. Design indexes from real query predicates, joins, ordering, selectivity, and write volume. Remember that indexes speed reads but add storage and write overhead. Use query plans and representative measurements before and after performance changes.
5. Make transactions and concurrency behavior explicit. Prevent lost updates, duplicates, and race conditions with constraints, locking, isolation, or optimistic versioning appropriate to the workload.
6. Plan safe migrations: backward-compatible expand/migrate/contract steps, bounded batches, resumable backfills, lock awareness, verification queries, monitoring, rollback or roll-forward strategy, and backup/recovery implications.
7. Review queries for correctness, injection safety, excessive round trips, unbounded reads, N+1 behavior, and tenant isolation. Add focused tests and document important invariants and operational tradeoffs."#.into(),
            enabled: true,
        },
        ChoroRiff {
            id: Uuid::from_u128(0x9d6d_4a7d_a087_4728_9105_0000_0000_0005),
            name: "Analytics".into(),
            description: Some("Plan trustworthy events, instrumentation, and product analysis.".into()),
            instructions: r#"Treat analytics as a decision system, not a request to track every click.

1. Start with the product decision, user behavior question, and success metric. Define how the result will be segmented, compared, and acted on before proposing instrumentation.
2. Inspect the existing analytics providers, tracking plan, identity model, consent/privacy rules, event helpers, environments, dashboards, and naming conventions. Reuse the current taxonomy where it is sound.
3. Specify the smallest set of meaningful core events with rich properties. Use stable object-action event names and consistent casing; never generate event names dynamically. Define for every event: trigger, owner, description, source, required properties, property types/allowed values, and sample payload.
4. Distinguish event properties (context at the moment of an action) from user/account properties (durable traits). Define anonymous-to-known identity, account/group identity, deduplication, timestamps, revenue, and server-vs-client ownership explicitly.
5. Minimize sensitive data. Do not collect secrets or unnecessary PII; classify allowed sensitive properties, honor consent and deletion requirements, and avoid putting high-cardinality free text into analytics without a clear need.
6. Centralize and type instrumentation where the stack supports it. Validate payloads against the tracking plan, prevent duplicate firing, separate test data, and verify events and properties end-to-end in the destination.
7. When analyzing data, state the population, time range, filters, metric definition, data-quality caveats, and uncertainty. Prefer funnels, retention, cohorts, or segmentation that answer the stated question, then turn findings into concrete recommendations and follow-up measurements."#.into(),
            enabled: true,
        },
        ChoroRiff {
            id: Uuid::from_u128(0x9d6d_4a7d_a087_4728_9105_0000_0000_0006),
            name: "Code Reviewer".into(),
            description: Some("Review changes deeply for correctness, risk, and maintainability.".into()),
            instructions: r#"Before reviewing, ask the user to choose exactly one scope:
- Conversation scope: only changes made as part of the current conversation/task.
- Branch scope: every relevant change currently present on the branch or working tree.

Wait for the answer when the scope is not already explicit. Do not infer or silently broaden it.

Then perform the review:
1. Read the requirements, diff, surrounding code, repository instructions, and relevant tests. Understand the change at system level before commenting line by line.
2. Prioritize defects that can affect correctness, security, authorization, privacy, data integrity, concurrency, performance, reliability, compatibility, or user behavior. Check failure and rollback paths, not only the happy path.
3. Verify that tests meaningfully cover changed behavior and would fail for the defect they claim to prevent. Run safe, relevant checks when useful, but do not modify the implementation unless the user also asks for fixes.
4. Report only actionable findings supported by evidence. For each finding give severity, a concise title, exact file/line location, the failing scenario and impact, and a practical direction for correction.
5. Separate blocking defects from non-blocking suggestions. Do not present personal style preferences as defects when the code follows project conventions.
6. Review every relevant changed line and its necessary context. If no actionable defects remain, say so clearly and mention meaningful residual risks or untested areas rather than inventing findings."#.into(),
            enabled: true,
        },
    ]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentCapabilityCacheFile {
    pub schema_version: u32,
    pub cwd: String,
    pub refreshed_at: Option<String>,
    pub capabilities: Vec<AgentCapability>,
}

impl Default for AgentCapabilityCacheFile {
    fn default() -> Self {
        Self {
            schema_version: AGENT_CAPABILITY_CACHE_SCHEMA_VERSION,
            cwd: String::new(),
            refreshed_at: None,
            capabilities: Vec::new(),
        }
    }
}

impl AgentCapabilityCacheFile {
    pub fn cache_path() -> PathBuf {
        AppConfig::config_path()
            .parent()
            .map(|path| path.join("agent_capabilities.json"))
            .unwrap_or_else(|| PathBuf::from("agent_capabilities.json"))
    }

    pub fn load() -> Self {
        fs::read_to_string(Self::cache_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::cache_path();
        let dir = path
            .parent()
            .context("agent capability cache path has no parent directory")?;
        fs::create_dir_all(dir).context("failed to create agent capability cache directory")?;
        let json =
            serde_json::to_string_pretty(self).context("failed to serialize capabilities")?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json).context("failed to write agent capability cache")?;
        fs::rename(&tmp, path).context("failed to move agent capability cache into place")?;
        Ok(())
    }

    pub fn capabilities_for(&self, provider: AgentKind) -> Vec<AgentCapability> {
        self.capabilities
            .iter()
            .filter(|capability| capability.provider == provider && capability.enabled)
            .cloned()
            .collect()
    }

    /// Global Choro Riffs are always offered before runtime-discovered agent
    /// skills, regardless of the active project or provider.
    pub fn available_for(provider: AgentKind) -> Vec<AgentCapability> {
        let mut capabilities = ChoroRiffStore::load().capabilities_for(provider);
        capabilities.extend(Self::load().capabilities_for(provider));
        capabilities
    }

    pub fn refresh_from_runtime(cwd: &str) -> Result<Self, String> {
        let script = Self::runtime_skill_script_path()
            .ok_or_else(|| "Could not find scripts/list-agent-runtime-skills.mjs".to_string())?;
        let output = Command::new("node")
            .arg(script)
            .arg("--provider")
            .arg("all")
            .arg("--json")
            .arg("--cwd")
            .arg(cwd)
            .output()
            .map_err(|error| format!("Failed to run node diagnostic: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("Diagnostic exited with status {}", output.status)
            } else {
                stderr
            });
        }
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("Diagnostic returned invalid JSON: {error}"))?;
        let cache = Self::from_diagnostic_json(&value);
        cache
            .save()
            .map_err(|error| format!("Failed to save skill cache: {error:#}"))?;
        Ok(cache)
    }

    fn runtime_skill_script_path() -> Option<PathBuf> {
        let bundle_script = std::env::current_exe().ok().and_then(|exe| {
            exe.parent()?
                .parent()?
                .join("Resources")
                .join("scripts")
                .join("list-agent-runtime-skills.mjs")
                .canonicalize()
                .ok()
        });
        if bundle_script.is_some() {
            return bundle_script;
        }
        std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join("scripts").join("list-agent-runtime-skills.mjs"))
            .filter(|path| path.exists())
    }

    pub fn from_diagnostic_json(value: &Value) -> Self {
        let mut capabilities = Vec::new();
        if let Some(skills) = value
            .get("codex")
            .and_then(|codex| codex.get("skills"))
            .and_then(Value::as_array)
        {
            capabilities.extend(skills.iter().filter_map(codex_skill_from_value));
        }
        if let Some(commands) = value
            .get("claude")
            .and_then(|claude| claude.get("slashCommands"))
            .and_then(Value::as_array)
        {
            capabilities.extend(commands.iter().filter_map(claude_command_from_value));
        }
        if let Some(skills) = value
            .get("claude")
            .and_then(|claude| claude.get("skills"))
            .and_then(Value::as_array)
        {
            capabilities.extend(skills.iter().filter_map(claude_skill_from_value));
        }
        capabilities.sort_by(|left, right| {
            (
                left.provider.label(),
                left.title.to_ascii_lowercase(),
                left.source.label(),
            )
                .cmp(&(
                    right.provider.label(),
                    right.title.to_ascii_lowercase(),
                    right.source.label(),
                ))
        });
        Self {
            schema_version: AGENT_CAPABILITY_CACHE_SCHEMA_VERSION,
            cwd: value
                .get("cwd")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            refreshed_at: value
                .get("generatedAt")
                .and_then(Value::as_str)
                .map(str::to_string),
            capabilities,
        }
    }
}

fn codex_skill_from_value(skill: &Value) -> Option<AgentCapability> {
    let enabled = skill
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !enabled {
        return None;
    }
    let name = first_string(skill, &["name", "id", "skill", "slug"])?;
    let invocation = first_string(skill, &["invocation", "command"])
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("${name} "));
    let title = first_string(skill, &["title", "displayName", "display_name"])
        .or_else(|| {
            skill
                .get("interface")
                .and_then(|interface| first_string(interface, &["displayName"]))
        })
        .unwrap_or_else(|| name.clone());
    let description = skill
        .get("interface")
        .and_then(|interface| first_string(interface, &["shortDescription"]))
        .or_else(|| first_string(skill, &["description", "summary"]));
    Some(AgentCapability {
        provider: AgentKind::Codex,
        source: AgentCapabilitySource::Skill,
        name,
        title,
        invocation: ensure_trailing_space(invocation),
        description,
        instructions: None,
        enabled,
    })
}

fn claude_command_from_value(command: &Value) -> Option<AgentCapability> {
    let raw_name = command.as_str()?.trim();
    let name = raw_name.trim_start_matches('/').to_string();
    if name.is_empty() {
        return None;
    }
    Some(AgentCapability {
        provider: AgentKind::Claude,
        source: AgentCapabilitySource::Command,
        title: name.clone(),
        invocation: format!("/{name} "),
        name,
        description: None,
        instructions: None,
        enabled: true,
    })
}

fn claude_skill_from_value(skill: &Value) -> Option<AgentCapability> {
    let raw_name = skill.as_str()?.trim();
    let name = raw_name.trim_start_matches('/').to_string();
    if name.is_empty() {
        return None;
    }
    Some(AgentCapability {
        provider: AgentKind::Claude,
        source: AgentCapabilitySource::Skill,
        title: name.clone(),
        invocation: format!("/{name} "),
        name,
        description: None,
        instructions: None,
        enabled: true,
    })
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn ensure_trailing_space(value: String) -> String {
    if value.ends_with(' ') {
        value
    } else {
        format!("{value} ")
    }
}

fn default_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_diagnostic_json_into_provider_capabilities() {
        let cache = AgentCapabilityCacheFile::from_diagnostic_json(&json!({
            "cwd": "/tmp/app",
            "generatedAt": "2026-06-22T00:00:00.000Z",
            "codex": {
                "skills": [
                    { "name": "browser", "title": "Browser", "description": "Use a browser" },
                    { "name": "disabled", "enabled": false }
                ]
            },
            "claude": {
                "slashCommands": ["compact"],
                "skills": ["workflow"]
            }
        }));

        assert_eq!(cache.cwd, "/tmp/app");
        assert_eq!(
            cache.refreshed_at.as_deref(),
            Some("2026-06-22T00:00:00.000Z")
        );
        assert_eq!(cache.capabilities_for(AgentKind::Codex).len(), 1);
        assert_eq!(
            cache.capabilities_for(AgentKind::Codex)[0].invocation,
            "$browser "
        );
        assert_eq!(cache.capabilities_for(AgentKind::Claude).len(), 2);
        assert!(cache
            .capabilities_for(AgentKind::Claude)
            .iter()
            .any(|capability| capability.invocation == "/compact "));
    }

    #[test]
    fn default_riff_store_contains_the_researched_starter_set() {
        let store = ChoroRiffStore::default();
        let names = store
            .riffs
            .iter()
            .map(|riff| riff.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(store.schema_version, CHORO_RIFFS_SCHEMA_VERSION);
        assert_eq!(
            names,
            vec![
                "UI Designer",
                "Frontend",
                "Backend",
                "DB Expert",
                "Analytics",
                "Code Reviewer",
            ]
        );
        assert!(store
            .riffs
            .iter()
            .all(|riff| riff.enabled && !riff.instructions.trim().is_empty()));
    }
}
