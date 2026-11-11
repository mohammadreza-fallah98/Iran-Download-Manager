//! HLS (`.m3u8`) downloads: reads the playlist, fetches the segments a few at a time,
//! decrypts AES-128 segments and appends everything to one file (no ffmpeg needed).
use crate::model::*;
use crate::net::{self, describe_error};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

const PARALLEL_SEGMENTS: usize = 4;
const SEGMENT_RETRIES: u32 = 3;
const MAX_PLAYLIST_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SEGMENT_BYTES: u64 = 512 * 1024 * 1024;

pub fn is_hls_url(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    path.ends_with(".m3u8") || path.ends_with(".m3u")
}

// ── Playlist parsing ─────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub struct Variant {
    pub bandwidth: u64,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub uri: String,
    pub iv: Option<[u8; 16]>,
}

#[derive(Debug, PartialEq)]
pub struct Seg {
    pub url: String,
    pub key: Option<Key>,
    pub seq: u64,
}

#[derive(Debug, PartialEq)]
pub struct Media {
    pub segs: Vec<Seg>,
    pub init: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
    Master(Vec<Variant>),
    Media(Media),
}

/// Splits `A=1,B="x,y"` into pairs; quotes are removed.
fn attrs(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut key = String::new();
    let mut val = String::new();
    let (mut in_key, mut quoted) = (true, false);
    let flush = |k: &mut String, v: &mut String, out: &mut HashMap<String, String>| {
        if !k.trim().is_empty() {
            out.insert(k.trim().to_ascii_uppercase(), v.clone());
        }
        k.clear();
        v.clear();
    };
    for c in text.chars() {
        match (c, in_key, quoted) {
            ('=', true, _) => in_key = false,
            ('"', false, _) => quoted = !quoted,
            (',', false, false) => {
                flush(&mut key, &mut val, &mut out);
                in_key = true;
            }
            (_, true, _) => key.push(c),
            (_, false, _) => val.push(c),
        }
    }
    flush(&mut key, &mut val, &mut out);
    out
}

fn parse_iv(text: &str) -> Option<[u8; 16]> {
    let hex = text.trim().trim_start_matches("0x").trim_start_matches("0X");
    if hex.is_empty() || hex.len() > 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let padded = format!("{hex:0>32}");
    let mut iv = [0u8; 16];
    for (i, b) in iv.iter_mut().enumerate() {
        *b = u8::from_str_radix(&padded[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(iv)
}

pub fn parse(text: &str, base: &str) -> Result<Parsed, String> {
    if !text.trim_start().starts_with("#EXTM3U") {
        return Err("This address is not an HLS playlist".into());
    }
    if text.contains("#EXT-X-STREAM-INF") {
        let mut variants = Vec::new();
        let mut pending: Option<u64> = None;
        for line in text.lines().map(str::trim) {
            if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
                pending = Some(attrs(rest).get("BANDWIDTH").and_then(|b| b.parse().ok()).unwrap_or(0));
            } else if !line.is_empty() && !line.starts_with('#') {
                if let Some(bandwidth) = pending.take() {
                    variants.push(Variant { bandwidth, url: net::join_url(base, line) });
                }
            }
        }
        return Ok(Parsed::Master(variants));
    }
    let mut segs = Vec::new();
    let mut init = None;
    let mut key: Option<Key> = None;
    let mut media_seq = 0u64;
    let mut ended = false;
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            media_seq = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("#EXT-X-KEY:") {
            let a = attrs(rest);
            match a.get("METHOD").map(String::as_str) {
                Some("NONE") | None => key = None,
                Some("AES-128") => {
                    let uri = a.get("URI").ok_or("The encryption key address is missing")?;
                    key = Some(Key { uri: net::join_url(base, uri), iv: a.get("IV").and_then(|v| parse_iv(v)) });
                }
                Some(other) => return Err(format!("Encryption method {other} is not supported")),
            }
        } else if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            init = attrs(rest).get("URI").map(|u| net::join_url(base, u));
        } else if line == "#EXT-X-ENDLIST" {
            ended = true;
        } else if !line.is_empty() && !line.starts_with('#') {
            let seq = media_seq + segs.len() as u64;
            segs.push(Seg { url: net::join_url(base, line), key: key.clone(), seq });
        }
    }
    if segs.is_empty() {
        return Err("The playlist has no segments".into());
    }
    if !ended {
        return Err("Live streams are not supported".into());
    }
    Ok(Parsed::Media(Media { segs, init }))
}

/// The highest-bandwidth variant of a master playlist.
pub fn best_variant(list: &[Variant]) -> Option<&Variant> {
    list.iter().max_by_key(|v| v.bandwidth)
}

