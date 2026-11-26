use super::{ServiceManager, ServiceStatus, SystemService};
use std::process::Command;

pub struct WindowsServiceManager;

impl WindowsServiceManager {
    fn run_sc(&self, args: &[&str]) -> Result<String, String> {
        let output = Command::new("sc.exe")
            .args(args)
            .output()
            .map_err(|e| format!("failed to execute sc.exe: {e}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if output.status.success() {
            Ok(stdout)
        } else if !stderr.is_empty() {
            Err(stderr)
        } else if !stdout.is_empty() {
            Err(stdout)
        } else {
            Err(format!("sc.exe exited with {}", output.status))
        }
    }

    fn parse_state(output: &str) -> String {
        let lower = output.to_lowercase();
        if lower.contains("running") {
            "running".to_string()
        } else if lower.contains("stopped") {
            "stopped".to_string()
        } else if lower.contains("paused") {
            "paused".to_string()
        } else if lower.contains("start_pending") {
            "starting".to_string()
        } else if lower.contains("stop_pending") {
            "stopping".to_string()
        } else {
            "unknown".to_string()
        }
    }
}

impl ServiceManager for WindowsServiceManager {
    fn status(&self, name: &str) -> ServiceStatus {
        match self.run_sc(&["query", name]) {
            Ok(out) => ServiceStatus {
                status: Self::parse_state(&out),
                detail: None,
            },
            Err(e) => ServiceStatus {
                status: "unknown".to_string(),
                detail: Some(e),
            },
        }
    }

    fn start(&self, name: &str) -> Result<(), String> {
        self.run_sc(&["start", name]).map(|_| ())
    }

    fn stop(&self, name: &str) -> Result<(), String> {
        self.run_sc(&["stop", name]).map(|_| ())
    }

    fn restart(&self, name: &str) -> Result<(), String> {
        let _ = self.stop(name);
        std::thread::sleep(std::time::Duration::from_millis(500));
        self.start(name).map(|_| ())
    }

    fn list_system_services(&self) -> Result<Vec<SystemService>, String> {
        let script = "[Console]::OutputEncoding=[Text.Encoding]::UTF8; $OutputEncoding=[Text.Encoding]::UTF8; \
            Get-Service | Select-Object Name,DisplayName,@{n='Status';e={$_.Status.ToString()}} | ConvertTo-Json -Compress";
        let out = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .map_err(|e| format!("failed to execute powershell: {e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if !out.status.success() {
            return Err(if !stderr.is_empty() { stderr } else { stdout });
        }
        if stdout.is_empty() {
            return Ok(vec![]);
        }
        let v: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| format!("parse services json: {e}; raw: {stdout}"))?;
        let arr = match v {
            serde_json::Value::Array(a) => a,
            serde_json::Value::Object(_) => vec![v],
            _ => return Ok(vec![]),
        };
        let mut services = Vec::new();
        for item in arr {
            let name = item
                .get("Name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if name.is_empty() {
                continue;
            }
            let label = item
                .get("DisplayName")
                .and_then(|v| v.as_str())
                .unwrap_or(name)
                .trim();
            let status_raw = item
                .get("Status")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let status = match status_raw.to_lowercase().as_str() {
                "running" => "running",
                "stopped" => "stopped",
                "paused" => "paused",
                "startpending" | "continuepending" => "starting",
                "stoppending" | "pausepending" => "stopping",
                _ => "unknown",
            }
            .to_string();
            services.push(SystemService {
                name: name.to_string(),
                label: if label.is_empty() {
                    name.to_string()
                } else {
                    label.to_string()
                },
                status,
            });
        }
        services.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(services)
    }
}
