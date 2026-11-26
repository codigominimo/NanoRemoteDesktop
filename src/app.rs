use crate::config::Config;
use crate::db::Db;
use crate::monitor;
use crate::services::create_manager;
use crate::web;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

pub fn start(config: Config) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

    thread::spawn(move || {
        if let Err(e) = run(config, stop_clone.clone()) {
            eprintln!("app error: {e}");
            stop_clone.store(true, Ordering::Relaxed);
        }
    });

    stop
}

fn run(config: Config, stop: Arc<AtomicBool>) -> Result<(), String> {
    let db = Arc::new(
        Db::open(&config.db_path)
            .map_err(|e| format!("db open {}: {e}", config.db_path.display()))?,
    );
    let manager: Arc<dyn crate::services::ServiceManager> = Arc::from(create_manager());

    monitor::start_monitor(
        db.clone(),
        manager.clone(),
        config.check_interval_secs,
        config.retention_days,
        config.heartbeat_secs,
        stop.clone(),
    );

    web::serve(config, db, manager, stop)
}
