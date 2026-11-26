use crate::auth::{Sessions, check_basic_auth, check_credentials, cookie_token};
use crate::config::Config;
use crate::db::{Db, NewRemoteConnection, NewService};
use crate::deploy::{self, DeployRequest};
use crate::monitor;
use crate::services::ServiceManager;
use crate::templates::{
    self, DashboardTemplate, DeployTemplate, LoginTemplate, RemoteTemplate,
    ServicesFragmentTemplate, ServicesTemplate, SystemTemplate,
};
use askama::Template;
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tiny_http::{Header, Method, Response, Server, StatusCode};

const SESSION_COOKIE: &str = "nrd_session";

#[derive(Deserialize)]
struct ActionReq {
    action: String,
}

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

pub fn serve(
    config: Config,
    db: Arc<Db>,
    manager: Arc<dyn ServiceManager>,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let server = build_server(&config)?;
    let scheme = if config.tls_cert.is_some() {
        "https"
    } else {
        "http"
    };
    eprintln!("NanoRemoteDesktop listening on {scheme}://{}", config.bind);

    let heartbeat_secs = config.heartbeat_secs;
    let username = config.username.clone();
    let password = config.password.clone();
    let config = Arc::new(config);
    let sessions = Arc::new(Sessions::new());

    while !stop.load(Ordering::Relaxed) {
        let Some(mut request) = server
            .recv_timeout(Duration::from_millis(500))
            .map_err(|e| e.to_string())?
        else {
            continue;
        };
        let url = request.url().to_string();
        let method = request.method().clone();
        let path = url.split('?').next().unwrap_or(&url).to_string();

        let auth_header = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .map(|h| h.value.as_str())
            .map(|s| s.to_string());
        let cookie_header = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Cookie"))
            .map(|h| h.value.as_str())
            .map(|s| s.to_string());

        let has_session = cookie_token(cookie_header.as_deref(), SESSION_COOKIE)
            .map(|t| sessions.verify(&t))
            .unwrap_or(false);
        let has_basic = check_basic_auth(auth_header.as_deref(), &username, &password);
        let authenticated = has_session || has_basic;

        // Public routes
        if method == Method::Get && (path == "/login" || path == "/login.html") {
            let resp = if authenticated {
                redirect_response("/")
            } else {
                render_login("", "")
            };
            let _ = request.respond(resp);
            continue;
        }
        if method == Method::Post && path == "/login" {
            let mut body = String::new();
            let _ = std::io::Read::read_to_string(request.as_reader(), &mut body);
            let form = parse_form(&body);
            let user = form.get("username").cloned().unwrap_or_default();
            let pass = form.get("password").cloned().unwrap_or_default();
            if !check_credentials(&user, &pass, &username, &password) {
                let _ = request.respond(render_login(&user, "Credenciales inválidas"));
                continue;
            }
            let token = sessions.create();
            let mut resp = redirect_response("/dashboard");
            resp.add_header(
                Header::from_bytes(
                    &b"Set-Cookie"[..],
                    format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/")
                        .as_bytes(),
                )
                .unwrap(),
            );
            let _ = request.respond(resp);
            continue;
        }
        if method == Method::Post && path == "/api/login" {
            let body: LoginReq = match read_json(&mut request) {
                Ok(b) => b,
                Err((code, msg)) => {
                    let _ =
                        request.respond(json_response(code, &serde_json::json!({"error": msg})));
                    continue;
                }
            };
            if !check_credentials(&body.username, &body.password, &username, &password) {
                let _ = request.respond(json_response(
                    401,
                    &serde_json::json!({"error":"invalid credentials"}),
                ));
                continue;
            }
            let token = sessions.create();
            let mut resp = json_response(200, &serde_json::json!({"ok": true}));
            resp.add_header(
                Header::from_bytes(
                    &b"Set-Cookie"[..],
                    format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/")
                        .as_bytes(),
                )
                .unwrap(),
            );
            let _ = request.respond(resp);
            continue;
        }
        if method == Method::Post && (path == "/api/logout" || path == "/logout") {
            if let Some(t) = cookie_token(cookie_header.as_deref(), SESSION_COOKIE) {
                sessions.revoke(&t);
            }
            let mut resp = if path == "/logout" {
                redirect_response("/login")
            } else {
                json_response(200, &serde_json::json!({"ok": true}))
            };
            resp.add_header(
                Header::from_bytes(
                    &b"Set-Cookie"[..],
                    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
                        .as_bytes(),
                )
                .unwrap(),
            );
            let _ = request.respond(resp);
            continue;
        }

        if !authenticated {
            let resp = if path.starts_with("/api/") {
                json_response(401, &serde_json::json!({"error":"unauthorized"}))
            } else {
                redirect_response("/login")
            };
            let _ = request.respond(resp);
            continue;
        }

        if method == Method::Get && path.starts_with("/assets/") {
            let resp = serve_static(&path);
            let _ = request.respond(resp);
            continue;
        }

        if method == Method::Get && path == "/ws/remote" {
            crate::remote::handle_ws_remote(request, &sessions, &config);
            continue;
        }
        if method == Method::Get && path == "/ws/terminal" {
            crate::remote::handle_ws_terminal(request, &sessions, &config);
            continue;
        }

        let result = handle(
            &method,
            &url,
            &mut request,
            &db,
            &manager,
            &config,
            heartbeat_secs,
        );
        let resp = match result {
            Ok(r) => r,
            Err((code, msg)) => json_response(code, &serde_json::json!({"error": msg})),
        };
        let _ = request.respond(resp);
    }
    Ok(())
}

