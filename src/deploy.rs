use crate::config::Config;
use crate::db::{self, Db, NewService};
use crate::services::ServiceManager;
use serde::Deserialize;
use std::fs;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct DeployRequest {
    pub name: String,
    pub label: Option<String>,
    pub executable: String,
    pub install_cmd: Option<String>,
    pub zip_base64: String,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct RedeployRequest {
    pub executable: Option<String>,
    pub install_cmd: Option<String>,
    pub zip_base64: String,
    pub label: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug)]
pub struct DeployResult {
    pub service_id: i64,
    pub install_dir: PathBuf,
    pub install_output: Option<String>,
    pub backup_dir: Option<PathBuf>,
    pub version: i64,
}

pub fn deploy_new(config: &Config, db: &Db, req: DeployRequest) -> Result<DeployResult, String> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err("name is required".to_string());
    }
    let executable = req.executable.trim();
    if executable.is_empty() {
        return Err("executable is required".to_string());
    }
    validate_rel_path(executable)?;

    let zip_bytes = decode_base64(&req.zip_base64).ok_or("invalid zip_base64")?;
    let safe_name = sanitize_name(name);
    let install_dir = config.data_dir.join("services").join(&safe_name);
    if install_dir.exists() {
        return Err(format!(
            "install dir already exists: {}. Use redeploy for existing service.",
            install_dir.display()
        ));
    }
    fs::create_dir_all(&install_dir).map_err(|e| format!("create install dir: {e}"))?;

    if let Err(e) = extract_zip(&zip_bytes, &install_dir) {
        let _ = fs::remove_dir_all(&install_dir);
        return Err(e);
    }

    let exec_path = install_dir.join(executable);
    if !exec_path.exists() {
        let _ = fs::remove_dir_all(&install_dir);
        return Err(format!(
            "executable not found after extract: {}",
            executable
        ));
    }

    let install_cmd = req
        .install_cmd
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let install_output = if let Some(cmd) = install_cmd {
        match run_install_command(cmd, &install_dir) {
            Ok(out) => Some(out),
            Err(e) => {
                let _ = fs::remove_dir_all(&install_dir);
                return Err(format!("install command failed: {e}"));
            }
        }
    } else {
        None
    };

    let svc = db
        .add_service(NewService {
            name: name.to_string(),
            label: req.label,
            enabled: req.enabled,
        })
        .map_err(|e| {
            let _ = fs::remove_dir_all(&install_dir);
            format!("db add service: {e}")
        })?;

    let now = db::now_epoch();
    db.set_deploy_info(
        svc.id,
        &install_dir.to_string_lossy(),
        executable,
        install_cmd,
        1,
        now,
    )
    .map_err(|e| format!("db set deploy info: {e}"))?;

    Ok(DeployResult {
        service_id: svc.id,
        install_dir,
        install_output,
        backup_dir: None,
        version: 1,
    })
}

pub fn redeploy(
    config: &Config,
    db: &Db,
    manager: &Arc<dyn ServiceManager>,
    service_id: i64,
    req: RedeployRequest,
) -> Result<DeployResult, String> {
    let svc = db
        .get_service(service_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "service not found".to_string())?;

    let install_dir = svc
        .install_dir
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| "service has no install_dir; deploy it first".to_string())?;

    let executable = req
        .executable
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(svc.executable.as_deref())
        .ok_or_else(|| "executable is required".to_string())?
        .to_string();
    validate_rel_path(&executable)?;

    let zip_bytes = decode_base64(&req.zip_base64).ok_or("invalid zip_base64")?;
    let install_cmd = req
        .install_cmd
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or(svc.install_cmd.clone());

    // Best-effort stop before replacing files
    let _ = manager.stop(&svc.name);

    let backup_dir = backup_install_dir(config, &svc.name, &install_dir)?;

    // Replace contents: extract into temp then swap
    let parent = install_dir
        .parent()
        .ok_or_else(|| "invalid install dir".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("create parent dir: {e}"))?;
    let tmp_dir = parent.join(format!(
        ".tmp-{}-{}",
        sanitize_name(&svc.name),
        db::now_epoch()
    ));
    if tmp_dir.exists() {
        let _ = fs::remove_dir_all(&tmp_dir);
    }
    fs::create_dir_all(&tmp_dir).map_err(|e| format!("create tmp dir: {e}"))?;

    if let Err(e) = extract_zip(&zip_bytes, &tmp_dir) {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(e);
    }
    if !tmp_dir.join(&executable).exists() {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(format!(
            "executable not found after extract: {}",
            executable
        ));
    }

    // swap: remove old install dir, rename tmp -> install dir
    if install_dir.exists() {
        fs::remove_dir_all(&install_dir).map_err(|e| {
            let _ = fs::remove_dir_all(&tmp_dir);
            format!("remove old install dir: {e}")
        })?;
    }
    fs::rename(&tmp_dir, &install_dir).map_err(|e| {
        let _ = fs::remove_dir_all(&tmp_dir);
        format!(
            "move new version into place: {e}. Backup at {}",
            backup_dir.display()
        )
    })?;

    let install_output = if let Some(cmd) = install_cmd.as_deref() {
        match run_install_command(cmd, &install_dir) {
            Ok(out) => Some(out),
            Err(e) => {
                return Err(format!(
                    "install command failed: {e}. Backup at {}",
                    backup_dir.display()
                ));
            }
        }
    } else {
        None
    };

    let version = svc.deploy_version + 1;
    let now = db::now_epoch();
    db.set_deploy_info(
        svc.id,
        &install_dir.to_string_lossy(),
        &executable,
        install_cmd.as_deref(),
        version,
        now,
    )
    .map_err(|e| e.to_string())?;

    // update label/enabled if provided
    if req.label.is_some() || req.enabled.is_some() {
        let _ = db.update_service(
            svc.id,
            NewService {
                name: svc.name.clone(),
                label: req.label.or(Some(svc.label.clone())),
                enabled: req.enabled.or(Some(svc.enabled)),
            },
        );
    }

    // Best-effort start after deploy
    let _ = manager.start(&svc.name);

    Ok(DeployResult {
        service_id: svc.id,
        install_dir,
        install_output,
        backup_dir: Some(backup_dir),
        version,
    })
}

