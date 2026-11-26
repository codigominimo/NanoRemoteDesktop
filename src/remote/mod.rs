#[cfg(target_os = "windows")]
mod capture_windows;
#[cfg(target_os = "windows")]
mod input_windows;
#[cfg(unix)]
mod terminal_unix;
mod ws;

use crate::auth::{Sessions, cookie_token};
use crate::config::Config;
use std::sync::Arc;
use tiny_http::{Header, Request, Response, StatusCode};

const SESSION_COOKIE: &str = "nrd_session";

pub fn handle_ws_remote(request: Request, sessions: &Arc<Sessions>, _config: &Arc<Config>) {
    handle_ws(request, sessions, false);
}

pub fn handle_ws_terminal(request: Request, sessions: &Arc<Sessions>, _config: &Arc<Config>) {
    handle_ws(request, sessions, true);
}

fn handle_ws(request: Request, sessions: &Arc<Sessions>, terminal: bool) {
    // Auth via cookie
    let cookie_header = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Cookie"))
        .map(|h| h.value.as_str());
    let has_session = cookie_token(cookie_header, SESSION_COOKIE)
        .map(|t| sessions.verify(&t))
        .unwrap_or(false);
    if !has_session {
        let resp = Response::from_string("Unauthorized").with_status_code(StatusCode(401));
        let _ = request.respond(resp);
        return;
    }

    let key = match request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Sec-WebSocket-Key"))
        .map(|h| h.value.as_str().to_string())
    {
        Some(k) => k,
        None => {
            let _ = request.respond(
                Response::from_string("Missing Sec-WebSocket-Key")
                    .with_status_code(StatusCode(400)),
            );
            return;
        }
    };

    let accept = ws::accept_key(&key);
    let response = Response::new_empty(StatusCode(101))
        .with_header(Header::from_bytes(&b"Upgrade"[..], &b"websocket"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Connection"[..], &b"Upgrade"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Sec-WebSocket-Accept"[..], accept.as_bytes()).unwrap());

    let stream = request.upgrade("websocket", response);

    std::thread::spawn(move || {
        let res = if terminal {
            #[cfg(unix)]
            { terminal_unix::run_terminal_session(stream) }
            #[cfg(not(unix))]
            {
                let mut s = stream;
                let _ = ws::write_frame(&mut *s, 0x1, b"Terminal is only supported on Unix.");
                let _ = ws::write_frame(&mut *s, 0x8, &[]);
                Ok(())
            }
        } else {
            ws::run_remote_session(stream)
        };
        if let Err(e) = res { eprintln!("ws session ended: {e}"); }
    });
}
