use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
    pub check_interval_secs: u64,
    pub retention_days: i64,
    pub heartbeat_secs: i64,
    pub username: String,
    pub password: String,
    pub config_path: Option<PathBuf>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct FileConfig {
    bind: Option<String>,
    db_path: Option<String>,
    data_dir: Option<String>,
    check_interval_secs: Option<u64>,
    retention_days: Option<i64>,
    heartbeat_secs: Option<i64>,
    username: Option<String>,
    password: Option<String>,
    tls_cert: Option<String>,
    tls_key: Option<String>,
}

impl Config {
    pub fn from_env_and_args() -> Self {
        let mut config_path = env::var("NRD_CONFIG").ok().map(PathBuf::from);

        // First pass: find --config
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--config" {
                if let Some(v) = args.next() {
                    config_path = Some(PathBuf::from(v));
                }
            }
        }

        let file_cfg = config_path
            .as_deref()
            .and_then(|p| load_file_config(p).ok())
            .or_else(|| load_file_config(&default_config_path()).ok())
            .unwrap_or_default();

        if config_path.is_none() && default_config_path().exists() {
            config_path = Some(default_config_path());
        }

        let mut bind = env::var("NRD_BIND")
            .ok()
            .or(file_cfg.bind)
            .unwrap_or_else(|| "0.0.0.0:8443".to_string());
        let mut db_path = env::var("NRD_DB")
            .ok()
            .map(PathBuf::from)
            .or(file_cfg.db_path.map(PathBuf::from))
            .unwrap_or_else(default_db_path);
        let mut data_dir = env::var("NRD_DATA_DIR")
            .ok()
            .map(PathBuf::from)
            .or(file_cfg.data_dir.map(PathBuf::from))
            .unwrap_or_else(default_data_dir);
        let mut check_interval_secs: u64 = env::var("NRD_CHECK_INTERVAL")
            .ok()
            .and_then(|v| v.parse().ok())
            .or(file_cfg.check_interval_secs)
            .unwrap_or(30);
        let retention_days: i64 = file_cfg.retention_days.unwrap_or(30).clamp(1, 3650);
        let mut heartbeat_secs: i64 = env::var("NRD_HEARTBEAT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .or(file_cfg.heartbeat_secs)
            .unwrap_or(3600);
        let mut username = env::var("NRD_USER")
            .ok()
            .or(file_cfg.username)
            .unwrap_or_else(|| "admin".to_string());
        let mut password = env::var("NRD_PASS")
            .ok()
            .or(file_cfg.password)
            .unwrap_or_else(|| "Gestion!".to_string());
        let mut tls_cert = env::var("NRD_TLS_CERT")
            .ok()
            .map(PathBuf::from)
            .or(file_cfg.tls_cert.map(PathBuf::from));
        let mut tls_key = env::var("NRD_TLS_KEY")
            .ok()
            .map(PathBuf::from)
            .or(file_cfg.tls_key.map(PathBuf::from));

        // Second pass: CLI overrides
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--bind" => {
                    if let Some(v) = args.next() {
                        bind = v;
                    }
                }
                "--db" => {
                    if let Some(v) = args.next() {
                        db_path = PathBuf::from(v);
                    }
                }
                "--data-dir" => {
                    if let Some(v) = args.next() {
                        data_dir = PathBuf::from(v);
                    }
                }
                "--interval" => {
                    if let Some(v) = args.next() {
                        if let Ok(n) = v.parse() {
                            check_interval_secs = n;
                        }
                    }
                }
                "--heartbeat" => {
                    if let Some(v) = args.next() {
                        if let Ok(n) = v.parse() {
                            heartbeat_secs = n;
                        }
                    }
                }
                "--tls-cert" => {
                    if let Some(v) = args.next() {
                        tls_cert = Some(PathBuf::from(v));
                    }
                }
                "--tls-key" => {
                    if let Some(v) = args.next() {
                        tls_key = Some(PathBuf::from(v));
                    }
                }
                "--config" => {
                    let _ = args.next();
                }
                _ => {}
            }
        }

        if check_interval_secs < 5 {
            check_interval_secs = 5;
        }
        if heartbeat_secs < 60 {
            heartbeat_secs = 60;
        }
        if username.is_empty() {
            username = "admin".to_string();
        }
        if password.is_empty() {
            password = "Gestion!".to_string();
        }

        Self {
            bind,
            db_path,
            data_dir,
            check_interval_secs,
            retention_days,
            heartbeat_secs,
            username,
            password,
            config_path,
            tls_cert,
            tls_key,
        }
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        let cfg = FileConfig {
            bind: Some(self.bind.clone()),
            db_path: Some(self.db_path.to_string_lossy().to_string()),
            data_dir: Some(self.data_dir.to_string_lossy().to_string()),
            check_interval_secs: Some(self.check_interval_secs),
            retention_days: Some(self.retention_days),
            heartbeat_secs: Some(self.heartbeat_secs),
            username: Some(self.username.clone()),
            password: Some(self.password.clone()),
            tls_cert: self
                .tls_cert
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            tls_key: self
                .tls_key
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
        }
        let data = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
        fs::write(path, data).map_err(|e| format!("write config {}: {e}", path.display()))?;
        set_private_permissions(path);
        Ok(())
    }
}

fn load_file_config(path: &Path) -> Result<FileConfig, String> {
    let data = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

fn default_db_path() -> PathBuf {
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("nanoremotedesktop.db");
        }
    }
    PathBuf::from("nanoremotedesktop.db")
}

fn default_data_dir() -> PathBuf {
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("data");
        }
    }
    PathBuf::from("data")
}

pub fn default_config_path() -> PathBuf {
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("nanoremotedesktop.json");
        }
    }
    PathBuf::from("nanoremotedesktop.json")
}

fn set_private_permissions(_path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(_path, fs::Permissions::from_mode(0o600));
    }
}
