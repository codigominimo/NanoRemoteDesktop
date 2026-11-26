use crate::db::Db;
use crate::services::ServiceManager;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

pub fn start_monitor(
    db: Arc<Db>,
    manager: Arc<dyn ServiceManager>,
    interval_secs: u64,
    retention_days: i64,
    heartbeat_secs: i64,
    stop: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            if let Err(e) = run_once(&db, &manager, retention_days, heartbeat_secs) {
                eprintln!("monitor error: {e}");
            }
            for _ in 0..interval_secs {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
    });
}

fn run_once(
    db: &Db,
    manager: &Arc<dyn ServiceManager>,
    retention_days: i64,
    heartbeat_secs: i64,
) -> Result<(), String> {
    let services = db.list_services().map_err(|e| e.to_string())?;
    for svc in services.into_iter().filter(|s| s.enabled) {
        let st = manager.status(&svc.name);
        db.record_check(svc.id, &st.status, st.detail.as_deref(), heartbeat_secs)
            .map_err(|e| e.to_string())?;
    }
    db.cleanup_old_checks(retention_days)
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn check_now(
    db: &Db,
    manager: &Arc<dyn ServiceManager>,
    service_id: i64,
    heartbeat_secs: i64,
) -> Result<(), String> {
    let svc = db
        .get_service(service_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "service not found".to_string())?;
    let st = manager.status(&svc.name);
    db.record_check(svc.id, &st.status, st.detail.as_deref(), heartbeat_secs)
        .map_err(|e| e.to_string())?;
    Ok(())
}
