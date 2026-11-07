//! Explicit integration permissions shared by the provider adapters.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StudioIntegrations {
    pub mobbin: bool,
    pub figma: bool,
    pub image_generation: bool,
    pub aside: bool,
}

impl Default for StudioIntegrations {
    fn default() -> Self {
        Self { mobbin: true, figma: true, image_generation: true, aside: true }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct StudioIntegrationServer {
    pub url: &'static str,
    pub tools: &'static [&'static str],
}

impl StudioIntegrations {
    pub fn disabled() -> Self {
        Self { mobbin: false, figma: false, image_generation: false, aside: false }
    }
    /// Match transport identities, never a user-chosen server name. Unknown
    /// servers and new tools stay unavailable until explicitly supported.
    pub fn servers(&self) -> Vec<StudioIntegrationServer> {
        let mut servers = Vec::new();
        if self.mobbin {
            servers.push(StudioIntegrationServer { url: "https://api.mobbin.com/mcp", tools: &[
                "search_screens", "search_flows", "search_sections", "list_collections", "get_collection_contents",
            ] });
        }
        if self.figma {
            for url in ["https://mcp.figma.com/mcp", "http://127.0.0.1:3845/mcp", "http://localhost:3845/mcp"] {
                servers.push(StudioIntegrationServer { url, tools: &[
                    "get_design_context", "get_screenshot", "get_metadata", "get_variable_defs",
                    "get_code_connect_map", "get_figjam", "whoami",
                ] });
            }
        }
        if self.image_generation {
            servers.push(StudioIntegrationServer { url: "https://mcp.higgsfield.ai/mcp", tools: &[
                "use_higgsfield", "models_explore", "generate_image", "generate_image_batch",
                "get_preset_instructions", "get_presets", "prepare_generation", "jobs_wait",
                "job_status", "job_display", "show_generations", "show_generation_by_ids",
                "reveal_generation", "media_import_url", "remove_background", "outpaint_image", "upscale_image",
            ] });
        }
        servers
    }

    pub fn tools_for_url(&self, url: &str) -> Option<&'static [&'static str]> {
        let parsed = url::Url::parse(url).ok()?;
        if !parsed.username().is_empty() || parsed.password().is_some() || parsed.query().is_some() || parsed.fragment().is_some() {
            return None;
        }
        self.servers().into_iter().find(|server| {
            url::Url::parse(server.url).is_ok_and(|allowed| allowed == parsed)
        }).map(|server| server.tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_permissions_match_transport_and_exclude_mutations() {
        let mut settings = StudioIntegrations::default();
        let tools = settings.tools_for_url("https://api.mobbin.com/mcp").unwrap();
        assert!(tools.contains(&"search_screens"));
        assert!(!tools.contains(&"save_content"));
        assert!(!tools.contains(&"add_comment"));
        assert!(!settings.tools_for_url("https://mcp.figma.com/mcp").unwrap().contains(&"use_figma"));
        assert!(!settings.tools_for_url("https://mcp.higgsfield.ai/mcp").unwrap().contains(&"sandbox_exec"));
        for url in ["https://api.mobbin.com.attacker.test/mcp", "http://api.mobbin.com/mcp", "https://api.mobbin.com/mcp?token=x", "https://user@api.mobbin.com/mcp"] {
            assert!(settings.tools_for_url(url).is_none(), "{url}");
        }
        settings.mobbin = false;
        assert!(settings.tools_for_url("https://api.mobbin.com/mcp").is_none());
    }
}
