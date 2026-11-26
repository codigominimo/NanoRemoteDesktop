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
    constant_time_eq(user.as_bytes(), expected_user.as_bytes()) && verify_password(pass, expected_pass)
}

pub fn hash_password(pass: &str) -> String {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).expect("random salt");
    let mut hash = [0u8; 32];
    pbkdf2_sha256(pass.as_bytes(), &salt, 120_000, &mut hash);
    format!("pbkdf2${}${}${}", 120_000, hex_encode(&salt), hex_encode(&hash))
}

pub fn verify_password(pass: &str, stored: &str) -> bool {
    if let Some(rest) = stored.strip_prefix("pbkdf2$") {
        let mut parts = rest.split('$');
        let Some(iters) = parts.next().and_then(|v| v.parse::<u32>().ok()) else { return false; };
        let Some(salt) = parts.next().and_then(hex_decode) else { return false; };
        let Some(expected) = parts.next().and_then(hex_decode) else { return false; };
        let mut out = vec![0u8; expected.len()];
        pbkdf2_sha256(pass.as_bytes(), &salt, iters, &mut out);
        return constant_time_eq(&out, &expected);
    }
    if let Some(plain) = stored.strip_prefix("plain:") {
        return constant_time_eq(pass.as_bytes(), plain.as_bytes());
    }
    constant_time_eq(pass.as_bytes(), stored.as_bytes())
}

fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    let mut block_index: u32 = 1;
    let mut pos = 0;
    while pos < out.len() {
        let mut u = hmac_sha256(password, &[salt, &block_index.to_be_bytes()].concat());
        let mut t = u.clone();
        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for (ti, ui) in t.iter_mut().zip(u.iter()) { *ti ^= *ui; }
        }
        let n = std::cmp::min(t.len(), out.len() - pos);
        out[pos..pos + n].copy_from_slice(&t[..n]);
        pos += n;
        block_index = block_index.saturating_add(1);
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK { let h = sha256(key); k[..32].copy_from_slice(&h); }
    else { k[..key.len()].copy_from_slice(key); }
    let mut ipad = [0u8; BLOCK];
    let mut opad = [0u8; BLOCK];
    for i in 0..BLOCK { ipad[i] = k[i] ^ 0x36; opad[i] = k[i] ^ 0x5c; }
    let mut inner = Vec::with_capacity(BLOCK + data.len());
    inner.extend_from_slice(&ipad); inner.extend_from_slice(data);
    let inner_hash = sha256(&inner);
    let mut outer = Vec::with_capacity(BLOCK + 32);
    outer.extend_from_slice(&opad); outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];
    let k: [u32; 64] = [
        0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
        0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
        0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
        0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
        0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
        0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
        0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
        0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2,
    ];
    let mut msg = data.to_vec();
    let bit_len = (msg.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes([chunk[i*4],chunk[i*4+1],chunk[i*4+2],chunk[i*4+3]]); }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let mut a=h[0]; let mut b=h[1]; let mut c=h[2]; let mut d=h[3]; let mut e=h[4]; let mut f=h[5]; let mut g=h[6]; let mut hh=h[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(k[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh=g; g=f; f=e; e=d.wrapping_add(t1); d=c; c=b; b=a; a=t1.wrapping_add(t2);
        }
        h[0]=h[0].wrapping_add(a); h[1]=h[1].wrapping_add(b); h[2]=h[2].wrapping_add(c); h[3]=h[3].wrapping_add(d);
        h[4]=h[4].wrapping_add(e); h[5]=h[5].wrapping_add(f); h[6]=h[6].wrapping_add(g); h[7]=h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() { out[i*4..i*4+4].copy_from_slice(&v.to_be_bytes()); }
    out
}

fn hex_encode(bytes: &[u8]) -> String { let mut s = String::with_capacity(bytes.len()*2); for b in bytes { use std::fmt::Write; let _ = write!(s, "{:02x}", b); } s }
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 { return None; }
    let mut out = Vec::with_capacity(s.len()/2);
    let b = s.as_bytes();
    for i in (0..b.len()).step_by(2) { out.push((hex_nibble(b[i])? << 4) | hex_nibble(b[i+1])?); }
    Some(out)
}
fn hex_nibble(b: u8) -> Option<u8> { match b { b'0'..=b'9'=>Some(b-b'0'), b'a'..=b'f'=>Some(b-b'a'+10), b'A'..=b'F'=>Some(b-b'A'+10), _=>None } }

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
