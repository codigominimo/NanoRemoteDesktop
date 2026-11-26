use std::collections::HashSet;
use std::sync::Mutex;

pub struct Sessions {
    tokens: Mutex<HashSet<String>>,
}

impl Sessions {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashSet::new()),
        }
    }

    pub fn create(&self) -> String {
        let token = random_hex(32);
        self.tokens
            .lock()
            .expect("sessions mutex")
            .insert(token.clone());
        token
    }

    pub fn verify(&self, token: &str) -> bool {
        self.tokens.lock().expect("sessions mutex").contains(token)
    }

    pub fn revoke(&self, token: &str) {
        self.tokens.lock().expect("sessions mutex").remove(token);
    }
}

pub fn check_basic_auth(header: Option<&str>, expected_user: &str, expected_pass: &str) -> bool {
    let Some(header) = header else {
        return false;
    };
    let Some(encoded) = header.strip_prefix("Basic ") else {
        return false;
    };
    let Some(decoded) = decode_base64(encoded.trim()) else {
        return false;
    };
    let Some((user, pass)) = decoded.split_once(':') else {
        return false;
    };
    constant_time_eq(user.as_bytes(), expected_user.as_bytes())
        && constant_time_eq(pass.as_bytes(), expected_pass.as_bytes())
}

pub fn check_credentials(user: &str, pass: &str, expected_user: &str, expected_pass: &str) -> bool {
    constant_time_eq(user.as_bytes(), expected_user.as_bytes())
        && constant_time_eq(pass.as_bytes(), expected_pass.as_bytes())
}

pub fn cookie_token(cookie_header: Option<&str>, name: &str) -> Option<String> {
    let header = cookie_header?;
    for part in header.split(';') {
        let mut kv = part.trim().splitn(2, '=');
        let k = kv.next()?.trim();
        let v = kv.next()?.trim();
        if k == name && !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("random");
    let mut out = String::with_capacity(bytes * 2);
    for b in buf {
        use std::fmt::Write;
        let _ = write!(out, "{:02x}", b);
    }
    out
}

fn decode_base64(input: &str) -> Option<String> {
    let bytes: Vec<u8> = input.bytes().filter(|b| !b" \r\n\t".contains(b)).collect();
    if bytes.is_empty() {
        return Some(String::new());
    }

    fn val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c0 = bytes.get(i).copied()?;
        let c1 = bytes.get(i + 1).copied()?;
        if c0 == b'=' || c1 == b'=' {
            return None;
        }
        let v0 = val(c0)?;
        let v1 = val(c1)?;
        out.push((v0 << 2) | (v1 >> 4));

        let c2 = bytes.get(i + 2).copied().unwrap_or(b'=');
        let c3 = bytes.get(i + 3).copied().unwrap_or(b'=');

        if c2 != b'=' {
            let v2 = val(c2)?;
            out.push(((v1 & 0x0f) << 4) | (v2 >> 2));
            if c3 != b'=' {
                let v3 = val(c3)?;
                out.push(((v2 & 0x03) << 6) | v3);
            }
        } else if c3 != b'=' {
            return None;
        }

        i += 4;
    }

    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_auth_ok() {
        // admin:Gestion! => YWRtaW46R2VzdGlvbiE=
        assert!(check_basic_auth(
            Some("Basic YWRtaW46R2VzdGlvbiE="),
            "admin",
            "Gestion!"
        ));
    }

    #[test]
    fn basic_auth_bad() {
        assert!(!check_basic_auth(
            Some("Basic YWRtaW46bm9wZQ=="),
            "admin",
            "Gestion!"
        ));
    }
}
