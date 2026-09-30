use super::*;

impl SettingsView {
    fn load_process_snapshot(projects: &[ProjectSource]) -> ProcessSnapshot {
        let agents = LocalStore::open_default()
            .and_then(|store| store.load_agents())
            .unwrap_or_default();
        Self::load_process_snapshot_inner(projects, &agents, true)
    }

    pub(crate) fn load_live_process_snapshot(
        projects: &[ProjectSource],
        agents: &[AgentRecord],
    ) -> ProcessSnapshot {
        Self::load_process_snapshot_inner(projects, agents, false)
    }

    fn load_process_snapshot_inner(
        projects: &[ProjectSource],
        agents: &[AgentRecord],
        include_footprint: bool,
    ) -> ProcessSnapshot {
        let root_pid = std::process::id() as i32;
        let output = Command::new("ps")
            .args(["-axo", "pid=,ppid=,pgid=,rss=,%cpu=,command="])
            .output();
        let Ok(output) = output else {
            return ProcessSnapshot {
                root_pid,
                error: Some("Unable to read process list".into()),
                ..Default::default()
            };
        };
        if !output.status.success() {
            return ProcessSnapshot {
                root_pid,
                error: Some("ps command failed".into()),
                ..Default::default()
            };
        }

        #[derive(Clone)]
        struct RawProcess {
            pid: i32,
            ppid: i32,
            pgid: i32,
            resident_bytes: u64,
            cpu: f64,
            command: String,
        }

        let mut table: HashMap<i32, RawProcess> = HashMap::new();
        let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let mut parts = line.split_whitespace();
            let (Some(pid), Some(ppid), Some(pgid), Some(rss), Some(cpu)) = (
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
            ) else {
                continue;
            };
            let Ok(pid) = pid.parse::<i32>() else {
                continue;
            };
            let Ok(ppid) = ppid.parse::<i32>() else {
                continue;
            };
            let Ok(pgid) = pgid.parse::<i32>() else {
                continue;
            };
            let rss_kb = rss.parse::<u64>().unwrap_or(0);
            let cpu = cpu.parse::<f64>().unwrap_or(0.0);
            let command = parts.collect::<Vec<_>>().join(" ");
            children.entry(ppid).or_default().push(pid);
            table.insert(
                pid,
                RawProcess {
                    pid,
                    ppid,
                    pgid,
                    resident_bytes: rss_kb.saturating_mul(1024),
                    cpu,
                    command,
                },
            );
        }

        let Some(root) = table.get(&root_pid).cloned() else {
            return ProcessSnapshot {
                root_pid,
                error: Some("Current app process was not found in ps output".into()),
                ..Default::default()
            };
        };

        let mut agent_by_process_group = HashMap::<i32, Option<Uuid>>::new();
        for process in table.values() {
            let Some(agent) = Self::infer_agent(&agents, None, &process.command) else {
                continue;
            };
            agent_by_process_group
                .entry(process.pgid)
                .and_modify(|existing| {
                    if existing.is_some_and(|id| id != agent.id) {
                        *existing = None;
                    }
                })
                .or_insert(Some(agent.id));
        }
        let resolve_agent_id = |process: &RawProcess| {
            let mut cursor = Some(process.pid);
            while let Some(pid) = cursor {
                let candidate = table.get(&pid)?;
                if let Some(agent) = Self::infer_agent(&agents, None, &candidate.command) {
                    return Some(agent.id);
                }
                if let Some(agent_id) = agent_by_process_group
                    .get(&candidate.pgid)
                    .and_then(|agent_id| *agent_id)
                {
                    return Some(agent_id);
                }
                cursor = (candidate.ppid > 1 && candidate.ppid != candidate.pid)
                    .then_some(candidate.ppid);
            }
            None
        };

        let mut stack = vec![root_pid];
        let mut tree = Vec::new();
        while let Some(pid) = stack.pop() {
            if let Some(process) = table.get(&pid) {
                tree.push(process.clone());
            }
            if let Some(kids) = children.get(&pid) {
                stack.extend(kids.iter().copied());
            }
        }

