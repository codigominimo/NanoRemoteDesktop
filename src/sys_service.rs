use crate::config::Config;

#[cfg(target_os = "linux")]
pub fn install(config: &Config) -> Result<(), String> {
    use std::fs;
    use std::path::PathBuf;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_string_lossy();
    let config_path = config
        .config_path
        .clone()
        .unwrap_or_else(crate::config::default_config_path);
    config.save_to(&config_path)?;

    let unit = format!(
        "[Unit]\n\
Description=NanoRemoteDesktop Service Dashboard\n\
After=network.target\n\
\n\
[Service]\n\
Type=simple\n\
ExecStart={exe} --config {cfg}\n\
Restart=on-failure\n\
RestartSec=3\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        cfg = config_path.to_string_lossy()
    );

    let unit_path = PathBuf::from("/etc/systemd/system/nanoremotedesktop.service");
    fs::write(&unit_path, unit).map_err(|e| format!("write {}: {e}", unit_path.display()))?;

    run_cmd("systemctl", &["daemon-reload"])?;
    run_cmd(
        "systemctl",
        &["enable", "--now", "nanoremotedesktop.service"],
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn uninstall(_config: &Config) -> Result<(), String> {
    use std::fs;
    let unit_path = PathBuf::from("/etc/systemd/system/nanoremotedesktop.service");
    let _ = run_cmd(
        "systemctl",
        &["disable", "--now", "nanoremotedesktop.service"],
    );
    if unit_path.exists() {
        fs::remove_file(&unit_path).map_err(|e| format!("remove {}: {e}", unit_path.display()))?;
    }
    let _ = run_cmd("systemctl", &["daemon-reload"]);
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_cmd(cmd: &str, args: &[&str]) -> Result<(), String> {
    use std::process::Command;
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Err(if !stderr.is_empty() { stderr } else { stdout })
    }
}

#[cfg(target_os = "windows")]
pub fn install(config: &Config) -> Result<(), String> {
    use std::process::Command;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_string_lossy();

    let config_path = config
        .config_path
        .clone()
        .unwrap_or_else(crate::config::default_config_path);
    config.save_to(&config_path)?;

    let bin_path = format!(
        "\"{}\" --service-run --config \"{}\"",
        exe,
        config_path.to_string_lossy()
    );

    let out = Command::new("sc.exe")
        .args([
            "create",
            "NanoRemoteDesktop",
            "binPath=",
            &bin_path,
            "start=",
            "auto",
            "DisplayName=",
            "NanoRemoteDesktop",
        ])
        .output()
        .map_err(|e| format!("sc.exe create: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stdout).trim().to_string()
            + " "
            + String::from_utf8_lossy(&out.stderr).trim());
    }

    let _ = Command::new("sc.exe")
        .args([
            "description",
            "NanoRemoteDesktop",
            "NanoRemoteDesktop service dashboard",
        ])
        .output();

    let _ = Command::new("sc.exe")
        .args(["start", "NanoRemoteDesktop"])
        .output();
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn uninstall(_config: &Config) -> Result<(), String> {
    use std::process::Command;
    let _ = Command::new("sc.exe")
        .args(["stop", "NanoRemoteDesktop"])
        .output();
    std::thread::sleep(std::time::Duration::from_millis(1000));
    let out = Command::new("sc.exe")
        .args(["delete", "NanoRemoteDesktop"])
        .output()
        .map_err(|e| format!("sc.exe delete: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if !err.is_empty() { err } else { msg });
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn install(_config: &Config) -> Result<(), String> {
    Err("service installation not supported on this OS".to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn uninstall(_config: &Config) -> Result<(), String> {
    Err("service installation not supported on this OS".to_string())
}
