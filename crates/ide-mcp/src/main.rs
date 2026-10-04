//! `choro-mcp` — Choro's first-party MCP server.
//!
//! A small, synchronous stdio server (JSON-RPC 2.0, newline-delimited) that
//! exposes IDE capabilities to the coding agents we launch. It reads the same
//! local store the GUI uses, so credentials never leave this process — the
//! agent only ever sees the *result* of a tool call, never an API token.
//!
//! Scope is passed at launch (`--project-id <uuid>` or `IDE_MCP_PROJECT_ID`) so
//! a given agent's server can only reach its own project's data.
//!
//! Today it ships task-tracker reads (`task_read`, `task_list`); the tool
//! registry in [`tools`] is built to grow.

mod mcp;
mod tools;

fn main() {
    let mut studio = false;
    let mut review_run = None;
    let mut project_id = None;
    let mut agent_id = None;
    let mut data_root = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--review-run" => {
                review_run = Some(args.next().and_then(|value| uuid::Uuid::parse_str(&value).ok()).unwrap_or_else(|| {
                    eprintln!("ide-mcp: --review-run requires a valid run ID"); std::process::exit(2);
                }));
            }
            "--studio" => studio = true,
            "--project-id" => {
                project_id = args
                    .next()
                    .and_then(|value| uuid::Uuid::parse_str(value.trim()).ok());
            }
            "--agent-id" => {
                agent_id = args
                    .next()
                    .and_then(|value| uuid::Uuid::parse_str(value.trim()).ok());
            }
            "--data-root" => {
                data_root = args
                    .next()
                    .filter(|value| !value.trim().is_empty())
                    .map(Into::into);
            }
            "--version" | "-V" => {
                println!("ide-mcp {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            _ => {}
        }
    }

    if project_id.is_none() {
        project_id = std::env::var("IDE_MCP_PROJECT_ID")
            .ok()
            .and_then(|value| uuid::Uuid::parse_str(value.trim()).ok());
    }

    if agent_id.is_none() {
        agent_id = std::env::var("IDE_MCP_AGENT_ID")
            .ok()
            .and_then(|value| uuid::Uuid::parse_str(value.trim()).ok());
    }

    if project_id.is_none() {
        eprintln!(
            "ide-mcp: warning: no --project-id / IDE_MCP_PROJECT_ID; task tools will refuse to read"
        );
    }
    let mut server = mcp::Server::new(project_id, agent_id, data_root);
    server.set_studio(studio);
    server.set_review_run(review_run);
    if let Err(error) = server.run() {
        eprintln!("ide-mcp: fatal: {error:#}");
        std::process::exit(1);
    }
}
