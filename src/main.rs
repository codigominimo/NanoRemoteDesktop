mod app;
mod auth;
mod config;
mod db;
mod deploy;
mod monitor;
mod remote;
mod services;
mod sys_service;
mod templates;
mod web;
#[cfg(target_os = "windows")]
mod win_service;

use config::Config;
use std::sync::atomic::Ordering;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    let config = Config::from_env_and_args();

    if args.iter().any(|a| a == "--install-service") {
        if let Err(e) = sys_service::install(&config) {
            eprintln!("install service failed: {e}");
            std::process::exit(1);
        }
        println!("Service installed.");
        return;
    }

    if args.iter().any(|a| a == "--uninstall-service") {
        if let Err(e) = sys_service::uninstall(&config) {
            eprintln!("uninstall service failed: {e}");
            std::process::exit(1);
        }
        println!("Service uninstalled.");
        return;
    }

    #[cfg(target_os = "windows")]
    if args.iter().any(|a| a == "--service-run") {
        if let Err(e) = win_service::run() {
            eprintln!("windows service failed: {e}");
            std::process::exit(1);
        }
        return;
    }

    let stop = app::start(config);
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

fn print_help() {
    println!("NanoRemoteDesktop");
    println!();
    println!("Options:");
    println!("  --bind <addr>           Bind address (default: 0.0.0.0:8443, env NRD_BIND)");
    println!("  --db <path>             SQLite path (default: ./nanoremotedesktop.db, env NRD_DB)");
    println!(
        "  --interval <secs>       Check interval seconds (default: 30, env NRD_CHECK_INTERVAL)"
    );
    println!(
        "  --heartbeat <secs>      Min seconds between unchanged status snapshots (default: 3600)"
    );
    println!("  --config <path>         JSON config file (env NRD_CONFIG)");
    println!("  --tls-cert <path>       TLS cert PEM (env NRD_TLS_CERT)");
    println!("  --tls-key <path>        TLS key PEM (env NRD_TLS_KEY)");
    println!("  --install-service       Install as system service (Linux systemd / Windows SCM)");
    println!("  --uninstall-service     Uninstall system service");
    println!("  --service-run           Internal: run as Windows service");
    println!("  -h, --help              Show help");
    println!();
    println!("Env:");
    println!("  NRD_CONFIG              Config file path");
    println!("  NRD_TLS_CERT/NRD_TLS_KEY TLS PEM files; self-signed cert is generated if omitted");
    println!("  NRD_HEARTBEAT_SECS      Unchanged-status heartbeat seconds (default: 3600)");
    println!("  NRD_USER                Basic auth user (default: admin)");
    println!("  NRD_PASS                Basic auth password (default: Gestion!)");
}
