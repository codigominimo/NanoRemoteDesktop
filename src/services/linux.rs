use super::{ServiceManager, ServiceStatus, SystemService};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Copy)]
enum Backend {
    Systemd,
    Openrc,
}

pub struct LinuxServiceManager {
    backend: Backend,
}

impl LinuxServiceManager {
    pub fn new() -> Self {
        let backend = if command_exists("systemctl") && Path::new("/run/systemd/system").exists() {
            Backend::Systemd
        } else if command_exists("rc-service") || Path::new("/etc/init.d").is_dir() {
            Backend::Openrc
        } else if command_exists("systemctl") {
            Backend::Systemd
        } else {
            Backend::Openrc
        };
        Self { backend }
    }
}

impl ServiceManager for LinuxServiceManager {
    fn status(&self, name: &str) -> ServiceStatus {
        match self.backend {
            Backend::Systemd => systemd_status(name),
            Backend::Openrc => openrc_status(name),
        }
    }

    fn start(&self, name: &str) -> Result<(), String> {
        match self.backend {
            Backend::Systemd => run("systemctl", &["start", name]).map(|_| ()),
            Backend::Openrc => run("rc-service", &[name, "start"]).map(|_| ()),
        }
    }

    fn stop(&self, name: &str) -> Result<(), String> {
        match self.backend {
            Backend::Systemd => run("systemctl", &["stop", name]).map(|_| ()),
            Backend::Openrc => run("rc-service", &[name, "stop"]).map(|_| ()),
        }
    }

    fn restart(&self, name: &str) -> Result<(), String> {
        match self.backend {
            Backend::Systemd => run("systemctl", &["restart", name]).map(|_| ()),
            Backend::Openrc => run("rc-service", &[name, "restart"]).map(|_| ()),
        }
    }

    fn list_system_services(&self) -> Result<Vec<SystemService>, String> {
        match self.backend {
            Backend::Systemd => systemd_list(),
            Backend::Openrc => openrc_list(),
        }
    }
}

fn command_exists(command: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        fs::metadata(dir.join(command))
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("failed to execute {program}: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok(stdout)
    } else if !stderr.is_empty() {
        Err(stderr)
    } else if !stdout.is_empty() {
        Err(stdout)
    } else {
        Err(format!("{program} exited with {}", output.status))
    }
}

fn systemd_status(name: &str) -> ServiceStatus {
    let output = Command::new("systemctl")
        .args(["is-active", name])
        .output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
            let status = match stdout.as_str() {
                "active" => "running",
                "inactive" | "dead" => "stopped",
                "failed" => "failed",
                "activating" => "starting",
                "deactivating" => "stopping",
                other if !other.is_empty() => other,
                _ => "unknown",
            }
            .to_string();
            let detail = if out.status.success() {
                None
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                if stderr.is_empty() {
                    Some(stdout)
                } else {
                    Some(stderr)
                }
            };
            ServiceStatus { status, detail }
        }
        Err(e) => ServiceStatus {
            status: "unknown".to_string(),
            detail: Some(format!("failed to execute systemctl: {e}")),
        },
    }
}

fn systemd_list() -> Result<Vec<SystemService>, String> {
    let out = Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--all",
            "--no-pager",
            "--plain",
            "--no-legend",
        ])
        .output()
        .map_err(|e| format!("failed to execute systemctl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "systemctl list-units failed".to_string()
        } else {
            err
        });
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut services = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let unit = parts[0];
        if !unit.ends_with(".service") {
            continue;
        }
        let name = unit.trim_end_matches(".service").to_string();
        let active = parts[2];
        let sub = parts[3];
        let status = match (active, sub) {
            ("active", "running") => "running",
            ("active", _) => "active",
            ("inactive", _) | ("dead", _) => "stopped",
            ("failed", _) => "failed",
            _ => active,
        }
        .to_string();
        let desc = if parts.len() > 4 {
            parts[4..].join(" ")
        } else {
            name.clone()
        };
        services.push(SystemService {
            name,
            label: desc,
            status,
        });
    }
    sort_services(&mut services);
    Ok(services)
}

fn openrc_status(name: &str) -> ServiceStatus {
    let output = Command::new("rc-service")
        .args([name, "status"])
        .output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_lowercase();
            let text = format!("{stdout} {stderr}");
            let status = if text.contains("started") || text.contains("running") {
                "running"
            } else if text.contains("stopped") || text.contains("inactive") {
                "stopped"
            } else if text.contains("crashed") || text.contains("failed") {
                "failed"
            } else if out.status.success() {
                "running"
            } else {
                "unknown"
            }
            .to_string();
            let detail = if status == "unknown" {
                let detail = if stderr.is_empty() { stdout } else { stderr };
                if detail.is_empty() {
                    Some(format!("rc-service exited with {}", out.status))
                } else {
                    Some(detail)
                }
            } else {
                None
            };
            ServiceStatus { status, detail }
        }
        Err(e) => ServiceStatus {
            status: "unknown".to_string(),
            detail: Some(format!("failed to execute rc-service: {e}")),
        },
    }
}

fn openrc_list() -> Result<Vec<SystemService>, String> {
    let mut names = match run("rc-service", &["-l"]) {
        Ok(output) => service_names(&output),
        Err(_) => Vec::new(),
    };
    if names.is_empty() {
        names = init_d_names();
    }
    if names.is_empty() {
        return Err("no services found (rc-service or /etc/init.d)".to_string());
    }
    let mut services = Vec::new();
    for name in names {
        let status = openrc_status(&name).status;
        services.push(SystemService {
            label: openrc_description(&name).unwrap_or_else(|| name.clone()),
            name,
            status,
        });
    }
    sort_services(&mut services);
    Ok(services)
}

fn service_names(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn init_d_names() -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = fs::read_dir("/etc/init.d") else {
        return names;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with('.') || !path.is_file() {
            continue;
        }
        names.push(name.to_string());
    }
    names
}

fn openrc_description(name: &str) -> Option<String> {
    let content = fs::read_to_string(Path::new("/etc/init.d").join(name)).ok()?;
    for line in content.lines().take(40) {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("description=") {
            return Some(value.trim_matches(['"', '\'']).to_string());
        }
        if let Some(value) = line.strip_prefix("# description:") {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn sort_services(services: &mut [SystemService]) {
    services.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
}