        let total_bytes: u64 = tree.iter().map(|p| p.resident_bytes).sum();
        let mut processes: Vec<ProcessInfo> = tree
            .iter()
            .map(|p| {
                let command = if p.command.is_empty() {
                    format!("pid {}", p.pid)
                } else {
                    p.command.clone()
                };
                let agent_id = resolve_agent_id(p)
                    .or_else(|| Self::infer_agent(&agents, None, &command).map(|agent| agent.id));
                let mut agent =
                    agent_id.and_then(|agent_id| agents.iter().find(|agent| agent.id == agent_id));
                let mut project = agent
                    .and_then(|agent| {
                        projects
                            .iter()
                            .find(|project| project.id == agent.project_id)
                    })
                    .or_else(|| Self::infer_project(projects, None, &command));
                if agent.is_none() && project.is_none() {
                    let cwd = Self::process_cwd(p.pid);
                    agent = Self::infer_agent(&agents, cwd.as_deref(), &command);
                    project = agent
                        .and_then(|agent| {
                            projects
                                .iter()
                                .find(|project| project.id == agent.project_id)
                        })
                        .or_else(|| Self::infer_project(projects, cwd.as_deref(), &command));
                }
                let project_id = agent
                    .map(|agent| agent.project_id)
                    .or_else(|| project.map(|project| project.id));
                let project_name = project.map(|project| project.name.clone()).or_else(|| {
                    agent.and_then(|agent| {
                        agent
                            .project_path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                    })
                });
                ProcessInfo {
                    pid: p.pid,
                    memory_bytes: p.resident_bytes,
                    cpu: p.cpu,
                    project_id,
                    project: project_name,
                    agent_id: agent.map(|agent| agent.id),
                    agent: agent.map(|agent| agent.title.clone()),
                    name: if p.pid == root_pid {
                        "Choro app".into()
                    } else {
                        Self::process_display_name("", &command)
                    },
                    command: Self::process_command_summary(&command),
                }
            })
            .collect();
        processes.sort_by_key(|process| std::cmp::Reverse(process.memory_bytes));

        let mut snapshot = ProcessSnapshot {
            root_pid,
            app_bytes: root.resident_bytes,
            total_bytes,
            total_cpu: tree.iter().map(|p| p.cpu).sum(),
            process_count: tree.len(),
            processes,
            categories: Vec::new(),
            measurement_note:
                "Resident memory only. Activity Monitor's physical footprint was unavailable."
                    .into(),
            error: None,
        };

        if !include_footprint {
            return snapshot;
        }

        let footprint = Command::new("/usr/bin/footprint")
            .args([
                "-f",
                "bytes",
                "-p",
                &root_pid.to_string(),
                "-t",
                "-x",
                "footprint",
            ])
            .output();
        let Ok(footprint) = footprint else {
            return snapshot;
        };
        if !footprint.status.success() {
            return snapshot;
        }
        let Some(footprint) =
            Self::parse_footprint_output(&String::from_utf8_lossy(&footprint.stdout), root_pid)
        else {
            return snapshot;
        };

