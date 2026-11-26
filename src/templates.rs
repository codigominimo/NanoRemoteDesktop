use crate::db::ServiceWithStatus;
use askama::Template;

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate<'a> {
    pub username: &'a str,
    pub error: &'a str,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
pub struct DashboardTemplate {
    pub active_tab: &'static str,
    pub stats_html: String,
}

#[derive(Template)]
#[template(path = "services.html")]
pub struct ServicesTemplate<'a> {
    pub active_tab: &'static str,
    pub services_html: String,
    pub services_json: &'a str,
}

#[derive(Template)]
#[template(path = "system.html")]
pub struct SystemTemplate {
    pub active_tab: &'static str,
}

#[derive(Template)]
#[template(path = "deploy.html")]
pub struct DeployTemplate {
    pub active_tab: &'static str,
}

#[derive(Template)]
#[template(path = "remote.html")]
pub struct RemoteTemplate {
    pub active_tab: &'static str,
    pub desktop_available: bool,
    pub terminal_available: bool,
}

#[derive(Template)]
#[template(path = "stats_fragment.html")]
pub struct StatsTemplate {
    pub total: usize,
    pub running: usize,
    pub stopped: usize,
    pub failed: usize,
    pub other: usize,
}

pub struct ServiceRow {
    pub id: i64,
    pub name: String,
    pub name_js: String,
    pub label: String,
    pub enabled: bool,
    pub status: String,
    pub status_class: String,
    pub last_checked: String,
    pub last_detail: String,
    pub has_install: bool,
}

#[derive(Template)]
#[template(path = "services_fragment.html")]
pub struct ServicesFragmentTemplate {
    pub services: Vec<ServiceRow>,
}

pub fn status_class(status: &str) -> String {
    match status.to_lowercase().as_str() {
        "running" | "active" => "running",
        "stopped" | "inactive" | "dead" => "stopped",
        "failed" => "failed",
        "starting" | "activating" | "stopping" | "deactivating" => "starting",
        _ => "",
    }
    .to_string()
}

pub fn fmt_epoch(epoch: Option<i64>) -> String {
    let Some(ts) = epoch else {
        return "-".to_string();
    };
    // Simple UTC formatting without extra deps
    let secs = ts.max(0) as u64;
    let days = secs / 86400;
    let time = secs % 86400;
    let h = time / 3600;
    let m = (time % 3600) / 60;
    let s = time % 60;
    // days since 1970-01-01 -> YYYY-MM-DD (civil)
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    // Howard Hinnant civil_from_days
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as i64;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as i64;
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m, d)
}

fn js_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', " ")
        .replace('\r', " ")
}

pub fn build_service_rows(services: &[ServiceWithStatus]) -> Vec<ServiceRow> {
    services
        .iter()
        .map(|s| {
            let status = s
                .last_status
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            ServiceRow {
                id: s.service.id,
                name: s.service.name.clone(),
                name_js: js_escape(&s.service.name),
                label: s.service.label.clone(),
                enabled: s.service.enabled,
                status_class: status_class(&status),
                status,
                last_checked: fmt_epoch(s.last_checked_at),
                last_detail: s.last_detail.clone().unwrap_or_default(),
                has_install: s.service.install_dir.is_some(),
            }
        })
        .collect()
}

pub fn build_stats(services: &[ServiceWithStatus]) -> StatsTemplate {
    let total = services.len();
    let mut running = 0;
    let mut stopped = 0;
    let mut failed = 0;
    let mut other = 0;
    for s in services {
        match s
            .last_status
            .as_deref()
            .unwrap_or("unknown")
            .to_lowercase()
            .as_str()
        {
            "running" | "active" => running += 1,
            "stopped" | "inactive" | "dead" => stopped += 1,
            "failed" => failed += 1,
            _ => other += 1,
        }
    }
    StatsTemplate {
        total,
        running,
        stopped,
        failed,
        other,
    }
}