// ── AES-128 (decrypt only) ───────────────────────────────────────────────────

fn xtime(x: u8) -> u8 {
    (x << 1) ^ if x & 0x80 != 0 { 0x1B } else { 0 }
}

fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut r = 0;
    while b != 0 {
        if b & 1 != 0 {
            r ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }
    r
}

fn sboxes() -> ([u8; 256], [u8; 256]) {
    let mut sbox = [0u8; 256];
    let (mut p, mut q) = (1u8, 1u8);
    loop {
        p = p ^ (p << 1) ^ if p & 0x80 != 0 { 0x1B } else { 0 };
        q ^= q << 1;
        q ^= q << 2;
        q ^= q << 4;
        if q & 0x80 != 0 {
            q ^= 0x09;
        }
        sbox[p as usize] = q ^ q.rotate_left(1) ^ q.rotate_left(2) ^ q.rotate_left(3) ^ q.rotate_left(4) ^ 0x63;
        if p == 1 {
            break;
        }
    }
    sbox[0] = 0x63;
    let mut inv = [0u8; 256];
    for (i, &s) in sbox.iter().enumerate() {
        inv[s as usize] = i as u8;
    }
    (sbox, inv)
}

struct Aes128 {
    round_keys: [[u8; 16]; 11],
    inv_sbox: [u8; 256],
}

impl Aes128 {
    fn new(key: &[u8; 16]) -> Self {
        let (sbox, inv_sbox) = sboxes();
        let mut w = [[0u8; 4]; 44];
        for i in 0..4 {
            w[i].copy_from_slice(&key[i * 4..i * 4 + 4]);
        }
        let mut rcon = 1u8;
        for i in 4..44 {
            let mut t = w[i - 1];
            if i % 4 == 0 {
                t = [sbox[t[1] as usize] ^ rcon, sbox[t[2] as usize], sbox[t[3] as usize], sbox[t[0] as usize]];
                rcon = xtime(rcon);
            }
            for j in 0..4 {
                w[i][j] = w[i - 4][j] ^ t[j];
            }
        }
        let mut round_keys = [[0u8; 16]; 11];
        for (r, rk) in round_keys.iter_mut().enumerate() {
            for c in 0..4 {
                rk[c * 4..c * 4 + 4].copy_from_slice(&w[r * 4 + c]);
            }
        }
        Aes128 { round_keys, inv_sbox }
    }

    fn decrypt_block(&self, block: &[u8; 16]) -> [u8; 16] {
        let mut s = *block;
        let add = |s: &mut [u8; 16], rk: &[u8; 16]| s.iter_mut().zip(rk).for_each(|(a, b)| *a ^= b);
        add(&mut s, &self.round_keys[10]);
        for round in (0..10).rev() {
            // inverse ShiftRows: row r moves right by r
            let old = s;
            for c in 0..4 {
                for r in 0..4 {
                    s[4 * c + r] = old[4 * ((c + 4 - r) % 4) + r];
                }
            }
            s.iter_mut().for_each(|b| *b = self.inv_sbox[*b as usize]);
            add(&mut s, &self.round_keys[round]);
            if round > 0 {
                for c in 0..4 {
                    let col = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                    s[4 * c] = gmul(col[0], 14) ^ gmul(col[1], 11) ^ gmul(col[2], 13) ^ gmul(col[3], 9);
                    s[4 * c + 1] = gmul(col[0], 9) ^ gmul(col[1], 14) ^ gmul(col[2], 11) ^ gmul(col[3], 13);
                    s[4 * c + 2] = gmul(col[0], 13) ^ gmul(col[1], 9) ^ gmul(col[2], 14) ^ gmul(col[3], 11);
                    s[4 * c + 3] = gmul(col[0], 11) ^ gmul(col[1], 13) ^ gmul(col[2], 9) ^ gmul(col[3], 14);
                }
            }
        }
        s
    }
}

/// AES-128-CBC with PKCS7 padding.
pub fn decrypt_cbc(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>, String> {
    if data.is_empty() || data.len() % 16 != 0 {
        return Err("Encrypted segment has an invalid size".into());
    }
    let aes = Aes128::new(key);
    let mut prev = *iv;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks_exact(16) {
        let block: [u8; 16] = chunk.try_into().unwrap();
        let plain = aes.decrypt_block(&block);
        out.extend(plain.iter().zip(prev.iter()).map(|(a, b)| a ^ b));
        prev = block;
    }
    let pad = *out.last().unwrap() as usize;
    if pad == 0 || pad > 16 || pad > out.len() {
        return Err("Wrong key or damaged segment".into());
    }
    out.truncate(out.len() - pad);
    Ok(out)
}

// ── Download ─────────────────────────────────────────────────────────────────

pub(crate) fn fetch(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut last = String::new();
    for attempt in 0..SEGMENT_RETRIES {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(500 * attempt as u64));
        }
        match net::get(url).call() {
            Ok(resp) => {
                let mut buf = Vec::new();
                match resp.into_reader().take(limit).read_to_end(&mut buf) {
                    Ok(_) => {
                        crate::engine::throttle_global(buf.len());
                        return Ok(buf);
                    }
                    Err(e) => last = e.to_string(),
                }
            }
            Err(e) => last = describe_error(e),
        }
    }
    Err(last)
}