        let mut footprint_processes = footprint.processes;
        footprint_processes.sort_by_key(|process| std::cmp::Reverse(process.bytes));
        snapshot.app_bytes = footprint.app_bytes;
        snapshot.total_bytes = footprint.total_bytes;
        snapshot.process_count = footprint_processes.len();
        snapshot.total_cpu = footprint_processes
            .iter()
            .filter_map(|process| table.get(&process.pid))
            .map(|process| process.cpu)
            .sum();
        snapshot.processes = footprint_processes
            .into_iter()
            .map(|process| {
                let raw = table.get(&process.pid);
                let command = raw
                    .map(|raw| raw.command.clone())
                    .filter(|command| !command.is_empty())
                    .unwrap_or_else(|| process.name.clone());
                let agent_id = raw
                    .and_then(&resolve_agent_id)
                    .or_else(|| Self::infer_agent(&agents, None, &command).map(|agent| agent.id));
                let mut agent =
                    agent_id.and_then(|agent_id| agents.iter().find(|agent| agent.id == agent_id));
                let mut project = agent
                    .and_then(|agent| {
                        projects
                            .iter()
                            .find(|project| project.id == agent.project_id)
                    })
                    .or_else(|| Self::infer_project(projects, None, &command));
                if agent.is_none() && project.is_none() {
                    let cwd = Self::process_cwd(process.pid);
                    agent = Self::infer_agent(&agents, cwd.as_deref(), &command);
                    project = agent
                        .and_then(|agent| {
                            projects
                                .iter()
                                .find(|project| project.id == agent.project_id)
                        })
                        .or_else(|| Self::infer_project(projects, cwd.as_deref(), &command));
                }
                let project_id = agent
                    .map(|agent| agent.project_id)
                    .or_else(|| project.map(|project| project.id));
                let project_name = project.map(|project| project.name.clone()).or_else(|| {
                    agent.and_then(|agent| {
                        agent
                            .project_path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                    })
                });
                ProcessInfo {
                    pid: process.pid,
                    memory_bytes: process.bytes,
                    cpu: raw.map_or(0.0, |raw| raw.cpu),
                    project_id,
                    project: project_name,
                    agent_id: agent.map(|agent| agent.id),
                    agent: agent.map(|agent| agent.title.clone()),
                    name: Self::process_display_name(&process.name, &command),
                    command: Self::process_command_summary(&command),
                }
            })
            .collect();
        snapshot.categories =
            Self::summarize_memory_categories(footprint.categories, footprint.app_bytes);
        snapshot.measurement_note =
            "Physical footprint, matching Activity Monitor. Includes compressed and swapped memory."
                .into();
        snapshot
    }

    fn parse_footprint_output(output: &str, root_pid: i32) -> Option<FootprintSnapshot> {
        let mut processes = Vec::<FootprintProcess>::new();
        let mut categories = Vec::<MemoryCategory>::new();
        let mut current_pid = None;
        let mut root_categories_complete = false;
        let mut total_bytes = None;

        for line in output.lines() {
            let trimmed = line.trim();
            if let Some(process) = Self::parse_footprint_process_header(trimmed) {
                current_pid = Some(process.pid);
                processes.push(process);
                continue;
            }
            if let Some(bytes) = trimmed
                .strip_prefix("Summary Footprint: ")
                .and_then(Self::parse_byte_count)
            {
                total_bytes = Some(bytes);
                current_pid = None;
                continue;
            }
            if let Some(bytes) = trimmed
                .strip_prefix("phys_footprint: ")
                .and_then(Self::parse_byte_count)
            {
                if let Some(pid) = current_pid {
                    if let Some(process) = processes.iter_mut().rev().find(|entry| entry.pid == pid)
                    {
                        process.bytes = bytes;
                    }
                }
                continue;
            }
            if current_pid == Some(root_pid) && !root_categories_complete {
                if let Some(category) = Self::parse_footprint_category(trimmed) {
                    if category.name == "TOTAL" {
                        root_categories_complete = true;
                    } else if category.bytes > 0 {
                        categories.push(category);
                    }
                }
            }
        }

        let app_bytes = processes
            .iter()
            .find(|process| process.pid == root_pid)
            .map(|process| process.bytes)?;
        let monitor_bytes = processes
            .iter()
            .filter(|process| process.name == "footprint")
            .map(|process| process.bytes)
            .fold(0_u64, u64::saturating_add);
        processes.retain(|process| process.name != "footprint");
        let total_bytes = total_bytes
            .unwrap_or_else(|| {
                processes
                    .iter()
                    .map(|process| process.bytes)
                    .fold(0_u64, u64::saturating_add)
            })
            .saturating_sub(monitor_bytes);
        Some(FootprintSnapshot {
            app_bytes,
            total_bytes,
            processes,
            categories,
        })
    }

    fn parse_footprint_process_header(line: &str) -> Option<FootprintProcess> {
        let footprint = line.rfind("    Footprint: ")?;
        let identity = &line[..footprint];
        let open = identity.rfind(" [")?;
        let close = identity[open + 2..].find("]:")? + open + 2;
        let pid = identity[open + 2..close].parse::<i32>().ok()?;
        let name = identity[..open].trim().to_string();
        let bytes = Self::parse_byte_count(&line[footprint + "    Footprint: ".len()..])?;
        Some(FootprintProcess { pid, name, bytes })
    }

    fn parse_byte_count(value: &str) -> Option<u64> {
        value
            .split_whitespace()
            .next()
            .and_then(|bytes| bytes.parse::<u64>().ok())
    }

    fn parse_footprint_category(line: &str) -> Option<MemoryCategory> {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 8 || parts[1] != "B" || parts[3] != "B" || parts[5] != "B" {
            return None;
        }
        let dirty = parts[0].parse::<u64>().ok()?;
        parts[4].parse::<u64>().ok()?;
        let regions = parts[6].parse::<u64>().ok()?;
        Some(MemoryCategory {
            // Apple's physical footprint is the dirty column. Clean and
            // reclaimable pages are intentionally outside that pressure total.
            bytes: dirty,
            regions,
            name: parts[7..].join(" "),
        })
    }

    fn summarize_memory_categories(
        mut categories: Vec<MemoryCategory>,
        app_bytes: u64,
    ) -> Vec<MemoryCategory> {
        categories.sort_by_key(|category| std::cmp::Reverse(category.bytes));
        let visible_count = categories.len().min(7);
        let mut visible = categories.drain(..visible_count).collect::<Vec<_>>();
        let visible_bytes = visible
            .iter()
            .map(|category| category.bytes)
            .fold(0_u64, u64::saturating_add);
        let remaining_regions = categories
            .iter()
            .map(|category| category.regions)
            .sum::<u64>();
        let other_bytes = app_bytes.saturating_sub(visible_bytes);
        if other_bytes > 0 {
            visible.push(MemoryCategory {
                bytes: other_bytes,
                regions: remaining_regions,
                name: "Other allocations".into(),
            });
        }
        visible
    }

    pub(super) fn format_bytes(bytes: u64) -> String {
        const KB: f64 = 1024.0;
        const MB: f64 = KB * 1024.0;
        const GB: f64 = MB * 1024.0;
        let bytes = bytes as f64;
        if bytes >= GB {
            format!("{:.2} GB", bytes / GB)
        } else if bytes >= MB {
            format!("{:.0} MB", bytes / MB)
        } else if bytes >= KB {
            format!("{:.0} KB", bytes / KB)
        } else {
            format!("{bytes:.0} B")
        }
    }

    fn process_display_name(name: &str, command: &str) -> String {
        if name == "choro" || command.ends_with("/Contents/MacOS/choro") {
            "Choro app".into()
        } else if name == "com.apple.WebKit.WebContent" {
            "Web page content".into()
        } else if name == "com.apple.WebKit.GPU" {
            "Web graphics".into()
        } else if name == "com.apple.WebKit.Networking" {
            "Web networking".into()
        } else if name == "com.apple.SafariPlatformSupport.Helper" {
            "Choro graphics and media".into()
        } else if command.contains("claude_bridge.mjs") {
            "Claude bridge".into()
        } else if command.contains("choro-mcp") {
            "Choro MCP".into()
        } else if !name.is_empty() {
            name.into()
        } else {
            command
                .split_whitespace()
                .next()
                .and_then(|path| path.rsplit('/').next())
                .filter(|name| !name.is_empty())
                .unwrap_or("Process")
                .into()
        }
    }

    fn process_command_summary(command: &str) -> String {
        let redacted = if let Some(index) = command.find("--mcp-config") {
            format!("{}--mcp-config <redacted>", &command[..index])
        } else {
            command.to_string()
        };
        let Some((end, _)) = redacted.char_indices().nth(240) else {
            return redacted;
        };
        format!("{}…", &redacted[..end])
    }

    pub(super) fn memory_category_name(name: &str) -> String {
        match name {
            "untagged (VM_ALLOCATE)" => "Untagged VM allocations".into(),
            "MALLOC_LARGE" => "Large heap allocations".into(),
            "MALLOC_SMALL" => "Small heap allocations".into(),
            "MALLOC_TINY" => "Tiny heap allocations".into(),
            "Owned physical footprint (unmapped) (graphics)" => "Graphics memory".into(),
            "Owned physical footprint (unmapped)" => "Owned app memory".into(),
            "IOAccelerator (graphics)" => "GPU accelerator memory".into(),
            "IOSurface" => "Image surfaces".into(),
            "page table" => "Page tables".into(),
            "stack" => "Thread stacks".into(),
            "WebKit malloc" => "WebKit allocations".into(),
            _ => name.into(),
        }
    }

    fn process_cwd(pid: i32) -> Option<String> {
        let output = Command::new("lsof")
            .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix('n').map(|path| path.to_string()))
    }

    fn infer_agent<'a>(
        agents: &'a [AgentRecord],
        cwd: Option<&str>,
        command: &str,
    ) -> Option<&'a AgentRecord> {
        agents
            .iter()
            .filter(|agent| {
                let agent_id = agent.id.to_string();
                if command.contains(&agent_id) || cwd.is_some_and(|cwd| cwd.contains(&agent_id)) {
                    return true;
                }
                agent.lane_path.as_ref().is_some_and(|lane_path| {
                    let lane_path = lane_path.to_string_lossy();
                    command.contains(lane_path.as_ref())
                        || cwd.is_some_and(|cwd| cwd.starts_with(lane_path.as_ref()))
                })
            })
            .max_by_key(|agent| {
                agent
                    .lane_path
                    .as_ref()
                    .map_or(36, |lane_path| lane_path.as_os_str().len())
            })
    }

    fn infer_project<'a>(
        projects: &'a [ProjectSource],
        cwd: Option<&str>,
        command: &str,
    ) -> Option<&'a ProjectSource> {
        projects
            .iter()
            .filter(|project| {
                cwd.is_some_and(|cwd| cwd.starts_with(&project.path))
                    || command.contains(&project.path)
            })
            .max_by_key(|project| project.path.len())
    }

    pub(super) fn group_processes(processes: &[ProcessInfo]) -> Vec<ProcessProjectGroup> {
        let mut projects = Vec::<ProcessProjectGroup>::new();

        for process in processes {
            let project_index = projects
                .iter()
                .position(|group| group.project_id == process.project_id)
                .unwrap_or_else(|| {
                    projects.push(ProcessProjectGroup {
                        project_id: process.project_id,
                        name: process
                            .project
                            .clone()
                            .unwrap_or_else(|| "Choro & system helpers".into()),
                        memory_bytes: 0,
                        cpu: 0.0,
                        process_count: 0,
                        agents: Vec::new(),
                    });
                    projects.len() - 1
                });
            let project = &mut projects[project_index];
            project.memory_bytes = project.memory_bytes.saturating_add(process.memory_bytes);
            project.cpu += process.cpu;
            project.process_count += 1;

            let agent_index = project
                .agents
                .iter()
                .position(|group| group.agent_id == process.agent_id)
                .unwrap_or_else(|| {
                    project.agents.push(ProcessAgentGroup {
                        agent_id: process.agent_id,
                        name: process.agent.clone().unwrap_or_else(|| {
                            if process.project_id.is_some() {
                                "Project tools & terminals".into()
                            } else {
                                "Core app & macOS helpers".into()
                            }
                        }),
                        memory_bytes: 0,
                        cpu: 0.0,
                        processes: Vec::new(),
                    });
                    project.agents.len() - 1
                });
            let agent = &mut project.agents[agent_index];
            agent.memory_bytes = agent.memory_bytes.saturating_add(process.memory_bytes);
            agent.cpu += process.cpu;
            agent.processes.push(process.clone());
        }

        for project in &mut projects {
            for agent in &mut project.agents {
                agent
                    .processes
                    .sort_by_key(|process| std::cmp::Reverse(process.memory_bytes));
            }
            project
                .agents
                .sort_by_key(|agent| std::cmp::Reverse(agent.memory_bytes));
        }
        projects.sort_by_key(|project| std::cmp::Reverse(project.memory_bytes));
        projects
    }

    pub(super) fn refresh_process_snapshot(&mut self, cx: &mut Context<Self>) {
        self.process_loading = true;
        self.process_seq = self.process_seq.wrapping_add(1);
        let seq = self.process_seq;
        let projects = self.projects.clone();

        cx.spawn(async move |this, cx| {
            let snapshot = cx
                .background_executor()
                .spawn(async move { Self::load_process_snapshot(&projects) })
                .await;
            this.update(cx, |this, cx| {
                if this.process_seq == seq {
                    this.process_snapshot = snapshot;
                    this.process_loading = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

#[cfg(test)]
mod process_monitor_tests {
    use super::{ProcessInfo, SettingsView};
    use ide_core::ProjectId;
    use uuid::Uuid;

    #[test]
    fn parses_physical_footprint_processes_and_root_categories() {
        let output = r#"
choro [72652]: 64-bit    Footprint: 18720848368 B (16384 bytes per page)

      Dirty         Clean   Reclaimable    Regions    Category
17756667904 B           0 B           0 B     160570    untagged (VM_ALLOCATE)
  502120448 B           0 B           0 B         94    MALLOC_LARGE
18720848368 B    65110016 B      884736 B     169603    TOTAL

Auxiliary data:
    phys_footprint: 18720897520 B

com.apple.WebKit.WebContent [73203]: 64-bit    Footprint: 1439304296 B (16384 bytes per page)

Auxiliary data:
    phys_footprint: 1439337064 B

footprint [90000]: 64-bit    Footprint: 100 B (16384 bytes per page)

Auxiliary data:
    phys_footprint: 100 B

Summary Footprint: 20160234740 B
"#;

        let snapshot = SettingsView::parse_footprint_output(output, 72652).unwrap();
        assert_eq!(snapshot.app_bytes, 18_720_897_520);
        assert_eq!(snapshot.total_bytes, 20_160_234_640);
        assert_eq!(snapshot.processes.len(), 2);
        assert_eq!(snapshot.processes[1].pid, 73203);
        assert_eq!(snapshot.processes[1].bytes, 1_439_337_064);
        assert_eq!(snapshot.categories.len(), 2);
        assert_eq!(snapshot.categories[0].name, "untagged (VM_ALLOCATE)");
        assert_eq!(snapshot.categories[0].regions, 160_570);
    }

    #[test]
    fn category_summary_reconciles_to_the_app_footprint() {
        let mut categories = (1_u64..=10)
            .map(|index| super::MemoryCategory {
                bytes: index * 100,
                regions: index,
                name: format!("Category {index}"),
            })
            .collect::<Vec<_>>();
        categories.reverse();

        let summary = SettingsView::summarize_memory_categories(categories, 6_000);
        assert_eq!(summary.len(), 8);
        assert_eq!(summary.last().unwrap().name, "Other allocations");
        assert_eq!(
            summary.iter().map(|category| category.bytes).sum::<u64>(),
            6_000
        );
    }

    #[test]
    fn process_commands_do_not_expose_mcp_credentials() {
        let command = "claude --verbose --mcp-config {\"url\":\"https://example.test?userToken=secret\"} --permission-mode bypassPermissions";
        let summary = SettingsView::process_command_summary(command);
        assert_eq!(summary, "claude --verbose --mcp-config <redacted>");
        assert!(!summary.contains("secret"));
    }

    #[test]
    fn groups_processes_by_project_then_agent_and_sorts_by_memory() {
        let project_a = ProjectId(Uuid::new_v4());
        let project_b = ProjectId(Uuid::new_v4());
        let agent_a = Uuid::new_v4();
        let process = |pid: i32,
                       memory_bytes: u64,
                       project_id: Option<ProjectId>,
                       project: Option<&str>,
                       agent_id: Option<Uuid>,
                       agent: Option<&str>| ProcessInfo {
            pid,
            memory_bytes,
            cpu: memory_bytes as f64 / 100.0,
            project_id,
            project: project.map(str::to_string),
            agent_id,
            agent: agent.map(str::to_string),
            name: format!("process-{pid}"),
            command: format!("command-{pid}"),
        };
        let processes = vec![
            process(
                1,
                100,
                Some(project_a),
                Some("Choro"),
                Some(agent_a),
                Some("Build notifications"),
            ),
            process(
                2,
                50,
                Some(project_a),
                Some("Choro"),
                Some(agent_a),
                Some("Build notifications"),
            ),
            process(3, 20, Some(project_a), Some("Choro"), None, None),
            process(4, 500, Some(project_b), Some("Website"), None, None),
            process(5, 10, None, None, None, None),
        ];

        let groups = SettingsView::group_processes(&processes);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].name, "Website");
        assert_eq!(groups[0].memory_bytes, 500);
        assert_eq!(groups[1].name, "Choro");
        assert_eq!(groups[1].memory_bytes, 170);
        assert_eq!(groups[1].process_count, 3);
        assert_eq!(groups[1].agents.len(), 2);
        assert_eq!(groups[1].agents[0].name, "Build notifications");
        assert_eq!(groups[1].agents[0].memory_bytes, 150);
        assert_eq!(groups[1].agents[0].processes[0].pid, 1);
        assert_eq!(groups[2].name, "Choro & system helpers");
    }
}
