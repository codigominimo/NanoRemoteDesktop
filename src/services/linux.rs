use super::{ServiceManager, ServiceStatus, SystemService};
use std::process::Command;

pub struct LinuxServiceManager;

impl LinuxServiceManager {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let output = Command::new("systemctl")
            .args(args)
            .output()
            .map_err(|e| format!("failed to execute systemctl: {e}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if output.status.success() {
            Ok(stdout)
        } else if !stderr.is_empty() {
            Err(stderr)
        } else if !stdout.is_empty() {
            Err(stdout)
        } else {
            Err(format!("systemctl exited with {}", output.status))
        }
    }
}

impl ServiceManager for LinuxServiceManager {
    fn status(&self, name: &str) -> ServiceStatus {
        let output = Command::new("systemctl").args(["is-active", name]).output();
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
                };
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
                ServiceStatus {
                    status: status.to_string(),
                    detail,
                }
            }
            Err(e) => ServiceStatus {
                status: "unknown".to_string(),
                detail: Some(format!("failed to execute systemctl: {e}")),
            },
        }
    }

    fn start(&self, name: &str) -> Result<(), String> {
        self.run(&["start", name]).map(|_| ())
    }

    fn stop(&self, name: &str) -> Result<(), String> {
        self.run(&["stop", name]).map(|_| ())
    }

    fn restart(&self, name: &str) -> Result<(), String> {
        self.run(&["restart", name]).map(|_| ())
    }

    fn list_system_services(&self) -> Result<Vec<SystemService>, String> {
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
            // format: UNIT LOAD ACTIVE SUB DESCRIPTION...
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
        services.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(services)
    }
}