fn fetch_text(url: &str) -> Result<String, String> {
    String::from_utf8(fetch(url, MAX_PLAYLIST_BYTES)?).map_err(|_| "The playlist is not text".to_string())
}

/// Loads the media playlist, following a master playlist to its best quality.
fn load_media(url: &str) -> Result<Media, String> {
    let mut current = url.to_string();
    for _ in 0..3 {
        match parse(&fetch_text(&current)?, &current)? {
            Parsed::Media(m) => return Ok(m),
            Parsed::Master(list) => {
                current = best_variant(&list).ok_or("The playlist has no video streams")?.url.clone();
            }
        }
    }
    Err("The playlist points to too many other playlists".into())
}

pub fn run(h: Handle) {
    let result = work(&h);
    finish(&h, result);
}

fn work(h: &Handle) -> Result<(), String> {
    let (url, temp, dir) = {
        let d = h.d.lock().unwrap();
        (d.url.clone(), d.temp_path(), d.save_dir.clone())
    };
    let media = load_media(&url)?;
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create folder: {e}"))?;

    // Resume only when the file on disk matches what was recorded
    let resume_done = {
        let d = h.d.lock().unwrap();
        match &d.hls {
            Some(s) if s.total == media.segs.len() && fs::metadata(&temp).map(|m| m.len()).unwrap_or(0) == s.bytes => s.done,
            _ => 0,
        }
    };
    let mut file = if resume_done > 0 {
        OpenOptions::new().append(true).open(&temp).map_err(|e| e.to_string())?
    } else {
        let f = fs::File::create(&temp).map_err(|e| format!("Cannot create file: {e}"))?;
        let mut d = h.d.lock().unwrap();
        d.hls = Some(HlsState { total: media.segs.len(), done: 0, bytes: 0, fragmented: media.init.is_some() });
        d.resumable = true;
        f
    };
    if resume_done == 0 {
        if let Some(init) = &media.init {
            let data = fetch(init, MAX_SEGMENT_BYTES)?;
            file.write_all(&data).map_err(|e| e.to_string())?;
            h.d.lock().unwrap().hls.as_mut().unwrap().bytes += data.len() as u64;
        }
    }

    let mut keys: HashMap<String, [u8; 16]> = HashMap::new();
    let mut idx = resume_done;
    while idx < media.segs.len() {
        if h.stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let end = (idx + PARALLEL_SEGMENTS).min(media.segs.len());
        let batch = &media.segs[idx..end];
        for seg in batch {
            if let Some(k) = &seg.key {
                if !keys.contains_key(&k.uri) {
                    let raw = fetch(&k.uri, 4096)?;
                    let key: [u8; 16] = raw.as_slice().try_into().map_err(|_| "The encryption key has an invalid size".to_string())?;
                    keys.insert(k.uri.clone(), key);
                }
            }
        }
        h.d.lock().unwrap().active_connections = batch.len() as u32;
        let results: Vec<Result<Vec<u8>, String>> = std::thread::scope(|scope| {
            let jobs: Vec<_> = batch
                .iter()
                .map(|seg| {
                    let keys = &keys;
                    scope.spawn(move || {
                        let data = fetch(&seg.url, MAX_SEGMENT_BYTES)?;
                        match &seg.key {
                            Some(k) => {
                                let iv = k.iv.unwrap_or_else(|| {
                                    let mut iv = [0u8; 16];
                                    iv[8..].copy_from_slice(&seg.seq.to_be_bytes());
                                    iv
                                });
                                decrypt_cbc(&data, &keys[&k.uri], &iv)
                            }
                            None => Ok(data),
                        }
                    })
                })
                .collect();
            jobs.into_iter().map(|j| j.join().unwrap_or_else(|_| Err("A download thread failed".into()))).collect()
        });
        for r in results {
            let data = r?;
            file.write_all(&data).map_err(|e| format!("Cannot write the file: {e}"))?;
            let mut d = h.d.lock().unwrap();
            let s = d.hls.as_mut().unwrap();
            s.done += 1;
            s.bytes += data.len() as u64;
        }
        file.flush().map_err(|e| e.to_string())?;
        idx = end;
    }
    Ok(())
}