fn handle(
    method: &Method,
    url: &str,
    request: &mut tiny_http::Request,
    db: &Arc<Db>,
    manager: &Arc<dyn ServiceManager>,
    config: &Arc<Config>,
    heartbeat_secs: i64,
) -> Result<Response<std::io::Cursor<Vec<u8>>>, (u16, String)> {
    let path = url.split('?').next().unwrap_or(url);

    if method == &Method::Get && (path == "/" || path == "/index.html") {
        return Ok(redirect_response("/dashboard"));
    }

    if method == &Method::Get && path == "/dashboard" {
        let services = db.list_services_with_status().map_err(internal)?;
        let stats_html = templates::build_stats(&services)
            .render()
            .map_err(|e| internal(e))?;
        let html = DashboardTemplate {
            active_tab: "dashboard",
            stats_html,
        }
        .render()
        .map_err(|e| internal(e))?;
        return Ok(html_response(&html));
    }

    if method == &Method::Get && path == "/services" {
        let services = db.list_services_with_status().map_err(internal)?;
        let rows = templates::build_service_rows(&services);
        let services_html = ServicesFragmentTemplate { services: rows }
            .render()
            .map_err(|e| internal(e))?;
        let services_json = serde_json::to_string(&services).unwrap_or_else(|_| "[]".to_string());
        let html = ServicesTemplate {
            active_tab: "services",
            services_html,
            services_json: &services_json,
        }
        .render()
        .map_err(|e| internal(e))?;
        return Ok(html_response(&html));
    }

    if method == &Method::Get && path == "/system" {
        let html = SystemTemplate {
            active_tab: "system",
        }
        .render()
        .map_err(|e| internal(e))?;
        return Ok(html_response(&html));
    }

    if method == &Method::Get && path == "/deploy" {
        let html = DeployTemplate {
            active_tab: "deploy",
        }
        .render()
        .map_err(|e| internal(e))?;
        return Ok(html_response(&html));
    }

    if method == &Method::Get && path == "/remote" {
        let desktop_available = cfg!(target_os = "windows");
        let html = RemoteTemplate {
            active_tab: "remote",
            desktop_available,
            terminal_available: cfg!(unix),
        }
        .render()
        .map_err(|e| internal(e))?;
        return Ok(html_response(&html));
    }

    if method == &Method::Get && path == "/fragments/services" {
        let services = db.list_services_with_status().map_err(internal)?;
        let rows = templates::build_service_rows(&services);
        let html = ServicesFragmentTemplate { services: rows }
            .render()
            .map_err(|e| internal(e))?;
        return Ok(html_response(&html));
    }

    if method == &Method::Get && path == "/fragments/stats" {
        let services = db.list_services_with_status().map_err(internal)?;
        let html = templates::build_stats(&services)
            .render()
            .map_err(|e| internal(e))?;
        return Ok(html_response(&html));
    }

    if method == &Method::Get && path == "/api/services" {
        let services = db.list_services_with_status().map_err(internal)?;
        return Ok(json_ok(&services));
    }

    if method == &Method::Get && path == "/api/system-services" {
        let mut services = manager.list_system_services().map_err(internal)?;
        let managed = db.list_services().map_err(internal)?;
        let managed_names: std::collections::HashSet<String> =
            managed.into_iter().map(|s| s.name.to_lowercase()).collect();
        let annotated: Vec<serde_json::Value> = services
            .drain(..)
            .map(|s| {
                let managed = managed_names.contains(&s.name.to_lowercase());
                serde_json::json!({
                    "name": s.name,
                    "label": s.label,
                    "status": s.status,
                    "managed": managed
                })
            })
            .collect();
        return Ok(json_ok(&annotated));
    }

    if method == &Method::Get && path == "/api/remote-connections" {
        let list = db.list_remote_connections().map_err(internal)?;
        return Ok(json_ok(&list));
    }
    if method == &Method::Post && path == "/api/remote-connections" {
        let input: NewRemoteConnection = read_json(request)?;
        let conn = db.add_remote_connection(input).map_err(bad_request)?;
        return Ok(json_response(201, &conn));
    }
    if let Some(id) = parse_id(path, "/api/remote-connections/", "") {
        if method == &Method::Delete {
            let deleted = db.delete_remote_connection(id).map_err(internal)?;
            if !deleted {
                return Err((404, "not found".to_string()));
            }
            return Ok(json_ok(&serde_json::json!({"ok": true})));
        }
    }

    if method == &Method::Post && path == "/api/deploy" {
        let req: DeployRequest = read_json(request)?;
        let res = deploy::deploy_new(config, db, req).map_err(bad_request)?;
        let _ = monitor::check_now(db, manager, res.service_id, heartbeat_secs);
        return Ok(json_response(
            201,
            &serde_json::json!({
                "ok": true,
                "service_id": res.service_id,
                "install_dir": res.install_dir,
                "install_output": res.install_output,
                "backup_dir": res.backup_dir,
                "version": res.version
            }),
        ));
    }

    if let Some(id) = parse_id(path, "/api/services/", "/redeploy") {
        if method == &Method::Post {
            let req: deploy::RedeployRequest = read_json(request)?;
            let res = deploy::redeploy(config, db, manager, id, req).map_err(bad_request)?;
            let _ = monitor::check_now(db, manager, res.service_id, heartbeat_secs);
            return Ok(json_ok(&serde_json::json!({
                "ok": true,
                "service_id": res.service_id,
                "install_dir": res.install_dir,
                "install_output": res.install_output,
                "backup_dir": res.backup_dir,
                "version": res.version
            })));
        }
    }

    if method == &Method::Post && path == "/api/services" {
        let input: NewService = read_json(request)?;
        let svc = db.add_service(input).map_err(bad_request)?;
        let _ = monitor::check_now(db, manager, svc.id, heartbeat_secs);
        return Ok(json_response(201, &svc));
    }

    if let Some(id) = parse_id(path, "/api/services/", "/action") {
        if method == &Method::Post {
            let body: ActionReq = read_json(request)?;
            let svc = db
                .get_service(id)
                .map_err(internal)?
                .ok_or((404, "not found".to_string()))?;
            let res = match body.action.as_str() {
                "start" => manager.start(&svc.name),
                "stop" => manager.stop(&svc.name),
                "restart" => manager.restart(&svc.name),
                "check" => {
                    let _ = monitor::check_now(db, manager, id, heartbeat_secs);
                    Ok(())
                }
                _ => return Err((400, "unknown action".to_string())),
            };
            if let Err(e) = res {
                return Err((500, e));
            }
            let _ = monitor::check_now(db, manager, id, heartbeat_secs);
            return Ok(json_ok(&serde_json::json!({"ok": true})));
        }
    }

    if let Some(id) = parse_id(path, "/api/services/", "/checks") {
        if method == &Method::Get {
            let limit = query_param(url, "limit")
                .and_then(|v| v.parse().ok())
                .unwrap_or(100);
            let checks = db.recent_checks(id, limit).map_err(internal)?;
            return Ok(json_ok(&checks));
        }
    }

    if let Some(id) = parse_id(path, "/api/services/", "") {
        if method == &Method::Put {
            let input: NewService = read_json(request)?;
            let svc = db
                .update_service(id, input)
                .map_err(bad_request)?
                .ok_or((404, "not found".to_string()))?;
            return Ok(json_ok(&svc));
        }
        if method == &Method::Delete {
            let deleted = db.delete_service(id).map_err(internal)?;
            if !deleted {
                return Err((404, "not found".to_string()));
            }
            return Ok(json_ok(&serde_json::json!({"ok": true})));
        }
        if method == &Method::Get {
            let svc = db
                .get_service(id)
                .map_err(internal)?
                .ok_or((404, "not found".to_string()))?;
            return Ok(json_ok(&svc));
        }
    }

    if method == &Method::Get && path == "/api/health" {
        return Ok(json_ok(&serde_json::json!({"ok": true})));
    }

    Err((404, "not found".to_string()))
}

