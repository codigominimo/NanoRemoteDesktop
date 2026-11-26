use rusqlite::{Connection, OptionalExtension, Result, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedService {
    pub id: i64,
    pub name: String,
    pub label: String,
    pub enabled: bool,
    pub install_dir: Option<String>,
    pub executable: Option<String>,
    pub install_cmd: Option<String>,
    pub deploy_version: i64,
    pub last_deployed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceCheck {
    pub id: i64,
    pub service_id: i64,
    pub status: String,
    pub detail: Option<String>,
    pub checked_at: i64,
}

fn status_to_code(status: &str) -> i64 {
    match status.trim().to_lowercase().as_str() {
        "running" | "active" => 1,
        "stopped" | "inactive" | "dead" => 2,
        "failed" => 3,
        "starting" | "activating" => 4,
        "stopping" | "deactivating" => 5,
        "paused" => 6,
        "unknown" => 0,
        _ => 0,
    }
}

fn status_from_code(code: i64) -> &'static str {
    match code {
        1 => "running",
        2 => "stopped",
        3 => "failed",
        4 => "starting",
        5 => "stopping",
        6 => "paused",
        _ => "unknown",
    }
}

fn normalize_detail(detail: Option<&str>) -> Option<String> {
    let d = detail?.trim();
    if d.is_empty() {
        return None;
    }
    let mut s = d.to_string();
    if s.len() > 512 {
        // truncate on char boundary conservatively
        let mut end = 512;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push_str("...");
    }
    Some(s)
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceWithStatus {
    #[serde(flatten)]
    pub service: ManagedService,
    pub last_status: Option<String>,
    pub last_detail: Option<String>,
    pub last_checked_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct NewService {
    pub name: String,
    pub label: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConnection {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub address: String,
    pub username: Option<String>,
    pub notes: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct NewRemoteConnection {
    pub name: String,
    pub kind: Option<String>,
    pub address: String,
    pub username: Option<String>,
    pub notes: Option<String>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;",
        )?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init()?;
        Ok(db)
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("db mutex poisoned")
    }

    fn init(&self) -> Result<()> {
        let conn = self.lock();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA temp_store=FILE;
             PRAGMA journal_size_limit=1048576;
             PRAGMA foreign_keys=ON;",
        )?;

        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS managed_services (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                install_dir TEXT,
                executable TEXT,
                install_cmd TEXT,
                deploy_version INTEGER NOT NULL DEFAULT 0,
                last_deployed_at INTEGER
            );
            CREATE TABLE IF NOT EXISTS remote_connections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'rustdesk',
                address TEXT NOT NULL,
                username TEXT,
                notes TEXT,
                created_at INTEGER NOT NULL
            );",
        )?;

        // migrations for existing DBs
        let _ = conn.execute(
            "ALTER TABLE managed_services ADD COLUMN install_dir TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE managed_services ADD COLUMN executable TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE managed_services ADD COLUMN install_cmd TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE managed_services ADD COLUMN deploy_version INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE managed_services ADD COLUMN last_deployed_at INTEGER",
            [],
        );

        let checks_exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='service_checks'",
                [],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);

        if !checks_exists {
            conn.execute_batch(
                "PRAGMA auto_vacuum=INCREMENTAL;
                 CREATE TABLE service_checks (
                    id INTEGER PRIMARY KEY,
                    service_id INTEGER NOT NULL REFERENCES managed_services(id) ON DELETE CASCADE,
                    status INTEGER NOT NULL,
                    detail TEXT,
                    checked_at INTEGER NOT NULL
                 );
                 CREATE INDEX idx_service_checks_service_time
                    ON service_checks(service_id, checked_at DESC, id DESC);",
            )?;
        } else if user_version < 1 {
            self.migrate_checks_to_v1(&conn)?;
        } else {
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_service_checks_service_time
                    ON service_checks(service_id, checked_at DESC, id DESC);",
            )?;
        }

        conn.execute_batch("PRAGMA user_version=1; PRAGMA optimize;")?;
        Ok(())
    }

    fn migrate_checks_to_v1(&self, conn: &Connection) -> Result<()> {
        conn.execute_batch("PRAGMA foreign_keys=OFF;")?;
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(
            "CREATE TABLE service_checks_new (
                id INTEGER PRIMARY KEY,
                service_id INTEGER NOT NULL REFERENCES managed_services(id) ON DELETE CASCADE,
                status INTEGER NOT NULL,
                detail TEXT,
                checked_at INTEGER NOT NULL
             );
             INSERT INTO service_checks_new (id, service_id, status, detail, checked_at)
             SELECT id, service_id,
                CASE lower(trim(COALESCE(status,'')))
                    WHEN 'running' THEN 1 WHEN 'active' THEN 1
                    WHEN 'stopped' THEN 2 WHEN 'inactive' THEN 2 WHEN 'dead' THEN 2
                    WHEN 'failed' THEN 3
                    WHEN 'starting' THEN 4 WHEN 'activating' THEN 4
                    WHEN 'stopping' THEN 5 WHEN 'deactivating' THEN 5
                    WHEN 'paused' THEN 6
                    WHEN 'unknown' THEN 0
                    ELSE CASE WHEN typeof(status)='integer' THEN status ELSE 0 END
                END,
                CASE
                    WHEN detail IS NULL OR trim(detail)='' THEN NULL
                    WHEN length(detail) > 512 THEN substr(trim(detail),1,512) || '...'
                    ELSE trim(detail)
                END,
                checked_at
             FROM service_checks;
             DROP TABLE service_checks;
             ALTER TABLE service_checks_new RENAME TO service_checks;
             CREATE INDEX idx_service_checks_service_time
                ON service_checks(service_id, checked_at DESC, id DESC);",
        )?;
        tx.commit()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let _ = conn.execute_batch("PRAGMA auto_vacuum=INCREMENTAL; VACUUM;");
        Ok(())
    }

    fn row_to_service(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManagedService> {
        Ok(ManagedService {
            id: row.get(0)?,
            name: row.get(1)?,
            label: row.get(2)?,
            enabled: row.get::<_, i64>(3)? != 0,
            install_dir: row.get(4)?,
            executable: row.get(5)?,
            install_cmd: row.get(6)?,
            deploy_version: row.get(7)?,
            last_deployed_at: row.get(8)?,
        })
    }

    const SERVICE_COLS: &'static str = "id, name, label, enabled, install_dir, executable, install_cmd, deploy_version, last_deployed_at";

    pub fn list_services(&self) -> Result<Vec<ManagedService>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM managed_services ORDER BY label COLLATE NOCASE, name",
            Self::SERVICE_COLS
        ))?;
        let rows = stmt.query_map([], Self::row_to_service)?;
        rows.collect()
    }

    pub fn list_services_with_status(&self) -> Result<Vec<ServiceWithStatus>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT s.id, s.name, s.label, s.enabled, s.install_dir, s.executable, s.install_cmd, s.deploy_version, s.last_deployed_at,
                    c.status, c.detail, c.checked_at
             FROM managed_services s
             LEFT JOIN service_checks c ON c.id = (
                SELECT c2.id FROM service_checks c2
                WHERE c2.service_id = s.id
                ORDER BY c2.checked_at DESC, c2.id DESC
                LIMIT 1
             )
             ORDER BY s.label COLLATE NOCASE, s.name"
        ))?;
        let rows = stmt.query_map([], |row| {
            let status_code: Option<i64> = row.get(9)?;
            Ok(ServiceWithStatus {
                service: Self::row_to_service(row)?,
                last_status: status_code.map(status_from_code).map(str::to_string),
                last_detail: row.get(10)?,
                last_checked_at: row.get(11)?,
            })
        })?;
        rows.collect()
    }

    pub fn get_service(&self, id: i64) -> Result<Option<ManagedService>> {
        let conn = self.lock();
        conn.query_row(
            &format!(
                "SELECT {} FROM managed_services WHERE id = ?1",
                Self::SERVICE_COLS
            ),
            params![id],
            Self::row_to_service,
        )
        .optional()
    }

    pub fn add_service(&self, input: NewService) -> Result<ManagedService> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(rusqlite::Error::InvalidParameterName(
                "name cannot be empty".to_string(),
            ));
        }
        let label = input
            .label
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(name)
            .to_string();
        let enabled = input.enabled.unwrap_or(true);
        let conn = self.lock();
        conn.execute(
            "INSERT INTO managed_services (name, label, enabled) VALUES (?1, ?2, ?3)",
            params![name, label, enabled as i64],
        )?;
        let id = conn.last_insert_rowid();
        Ok(ManagedService {
            id,
            name: name.to_string(),
            label,
            enabled,
            install_dir: None,
            executable: None,
            install_cmd: None,
            deploy_version: 0,
            last_deployed_at: None,
        })
    }

    pub fn update_service(&self, id: i64, input: NewService) -> Result<Option<ManagedService>> {
        let Some(existing) = self.get_service(id)? else {
            return Ok(None);
        };
        let name = input.name.trim();
        let name = if name.is_empty() {
            &existing.name
        } else {
            name
        };
        let label = input
            .label
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&existing.label)
            .to_string();
        let enabled = input.enabled.unwrap_or(existing.enabled);
        let conn = self.lock();
        conn.execute(
            "UPDATE managed_services SET name = ?1, label = ?2, enabled = ?3 WHERE id = ?4",
            params![name, label, enabled as i64, id],
        )?;
        Ok(Some(ManagedService {
            id,
            name: name.to_string(),
            label,
            enabled,
            install_dir: existing.install_dir,
            executable: existing.executable,
            install_cmd: existing.install_cmd,
            deploy_version: existing.deploy_version,
            last_deployed_at: existing.last_deployed_at,
        }))
    }

    pub fn delete_service(&self, id: i64) -> Result<bool> {
        let conn = self.lock();
        let changed = conn.execute("DELETE FROM managed_services WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    pub fn set_deploy_info(
        &self,
        id: i64,
        install_dir: &str,
        executable: &str,
        install_cmd: Option<&str>,
        deploy_version: i64,
        last_deployed_at: i64,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE managed_services SET install_dir=?1, executable=?2, install_cmd=?3, deploy_version=?4, last_deployed_at=?5 WHERE id=?6",
            params![install_dir, executable, install_cmd, deploy_version, last_deployed_at, id],
        )?;
        Ok(())
    }

    pub fn record_check(
        &self,
        service_id: i64,
        status: &str,
        detail: Option<&str>,
        heartbeat_secs: i64,
    ) -> Result<bool> {
        let checked_at = now_epoch();
        let status_code = status_to_code(status);
        let detail_norm = normalize_detail(detail);
        let conn = self.lock();
        let last: Option<(i64, Option<String>, i64)> = conn
            .query_row(
                "SELECT status, detail, checked_at FROM service_checks
                 WHERE service_id = ?1
                 ORDER BY checked_at DESC, id DESC
                 LIMIT 1",
                params![service_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        let should_insert = match last {
            None => true,
            Some((last_status, last_detail, last_checked_at)) => {
                last_status != status_code
                    || last_detail != detail_norm
                    || checked_at.saturating_sub(last_checked_at) >= heartbeat_secs
            }
        };

        if should_insert {
            conn.execute(
                "INSERT INTO service_checks (service_id, status, detail, checked_at) VALUES (?1, ?2, ?3, ?4)",
                params![service_id, status_code, detail_norm, checked_at],
            )?;
        }
        Ok(should_insert)
    }

    pub fn recent_checks(&self, service_id: i64, limit: i64) -> Result<Vec<ServiceCheck>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, service_id, status, detail, checked_at
             FROM service_checks
             WHERE service_id = ?1
             ORDER BY checked_at DESC, id DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![service_id, limit], |row| {
            let status_code: i64 = row.get(2)?;
            Ok(ServiceCheck {
                id: row.get(0)?,
                service_id: row.get(1)?,
                status: status_from_code(status_code).to_string(),
                detail: row.get(3)?,
                checked_at: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn cleanup_old_checks(&self, retention_days: i64) -> Result<usize> {
        let cutoff = now_epoch() - retention_days.saturating_mul(24 * 60 * 60);
        let conn = self.lock();
        let changed = conn.execute(
            "DELETE FROM service_checks WHERE checked_at < ?1",
            params![cutoff],
        )?;
        // Reclaim pages without blocking vacuum; helps keep file small.
        let _ = conn.execute_batch("PRAGMA incremental_vacuum(200);");
        Ok(changed)
    }

    pub fn list_remote_connections(&self) -> Result<Vec<RemoteConnection>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, address, username, notes, created_at
             FROM remote_connections ORDER BY created_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(RemoteConnection {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                address: row.get(3)?,
                username: row.get(4)?,
                notes: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn add_remote_connection(&self, input: NewRemoteConnection) -> Result<RemoteConnection> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(rusqlite::Error::InvalidParameterName(
                "name is required".to_string(),
            ));
        }
        let address = input.address.trim();
        if address.is_empty() {
            return Err(rusqlite::Error::InvalidParameterName(
                "address is required".to_string(),
            ));
        }
        let kind = input
            .kind
            .as_deref()
            .unwrap_or("rustdesk")
            .trim()
            .to_lowercase();
        let kind = match kind.as_str() {
            "rustdesk" | "vnc" | "rdp" | "ssh" => kind,
            _ => "rustdesk".to_string(),
        };
        let username = input
            .username
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let notes = input
            .notes
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let created_at = now_epoch();
        let conn = self.lock();
        conn.execute(
            "INSERT INTO remote_connections (name, kind, address, username, notes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![name, kind, address, username, notes, created_at],
        )?;
        let id = conn.last_insert_rowid();
        Ok(RemoteConnection {
            id,
            name: name.to_string(),
            kind,
            address: address.to_string(),
            username: username.map(str::to_string),
            notes: notes.map(str::to_string),
            created_at,
        })
    }

    pub fn delete_remote_connection(&self, id: i64) -> Result<bool> {
        let conn = self.lock();
        let changed = conn.execute("DELETE FROM remote_connections WHERE id=?1", params![id])?;
        Ok(changed > 0)
    }
}

pub fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