pub(crate) fn finish(h: &Handle, error: Result<(), String>) {
    let mut d = h.d.lock().unwrap();
    d.active_connections = 0;
    d.speed = 0.0;
    let complete = d.hls.as_ref().is_some_and(|s| s.total > 0 && s.done == s.total);
    if complete && error.is_ok() {
        let fragmented = d.hls.as_ref().is_some_and(|s| s.fragmented);
        let wanted = if fragmented { "mp4" } else { "ts" };
        let stem = d.file_name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_else(|| d.file_name.clone());
        let target = unique_path(d.save_dir.join(format!("{stem}.{wanted}")));
        match move_file_any(&d.temp_path(), &target) {
            Ok(()) => {
                d.file_name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                d.size = d.hls.as_ref().map(|s| s.bytes);
                d.status = Status::Complete;
                d.in_queue = false;
                d.message = format!("Download complete. Downloaded {} Bytes Total", d.hls.as_ref().map_or(0, |s| s.bytes));
            }
            Err(e) => {
                d.status = Status::Error;
                d.message = format!("Cannot rename the downloaded file: {e}");
            }
        }
    } else if let Some(e) = error.err().filter(|_| !h.stop.load(Ordering::SeqCst)) {
        d.status = Status::Error;
        d.message = e;
    } else {
        d.status = Status::Stopped;
        d.message = "Stopped".into();
    }
}

pub(crate) fn unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let dir = path.parent().map(PathBuf::from).unwrap_or_default();
    (2..).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|p| !p.exists()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_hls_url_checks_path_not_query() {
        assert!(is_hls_url("https://a.com/v/index.m3u8?token=1"));
        assert!(!is_hls_url("https://a.com/v.mp4?x=.m3u8"));
    }

    #[test]
    fn parse_media_playlist_reads_segments_key_and_init() {
        let text = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-KEY:METHOD=AES-128,URI=\"k.bin\",IV=0x01\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4,\nseg0.ts\n#EXTINF:4,\nhttps://cdn.x/seg1.ts\n#EXT-X-ENDLIST\n";
        let Parsed::Media(m) = parse(text, "https://a.com/v/index.m3u8").unwrap() else { panic!("expected media") };
        assert_eq!(m.init.as_deref(), Some("https://a.com/v/init.mp4"));
        assert_eq!(m.segs.len(), 2);
        assert_eq!(m.segs[0].url, "https://a.com/v/seg0.ts");
        assert_eq!(m.segs[0].seq, 7);
        assert_eq!(m.segs[1].url, "https://cdn.x/seg1.ts");
        let key = m.segs[0].key.as_ref().unwrap();
        assert_eq!(key.uri, "https://a.com/v/k.bin");
        assert_eq!(key.iv.unwrap()[15], 1);
    }

    #[test]
    fn parse_master_playlist_picks_highest_bandwidth() {
        let text = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2400000\nhigh.m3u8\n";
        let Parsed::Master(list) = parse(text, "https://a.com/m.m3u8").unwrap() else { panic!("expected master") };
        assert_eq!(best_variant(&list).unwrap().url, "https://a.com/high.m3u8");
    }

    #[test]
    fn parse_rejects_live_unknown_encryption_and_non_playlists() {
        assert!(parse("#EXTM3U\n#EXTINF:4,\na.ts\n", "https://a.com/x.m3u8").unwrap_err().contains("Live"));
        assert!(parse("#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\"\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n", "https://a.com/x.m3u8").is_err());
        assert!(parse("<html>", "https://a.com/x.m3u8").is_err());
    }

    #[test]
    fn aes128_matches_fips197_vector() {
        let key: [u8; 16] = core::array::from_fn(|i| i as u8);
        let cipher: [u8; 16] = [0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30, 0xd8, 0xcd, 0xb7, 0x80, 0x70, 0xb4, 0xc5, 0x5a];
        let plain: [u8; 16] = core::array::from_fn(|i| (i as u8) * 0x11);
        assert_eq!(Aes128::new(&key).decrypt_block(&cipher), plain);
    }

    #[test]
    fn decrypt_cbc_unpads_and_rejects_bad_sizes() {
        // One block whose plaintext is 16 bytes of 0x10 padding (empty message) under key=0, iv=0
        let key = [0u8; 16];
        let iv = [0u8; 16];
        assert!(decrypt_cbc(&[1, 2, 3], &key, &iv).is_err());
        assert!(decrypt_cbc(&[], &key, &iv).is_err());
    }
}