fn parse_id(path: &str, prefix: &str, suffix: &str) -> Option<i64> {
    let rest = path.strip_prefix(prefix)?;
    let id_str = if suffix.is_empty() {
        if rest.contains('/') {
            return None;
        }
        rest
    } else {
        rest.strip_suffix(suffix)?
    };
    if id_str.is_empty() || id_str.contains('/') {
        return None;
    }
    id_str.parse().ok()
}

fn query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let q = url.split_once('?')?.1;
    for pair in q.split('&') {
        let mut kv = pair.splitn(2, '=');
        let k = kv.next()?;
        let v = kv.next().unwrap_or("");
        if k == key {
            return Some(v);
        }
    }
    None
}

fn read_json<T: serde::de::DeserializeOwned>(
    request: &mut tiny_http::Request,
) -> Result<T, (u16, String)> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|e| (400, e.to_string()))?;
    serde_json::from_str(&body).map_err(|e| (400, format!("invalid json: {e}")))
}

fn internal<E: std::fmt::Display>(e: E) -> (u16, String) {
    (500, e.to_string())
}

fn bad_request<E: std::fmt::Display>(e: E) -> (u16, String) {
    (400, e.to_string())
}

fn json_ok<T: serde::Serialize + ?Sized>(value: &T) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(200, value)
}

