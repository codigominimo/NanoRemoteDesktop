#![cfg(unix)]

use std::ffi::CString;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::ws::{read_frame, write_frame};

pub fn run_terminal_session(stream: Box<dyn tiny_http::ReadWrite + Send>) -> Result<(), String> {
    let (master, slave) = open_pty()?;
    let shell = default_shell();
    let child = spawn_shell(&slave, &shell)?;
    drop(slave);

    let stream = Arc::new(Mutex::new(stream));
    let running = Arc::new(AtomicBool::new(true));

    let reader_stream = stream.clone();
    let reader_running = running.clone();
    let master_fd = master.as_raw_fd();
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while reader_running.load(Ordering::Relaxed) {
            let n = unsafe { libc::read(master_fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            if n > 0 {
                let mut s = match reader_stream.lock() { Ok(g) => g, Err(_) => break };
                if write_frame(&mut **s, 0x2, &buf[..n as usize]).is_err() { break; }
            } else if n == 0 {
                break;
            } else {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted { continue; }
                if err.kind() == std::io::ErrorKind::WouldBlock { std::thread::sleep(Duration::from_millis(10)); continue; }
                // EIO on master means child closed slave side
                break;
            }
        }
        reader_running.store(false, Ordering::Relaxed);
    });

    while running.load(Ordering::Relaxed) {
        let frame = {
            let mut s = match stream.lock() { Ok(g) => g, Err(_) => break };
            read_frame(&mut **s)
        };
        match frame {
            Ok(Some(f)) => match f.opcode {
                0x8 => {
                    running.store(false, Ordering::Relaxed);
                    let mut s = match stream.lock() { Ok(g) => g, Err(_) => break };
                    let _ = write_frame(&mut **s, 0x8, &[]);
                    break;
                }
                0x9 => { let mut s = match stream.lock() { Ok(g)=>g, Err(_)=>break }; let _ = write_frame(&mut **s, 0xA, &f.payload); }
                0xA => {}
                0x1 | 0x2 => { handle_client_msg(master.as_raw_fd(), &f.payload); }
                _ => {}
            },
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => { running.store(false, Ordering::Relaxed); break; }
        }
    }

    unsafe {
        libc::kill(child, libc::SIGTERM);
        let mut status = 0;
        libc::waitpid(child, &mut status, libc::WNOHANG);
    }
    drop(master);
    let _ = reader.join();
    Ok(())
}

fn handle_client_msg(master_fd: RawFd, payload: &[u8]) {
    if payload.is_empty() { return; }
    if payload[0] == b'{' {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(payload) {
            match v.get("t").and_then(|x| x.as_str()) {
                Some("i") => {
                    if let Some(data) = v.get("d").and_then(|x| x.as_str()) { write_all_fd(master_fd, data.as_bytes()); }
                }
                Some("r") => {
                    let cols = v.get("c").and_then(|x| x.as_u64()).unwrap_or(120) as libc::c_ushort;
                    let rows = v.get("r").and_then(|x| x.as_u64()).unwrap_or(30) as libc::c_ushort;
                    set_winsize(master_fd, rows.max(2), cols.max(10));
                }
                _ => {}
            }
            return;
        }
    }
    write_all_fd(master_fd, payload);
}

fn write_all_fd(fd: RawFd, mut data: &[u8]) {
    while !data.is_empty() {
        let n = unsafe { libc::write(fd, data.as_ptr() as *const _, data.len()) };
        if n > 0 { data = &data[n as usize..]; }
        else {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted { continue; }
            break;
        }
    }
}

fn open_pty() -> Result<(OwnedFd, OwnedFd), String> {
    unsafe {
        let mut master: libc::c_int = -1;
        let mut slave: libc::c_int = -1;
        let rc = libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
        if rc != 0 { return Err(format!("openpty: {}", std::io::Error::last_os_error())); }
        let flags = libc::fcntl(master, libc::F_GETFL, 0);
        if flags != -1 { libc::fcntl(master, libc::F_SETFL, flags | libc::O_NONBLOCK); }
        Ok((OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)))
    }
}

fn set_winsize(fd: RawFd, rows: libc::c_ushort, cols: libc::c_ushort) {
    unsafe {
        let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        libc::ioctl(fd, libc::TIOCSWINSZ as _, &ws);
    }
}

fn default_shell() -> String {
    std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".to_string())
}

fn spawn_shell(slave: &OwnedFd, shell: &str) -> Result<libc::pid_t, String> {
    let shell_c = CString::new(shell).map_err(|_| "invalid shell path")?;
    let arg0 = CString::new(shell).map_err(|_| "invalid shell path")?;
    unsafe {
        let pid = libc::fork();
        if pid < 0 { return Err(format!("fork: {}", std::io::Error::last_os_error())); }
        if pid == 0 {
            libc::setsid();
            libc::ioctl(slave.as_raw_fd(), libc::TIOCSCTTY as _, 0);
            libc::dup2(slave.as_raw_fd(), 0);
            libc::dup2(slave.as_raw_fd(), 1);
            libc::dup2(slave.as_raw_fd(), 2);
            if slave.as_raw_fd() > 2 { libc::close(slave.as_raw_fd()); }
            let mut args = [arg0.as_ptr(), std::ptr::null()];
            let env_term = CString::new("TERM=xterm-256color").unwrap();
            let env_path = CString::new(format!("PATH={}", std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()))).unwrap();
            let env_home = CString::new(format!("HOME={}", std::env::var("HOME").unwrap_or_else(|_| "/root".into()))).unwrap();
            let mut envp = [env_term.as_ptr(), env_path.as_ptr(), env_home.as_ptr(), std::ptr::null()];
            libc::execve(shell_c.as_ptr(), args.as_mut_ptr(), envp.as_mut_ptr());
            libc::_exit(127);
        }
        Ok(pid)
    }
}