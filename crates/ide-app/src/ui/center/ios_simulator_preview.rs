//! Explicit, UDID-scoped access to booted Apple Simulators.
//!
//! Device discovery is cheap and does not attach to a Simulator. The interactive
//! bridge is started only after a user selects a `simulator://<UDID>` source and
//! is stopped as soon as that source is no longer the active project Preview.

use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context as _, Result};
use serde::Deserialize;

pub(super) const SOURCE_PREFIX: &str = "simulator://";
const SERVE_SIM_PACKAGE: &str = "serve-sim@0.1.45";
const BRIDGE_START_TIMEOUT: Duration = Duration::from_secs(20);
const BRIDGE_START_RETRY: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BootedSimulator {
    pub udid: String,
    pub name: String,
    pub runtime: String,
}

impl BootedSimulator {
    pub fn source_key(&self) -> String {
        source_key(&self.udid)
    }

    pub fn display_label(&self) -> String {
        format!("{} · {}", self.name, self.runtime)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SimulatorBridgeEndpoint {
    pub udid: String,
    pub url: String,
}

struct ActiveBridge {
    endpoint: SimulatorBridgeEndpoint,
    child: Child,
}

/// Owns at most one interactive Simulator bridge for the whole Choro window.
///
/// Keeping the server in the foreground process group gives Choro exact
/// lifecycle ownership: closing Preview tears down the stream, and a normal app
/// exit cannot leave an unscoped `serve-sim` daemon consuming resources.
#[derive(Default)]
pub(super) struct SimulatorBridgeController {
    active: Option<ActiveBridge>,
}

impl SimulatorBridgeController {
    pub fn reconcile(
        &mut self,
        desired_udid: Option<&str>,
    ) -> Result<Option<SimulatorBridgeEndpoint>> {
        let Some(desired_udid) = desired_udid else {
            self.disconnect();
            return Ok(None);
        };

        if let Some(active) = self.active.as_mut() {
            let still_running = active
                .child
                .try_wait()
                .context("check Simulator Preview bridge")?
                .is_none();
            if still_running && active.endpoint.udid == desired_udid {
                return Ok(Some(active.endpoint.clone()));
            }
        }

        self.disconnect();
        self.start(desired_udid).map(Some)
    }

    fn start(&mut self, udid: &str) -> Result<SimulatorBridgeEndpoint> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .context("reserve a local Simulator Preview port")?;
        let port = listener
            .local_addr()
            .context("read the local Simulator Preview port")?
            .port();
        drop(listener);

        let mut command = serve_sim_command()?;
        command
            .args([
                "--quiet",
                "--port",
                &port.to_string(),
                "--panes",
                "none",
                "--fit",
                "--codec",
                "auto",
                udid,
            ])
            .env(
                "PATH",
                crate::state::agent_chat::protocol::agent_command_path_env(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        command.process_group(0);

        let mut child = command
            .spawn()
            .with_context(|| format!("start interactive Simulator Preview for {udid}"))?;
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let deadline = Instant::now() + BRIDGE_START_TIMEOUT;
        loop {
            if TcpStream::connect_timeout(&address, BRIDGE_START_RETRY).is_ok() {
                break;
            }
            if let Some(status) = child
                .try_wait()
                .context("wait for the Simulator Preview bridge")?
            {
                return Err(anyhow!(
                    "Simulator Preview bridge exited before connecting ({status})"
                ));
            }
            if Instant::now() >= deadline {
                terminate_bridge_process(&mut child);
                return Err(anyhow!("Simulator Preview bridge timed out"));
            }
            thread::sleep(BRIDGE_START_RETRY);
        }

        let endpoint = SimulatorBridgeEndpoint {
            udid: udid.to_string(),
            url: format!("http://127.0.0.1:{port}"),
        };
        self.active = Some(ActiveBridge {
            endpoint: endpoint.clone(),
            child,
        });
        Ok(endpoint)
    }

    fn disconnect(&mut self) {
        if let Some(mut active) = self.active.take() {
            terminate_bridge_process(&mut active.child);
        }
    }
}

impl Drop for SimulatorBridgeController {
    fn drop(&mut self) {
        self.disconnect();
    }
}

fn terminate_bridge_process(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }

    #[cfg(unix)]
    {
        let process_group = child.id() as libc::pid_t;
        if process_group > 1 {
            unsafe {
                libc::kill(-process_group, libc::SIGTERM);
            }
            for _ in 0..8 {
                thread::sleep(Duration::from_millis(25));
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
            }
            unsafe {
                libc::kill(-process_group, libc::SIGKILL);
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
}

fn serve_sim_command() -> Result<Command> {
    if let Some(script) = serve_sim_script_path() {
        let node = crate::state::agent_chat::protocol::find_agent_cli_executable("node")
            .ok_or_else(|| anyhow!("Node.js 20 or newer is required for Simulator Preview"))?;
        let mut command = Command::new(node);
        command.arg(script);
        return Ok(command);
    }

    let npx = crate::state::agent_chat::protocol::find_agent_cli_executable("npx")
        .ok_or_else(|| anyhow!("The bundled Simulator Preview bridge was not found"))?;
    let mut command = Command::new(npx);
    command.args(["--yes", SERVE_SIM_PACKAGE]);
    Ok(command)
}

fn serve_sim_script_path() -> Option<PathBuf> {
    let relative = Path::new("agent-chat").join("node_modules/serve-sim/dist/serve-sim.js");
    let bundle_resource = std::env::current_exe()
        .ok()
        .and_then(|executable| {
            executable
                .parent()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
        })
        .map(|contents| contents.join("Resources").join(&relative))
        .filter(|path| path.is_file());
    if bundle_resource.is_some() {
        return bundle_resource;
    }

    let repo_resource = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(relative);
    repo_resource.is_file().then_some(repo_resource)
}

#[derive(Deserialize)]
struct SimctlList {
    devices: BTreeMap<String, Vec<SimctlDevice>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimctlDevice {
    name: String,
    udid: String,
    state: String,
    #[serde(default = "available_by_default")]
    is_available: bool,
}

fn available_by_default() -> bool {
    true
}

pub(super) fn source_key(udid: &str) -> String {
    format!("{SOURCE_PREFIX}{udid}")
}

pub(super) fn source_udid(source: &str) -> Option<&str> {
    source
        .strip_prefix(SOURCE_PREFIX)
        .filter(|udid| !udid.is_empty())
}

fn runtime_label(identifier: &str) -> Option<String> {
    let runtime = identifier.rsplit(".SimRuntime.").next()?;
    let version = runtime.strip_prefix("iOS-")?;
    Some(format!("iOS {}", version.replace('-', ".")))
}

fn parse_booted_simulators(bytes: &[u8]) -> Result<Vec<BootedSimulator>> {
    let list: SimctlList = serde_json::from_slice(bytes).context("parse simctl device list")?;
    let mut simulators = list
        .devices
        .into_iter()
        .filter_map(|(runtime, devices)| Some((runtime_label(&runtime)?, devices)))
        .flat_map(|(runtime, devices)| {
            devices.into_iter().filter_map(move |device| {
                (device.state == "Booted" && device.is_available).then(|| BootedSimulator {
                    udid: device.udid,
                    name: device.name,
                    runtime: runtime.clone(),
                })
            })
        })
        .collect::<Vec<_>>();
    simulators.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.runtime.cmp(&right.runtime))
            .then_with(|| left.udid.cmp(&right.udid))
    });
    Ok(simulators)
}

pub(super) fn discover_booted_simulators() -> Result<Vec<BootedSimulator>> {
    let output = Command::new("xcrun")
        .args(["simctl", "list", "devices", "booted", "--json"])
        .output()
        .context("run xcrun simctl list")?;
    anyhow::ensure!(
        output.status.success(),
        "simctl device discovery failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    parse_booted_simulators(&output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_available_booted_ios_devices() {
        let json = br#"{
          "devices": {
            "com.apple.CoreSimulator.SimRuntime.iOS-18-1": [
              {"name":"iPhone 16 Pro","udid":"PHONE","state":"Booted","isAvailable":true},
              {"name":"iPhone SE","udid":"OFF","state":"Shutdown","isAvailable":true}
            ],
            "com.apple.CoreSimulator.SimRuntime.watchOS-11-0": [
              {"name":"Apple Watch","udid":"WATCH","state":"Booted","isAvailable":true}
            ],
            "com.apple.CoreSimulator.SimRuntime.iOS-17-5": [
              {"name":"Old iPhone","udid":"OLD","state":"Booted","isAvailable":false}
            ]
          }
        }"#;

        assert_eq!(
            parse_booted_simulators(json).unwrap(),
            vec![BootedSimulator {
                udid: "PHONE".to_string(),
                name: "iPhone 16 Pro".to_string(),
                runtime: "iOS 18.1".to_string(),
            }]
        );
    }

    #[test]
    fn simulator_sources_round_trip_the_udid() {
        let source = source_key("A-B-C");
        assert_eq!(source_udid(&source), Some("A-B-C"));
        assert_eq!(source_udid("https://localhost:3000"), None);
    }
}
