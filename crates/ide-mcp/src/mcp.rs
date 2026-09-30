//! Minimal MCP over stdio: newline-delimited JSON-RPC 2.0.
//!
//! We hand-roll the small slice of the protocol we need (`initialize`,
//! `tools/list`, `tools/call`, `ping`) rather than pull in a full SDK — it
//! keeps the dependency surface tiny and the behavior obvious. stdout carries
//! protocol messages only; all logging goes to stderr.

use std::{
    io::{BufRead, Write},
    path::PathBuf,
};

use anyhow::Result;
use serde_json::{json, Value};

use crate::tools::{ServerContext, ToolRegistry};

/// The newest protocol revision we advertise when a client doesn't pin one.
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const CHORO_NATIVE_TOOL_INSTRUCTIONS: &str = "Use create_choro_doc only when the user explicitly asks for a Choro doc, a document in Choro Docs, or equivalent wording. Use create_choro_script only when the user explicitly asks for a Choro script or a command in Choro's top-header Scripts control. When either request applies, use the matching Choro MCP tool instead of directly writing a .choro file or modifying Choro state. Ordinary repository documents, files, scripts, and commands should continue to use the normal coding tools.";

pub struct Server {
    ctx: ServerContext,
    tools: ToolRegistry,
}

impl Server {
    pub fn new(
        project_id: Option<uuid::Uuid>,
        agent_id: Option<uuid::Uuid>,
        data_root: Option<PathBuf>,
    ) -> Self {
        Self {
            ctx: ServerContext::new(project_id, agent_id, data_root),
            tools: ToolRegistry::default(),
        }
    }

    pub fn set_studio(&mut self, studio: bool) {
        self.ctx.studio = studio;
    }

    pub fn run(&mut self) -> Result<()> {
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        let stdout = std::io::stdout();
        let mut writer = stdout.lock();

        let mut line = String::new();
        loop {
            line.clear();
            let read = reader.read_line(&mut line)?;
            if read == 0 {
                break; // EOF — the client (agent) closed the pipe.
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let message: Value = match serde_json::from_str(trimmed) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("ide-mcp: ignoring malformed message: {error}");
                    continue;
                }
            };
            if let Some(response) = self.handle(&message) {
                let encoded = serde_json::to_string(&response)?;
                writeln!(writer, "{encoded}")?;
                writer.flush()?;
            }
        }
        Ok(())
    }

    /// Returns the JSON-RPC response to send, or `None` for notifications and
    /// anything that doesn't warrant a reply.
    fn handle(&mut self, message: &Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str);

        match method {
            Some("initialize") => Some(reply(id, self.initialize_result(message))),
            // Lifecycle notifications — acknowledged by doing nothing.
            Some("notifications/initialized") | Some("initialized") => None,
            Some("ping") => Some(reply(id, json!({}))),
            Some("tools/list") => Some(reply(
                id,
                json!({ "tools": self.tools.list_for(&self.ctx) }),
            )),
            Some("tools/call") => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                Some(reply(id, self.tools.call(&self.ctx, &params)))
            }
            Some(other) => {
                // Only requests (those with an id) get an error; notifications don't.
                id.map(|id| error(Some(id), -32601, &format!("method not found: {other}")))
            }
            // No method → this is a response/ack directed at us; ignore.
            None => None,
        }
    }

    fn initialize_result(&self, message: &Value) -> Value {
        let version = message
            .pointer("/params/protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_PROTOCOL_VERSION);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "instructions": format!("{}\n\n{}",CHORO_NATIVE_TOOL_INSTRUCTIONS,ide_core::agent_changes::AGENT_CHANGE_INSTRUCTIONS),
            "serverInfo": {
                "name": "ide-mcp",
                "version": env!("CARGO_PKG_VERSION"),
            }
        })
    }
}

fn reply(id: Option<Value>, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "result": result,
    })
}

fn error(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": { "code": code, "message": message },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_scopes_choro_native_creation_instructions() {
        let server = Server {
            ctx: ServerContext {
                studio: false,
                delegation_scope: None,
                project_id: None,
                agent_id: None,
                store: None,
            },
            tools: ToolRegistry::default(),
        };
        let initialized = server.initialize_result(&json!({
            "params": { "protocolVersion": DEFAULT_PROTOCOL_VERSION }
        }));
        let instructions = initialized
            .get("instructions")
            .and_then(Value::as_str)
            .unwrap();

        assert!(instructions.contains("explicitly asks for a Choro doc"));
        assert!(instructions.contains("create_choro_script"));
        assert!(instructions.contains("Ordinary repository documents"));
    }
}