fn json_response<T: serde::Serialize + ?Sized>(
    status: u16,
    value: &T,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let body =
        serde_json::to_vec(value).unwrap_or_else(|_| b"{\"error\":\"serialization\"}".to_vec());
    let mut resp = Response::from_data(body).with_status_code(StatusCode(status));
    resp.add_header(
        Header::from_bytes(
            &b"Content-Type"[..],
            &b"application/json; charset=utf-8"[..],
        )
        .unwrap(),
    );
    resp
}

fn html_response(html: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut resp = Response::from_string(html).with_status_code(StatusCode(200));
    resp.add_header(
        Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap(),
    );
    resp
}

fn redirect_response(location: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut resp = Response::from_string("").with_status_code(StatusCode(302));
    resp.add_header(Header::from_bytes(&b"Location"[..], location.as_bytes()).unwrap());
    resp
}

fn render_login(username: &str, error: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let html = LoginTemplate { username, error }
        .render()
        .unwrap_or_else(|_| "<h1>Login error</h1>".to_string());
    let mut resp = Response::from_string(html).with_status_code(if error.is_empty() {
        StatusCode(200)
    } else {
        StatusCode(401)
    });
    resp.add_header(
        Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap(),
    );
    resp
}

fn build_server(config: &Config) -> Result<Server, String> {
    if let Some(cert_path) = &config.tls_cert {
        let key_path = config
            .tls_key
            .as_ref()
            .ok_or_else(|| "tls_key is required when tls_cert is set".to_string())?;
        let cert = std::fs::read(cert_path)
            .map_err(|e| format!("read tls cert {}: {e}", cert_path.display()))?;
        let key = std::fs::read(key_path)
            .map_err(|e| format!("read tls key {}: {e}", key_path.display()))?;
        return Server::https(
            &config.bind,
            tiny_http::SslConfig {
                certificate: cert,
                private_key: key,
            },
        )
        .map_err(|e| format!("bind https {}: {e}", config.bind));
    }

    let (cert_path, key_path) = ensure_self_signed_cert(&config.data_dir)?;
    let cert = std::fs::read(&cert_path)
        .map_err(|e| format!("read tls cert {}: {e}", cert_path.display()))?;
    let key = std::fs::read(&key_path)
        .map_err(|e| format!("read tls key {}: {e}", key_path.display()))?;
    Server::https(
        &config.bind,
        tiny_http::SslConfig {
            certificate: cert,
            private_key: key,
        },
    )
    .map_err(|e| format!("bind https {}: {e}", config.bind))
}

