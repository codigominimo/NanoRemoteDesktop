use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use super::capture_windows;
#[cfg(target_os = "windows")]
use super::input_windows;

pub fn accept_key(key: &str) -> String {
    const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let mut input = String::with_capacity(key.len() + GUID.len());
    input.push_str(key);
    input.push_str(GUID);
    let sha1 = sha1_bytes(input.as_bytes());
    base64_encode(&sha1)
}

pub struct RemoteConfig {
    pub fps_ms: AtomicU64,
    pub max_width: AtomicU32,
    pub quality: AtomicU32,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            fps_ms: AtomicU64::new(16), // ~60 fps target
            max_width: AtomicU32::new(1280),
            quality: AtomicU32::new(45),
        }
    }
}

pub fn run_remote_session(stream: Box<dyn tiny_http::ReadWrite + Send>) -> Result<(), String> {
    #[cfg(not(target_os = "windows"))]
    {
        let mut stream = stream;
        let _ = write_frame(
            &mut *stream,
            0x1,
            b"Remote capture/input is currently implemented for Windows only.",
        );
        let _ = write_frame(&mut *stream, 0x8, &[]);
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let stream = Arc::new(Mutex::new(stream));
        let running = Arc::new(AtomicBool::new(true));
        let cfg = Arc::new(RemoteConfig::default());

        let reader_stream = stream.clone();
        let reader_running = running.clone();
        let reader_cfg = cfg.clone();
        let reader = std::thread::spawn(move || {
            while reader_running.load(Ordering::Relaxed) {
                let frame = {
                    let mut s = match reader_stream.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    read_frame(&mut **s)
                };
                match frame {
                    Ok(Some(f)) => match f.opcode {
                        0x8 => {
                            reader_running.store(false, Ordering::Relaxed);
                            let mut s = match reader_stream.lock() {
                                Ok(g) => g,
                                Err(_) => break,
                            };
                            let _ = write_frame(&mut **s, 0x8, &[]);
                            break;
                        }
                        0x9 => {
                            let mut s = match reader_stream.lock() {
                                Ok(g) => g,
                                Err(_) => break,
                            };
                            let _ = write_frame(&mut **s, 0xA, &f.payload);
                        }
                        0xA => {}
                        0x1 => {
                            if let Ok(text) = std::str::from_utf8(&f.payload) {
                                handle_client_message(text, &reader_cfg);
                            }
                        }
                        _ => {}
                    },
                    Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                    Err(_) => {
                        reader_running.store(false, Ordering::Relaxed);
                        break;
                    }
                }
            }
        });

        let mut capturer =
            capture_windows::Capturer::new().map_err(|e| format!("capture init: {e}"))?;
        let mut ping_ticks: u32 = 0;
        while running.load(Ordering::Relaxed) {
            let loop_start = Instant::now();
            let max_w = cfg.max_width.load(Ordering::Relaxed) as usize;
            let fps_ms = cfg.fps_ms.load(Ordering::Relaxed).max(10);
            let quality = cfg.quality.load(Ordering::Relaxed) as u8;
            capturer.set_max_width(max_w);
            capturer.set_quality(quality);

            match capturer.capture_frame() {
                Ok(Some(frame_bytes)) => {
                    let mut s = match stream.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    if write_frame(&mut **s, 0x2, frame_bytes).is_err() {
                        running.store(false, Ordering::Relaxed);
                        break;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    let msg = format!("capture error: {e}");
                    let mut s = match stream.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    let _ = write_frame(&mut **s, 0x1, msg.as_bytes());
                    running.store(false, Ordering::Relaxed);
                    break;
                }
            }

            ping_ticks += 1;
            if ping_ticks >= 120 {
                ping_ticks = 0;
                let mut s = match stream.lock() {
                    Ok(g) => g,
                    Err(_) => break,
                };
                let _ = write_frame(&mut **s, 0x9, b"ping");
            }

            let elapsed = loop_start.elapsed();
            let target = Duration::from_millis(fps_ms);
            if elapsed < target {
                std::thread::sleep(target - elapsed);
            }
        }

        {
            if let Ok(mut s) = stream.lock() {
                let _ = write_frame(&mut **s, 0x8, &[]);
            }
        }
        let _ = reader.join();
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn handle_client_message(text: &str, cfg: &RemoteConfig) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let Some(t) = v.get("t").and_then(|x| x.as_str()) else {
        return;
    };
    match t {
        "cfg" => {
            if let Some(fps) = v.get("fps").and_then(|x| x.as_u64()) {
                let ms = (1000 / fps.clamp(1, 60)).max(16);
                cfg.fps_ms.store(ms, Ordering::Relaxed);
            }
            if let Some(w) = v.get("maxW").and_then(|x| x.as_u64()) {
                let w = w.clamp(480, 1920) as u32;
                cfg.max_width.store(w, Ordering::Relaxed);
            }
            if let Some(q) = v.get("q").and_then(|x| x.as_u64()) {
                cfg.quality.store(q.clamp(20, 90) as u32, Ordering::Relaxed);
            }
            if let Some(br) = v.get("br").and_then(|x| x.as_u64()) {
                // legacy client compat: map bitrate to quality-ish
                let q = ((br.clamp(300, 12000) as f32 / 12000.0) * 70.0 + 20.0) as u32;
                cfg.quality.store(q.clamp(20, 90), Ordering::Relaxed);
            }
        }
        "mm" | "md" | "mu" => {
            let x = v.get("x").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let y = v.get("y").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let btn = v.get("b").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            match t {
                "mm" => input_windows::mouse_move_norm(x, y),
                "md" => input_windows::mouse_button(x, y, btn, true),
                "mu" => input_windows::mouse_button(x, y, btn, false),
                _ => {}
            }
        }
        "mw" => {
            let dy = v.get("dy").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
            input_windows::mouse_wheel(dy);
        }
        "kd" | "ku" => {
            let down = t == "kd";
            if let Some(vk) = v.get("vk").and_then(|x| x.as_u64()) {
                input_windows::key_vk(vk as u32, down);
            } else if let Some(ch) = v.get("char").and_then(|x| x.as_str()) {
                if down {
                    input_windows::key_text(ch);
                }
            }
        }
        _ => {}
    }
}

pub(crate) struct WsFrame {
    pub opcode: u8,
    pub payload: Vec<u8>,
}

pub(crate) fn read_frame<S: Read + ?Sized>(stream: &mut S) -> Result<Option<WsFrame>, String> {
    let mut head = [0u8; 2];
    match read_exact_or_eof(stream, &mut head) {
        Ok(true) => {}
        Ok(false) => return Ok(None),
        Err(e) => return Err(e),
    }
    let b0 = head[0];
    let b1 = head[1];
    let opcode = b0 & 0x0F;
    let masked = (b1 & 0x80) != 0;
    let mut len = (b1 & 0x7F) as usize;
    if len == 126 {
        let mut ext = [0u8; 2];
        read_exact(stream, &mut ext).map_err(|e| e.to_string())?;
        len = u16::from_be_bytes(ext) as usize;
    } else if len == 127 {
        let mut ext = [0u8; 8];
        read_exact(stream, &mut ext).map_err(|e| e.to_string())?;
        let l = u64::from_be_bytes(ext);
        if l > (32 * 1024 * 1024) {
            return Err("frame too large".to_string());
        }
        len = l as usize;
    }
    let mut mask = [0u8; 4];
    if masked {
        read_exact(stream, &mut mask).map_err(|e| e.to_string())?;
    }
    let mut payload = vec![0u8; len];
    if len > 0 {
        read_exact(stream, &mut payload).map_err(|e| e.to_string())?;
    }
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }
    Ok(Some(WsFrame { opcode, payload }))
}

pub(crate) fn write_frame<S: Write + ?Sized>(
    stream: &mut S,
    opcode: u8,
    payload: &[u8],
) -> Result<(), String> {
    let mut header = Vec::with_capacity(10 + payload.len());
    header.push(0x80 | (opcode & 0x0F));
    let len = payload.len();
    if len < 126 {
        header.push(len as u8);
    } else if len <= u16::MAX as usize {
        header.push(126);
        header.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        header.push(127);
        header.extend_from_slice(&(len as u64).to_be_bytes());
    }
    header.extend_from_slice(payload);
    stream.write_all(&header).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn read_exact<Rd: Read + ?Sized>(r: &mut Rd, buf: &mut [u8]) -> Result<(), String> {
    let mut off = 0;
    while off < buf.len() {
        let n = r.read(&mut buf[off..]).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("unexpected eof".to_string());
        }
        off += n;
    }
    Ok(())
}

fn read_exact_or_eof<Rd: Read + ?Sized>(r: &mut Rd, buf: &mut [u8]) -> Result<bool, String> {
    let mut off = 0;
    while off < buf.len() {
        match r.read(&mut buf[off..]) {
            Ok(0) if off == 0 => return Ok(false),
            Ok(0) => return Err("unexpected eof".to_string()),
            Ok(n) => off += n,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if off == 0 {
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(true)
}

fn sha1_bytes(msg: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;
    let ml = (msg.len() as u64).wrapping_mul(8);
    let mut buf = Vec::with_capacity(msg.len() + 64);
    buf.extend_from_slice(msg);
    buf.push(0x80);
    while (buf.len() % 64) != 56 {
        buf.push(0);
    }
    buf.extend_from_slice(&ml.to_be_bytes());
    for chunk in buf.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            let j = i * 4;
            w[i] = u32::from_be_bytes([chunk[j], chunk[j + 1], chunk[j + 2], chunk[j + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }
    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

fn base64_encode(input: &[u8]) -> String {
    const ALPH: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(ALPH[((n >> 18) & 63) as usize] as char);
        out.push(ALPH[((n >> 12) & 63) as usize] as char);
        out.push(ALPH[((n >> 6) & 63) as usize] as char);
        out.push(ALPH[(n & 63) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPH[((n >> 18) & 63) as usize] as char);
        out.push(ALPH[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPH[((n >> 18) & 63) as usize] as char);
        out.push(ALPH[((n >> 12) & 63) as usize] as char);
        out.push(ALPH[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}