fn backup_install_dir(config: &Config, name: &str, install_dir: &Path) -> Result<PathBuf, String> {
    if !install_dir.exists() {
        return Err(format!(
            "install dir does not exist: {}",
            install_dir.display()
        ));
    }
    let ts = db::now_epoch();
    let backup_root = config.data_dir.join("backups").join(sanitize_name(name));
    fs::create_dir_all(&backup_root).map_err(|e| format!("create backup dir: {e}"))?;
    let backup_dir = backup_root.join(format!("v-{ts}"));
    copy_dir_recursive(install_dir, &backup_dir)?;
    Ok(backup_dir)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("create backup dst: {e}"))?;
    for entry in fs::read_dir(src).map_err(|e| format!("read dir: {e}"))? {
        let entry = entry.map_err(|e| format!("read entry: {e}"))?;
        let ty = entry.file_type().map_err(|e| format!("file type: {e}"))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ty.is_file() {
            fs::copy(&src_path, &dst_path).map_err(|e| format!("copy file: {e}"))?;
        }
    }
    Ok(())
}

fn extract_zip(zip_bytes: &[u8], dest: &Path) -> Result<(), String> {
    let reader = Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| format!("invalid zip: {e}"))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| format!("zip entry: {e}"))?;
        let Some(enclosed) = file.enclosed_name() else {
            return Err("zip contains unsafe path".to_string());
        };
        validate_rel_path(&enclosed.to_string_lossy())?;
        let out_path = dest.join(enclosed);
        if file.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| format!("create dir: {e}"))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
            }
            let mut outfile =
                fs::File::create(&out_path).map_err(|e| format!("create file: {e}"))?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| format!("extract file: {e}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(mode));
                }
            }
        }
    }
    Ok(())
}

fn run_install_command(cmd: &str, dir: &Path) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    let output = Command::new("cmd")
        .args(["/C", cmd])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("run install command: {e}"))?;

    #[cfg(not(target_os = "windows"))]
    let output = Command::new("sh")
        .args(["-lc", cmd])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("run install command: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let combined = match (stdout.is_empty(), stderr.is_empty()) {
        (false, false) => format!("{stdout}\n{stderr}"),
        (false, true) => stdout,
        (true, false) => stderr,
        (true, true) => String::new(),
    };
    if output.status.success() {
        Ok(combined)
    } else {
        Err(if combined.is_empty() {
            format!("exit status {}", output.status)
        } else {
            combined
        })
    }
}

fn sanitize_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    out = out.trim_matches('.').to_string();
    if out.is_empty() {
        "service".to_string()
    } else {
        out
    }
}

fn validate_rel_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.is_absolute() {
        return Err("absolute paths are not allowed".to_string());
    }
    for c in p.components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err("invalid path component".to_string()),
        }
    }
    Ok(())
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let bytes: Vec<u8> = input.bytes().filter(|b| !b" \r\n\t".contains(b)).collect();
    if bytes.is_empty() {
        return Some(Vec::new());
    }
    fn val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c0 = *bytes.get(i)?;
        let c1 = *bytes.get(i + 1)?;
        if c0 == b'=' || c1 == b'=' {
            return None;
        }
        let v0 = val(c0)?;
        let v1 = val(c1)?;
        out.push((v0 << 2) | (v1 >> 4));
        let c2 = bytes.get(i + 2).copied().unwrap_or(b'=');
        let c3 = bytes.get(i + 3).copied().unwrap_or(b'=');
        if c2 != b'=' {
            let v2 = val(c2)?;
            out.push(((v1 & 0x0f) << 4) | (v2 >> 2));
            if c3 != b'=' {
                let v3 = val(c3)?;
                out.push(((v2 & 0x03) << 6) | v3);
            }
        } else if c3 != b'=' {
            return None;
        }
        i += 4;
    }
    Some(out)
}