fn ensure_self_signed_cert(
    data_dir: &std::path::Path,
) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let tls_dir = data_dir.join("tls");
    std::fs::create_dir_all(&tls_dir).map_err(|e| format!("create tls dir: {e}"))?;
    let cert_path = tls_dir.join("cert.pem");
    let key_path = tls_dir.join("key.pem");
    if cert_path.exists() && key_path.exists() {
        return Ok((cert_path, key_path));
    }

    let mut params = rcgen::CertificateParams::new(vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ])
    .map_err(|e| format!("tls params: {e}"))?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "NanoRemoteDesktop");
    let key_pair = rcgen::KeyPair::generate().map_err(|e| format!("tls key: {e}"))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| format!("tls cert: {e}"))?;
    std::fs::write(&cert_path, cert.pem()).map_err(|e| format!("write cert: {e}"))?;
    std::fs::write(&key_path, key_pair.serialize_pem()).map_err(|e| format!("write key: {e}"))?;
    set_private_permissions(&key_path);
    Ok((cert_path, key_path))
}

fn set_private_permissions(_path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(_path, std::fs::Permissions::from_mode(0o600));
    }
}

fn parse_form(body: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for pair in body.split('&') {
        let mut kv = pair.splitn(2, '=');
        let k = kv.next().unwrap_or("");
        let v = kv.next().unwrap_or("");
        map.insert(url_decode(k), url_decode(v));
    }
    map
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex(bytes[i + 1]);
                let lo = hex(bytes[i + 2]);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi << 4) | lo);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn serve_static(path: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let (bytes, ct): (&[u8], &str) = match path {
        "/assets/xterm.min.js" => (include_bytes!("../assets/xterm.min.js"), "application/javascript; charset=utf-8"),
        "/assets/xterm.css" => (include_bytes!("../assets/xterm.css"), "text/css; charset=utf-8"),
        _ => return Response::from_string("not found").with_status_code(StatusCode(404)),
    };
    let mut resp = Response::from_data(bytes.to_vec()).with_status_code(StatusCode(200));
    resp.add_header(Header::from_bytes(&b"Content-Type"[..], ct.as_bytes()).unwrap());
    resp.add_header(Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=86400"[..]).unwrap());
    resp
}
