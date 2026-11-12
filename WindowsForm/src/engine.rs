//! Multi-connection downloader. Rules come from MECHANICS.md:
//! the file is cut into one part per connection; when a connection finishes it takes over the
//! next part if that part received fewer than PART_TAKEOVER_BYTES (function 0x5a4df0), otherwise
//! it cuts the biggest remaining part in half and fetches the second half.

use crate::model::*;
use crate::net::{describe_error, probe};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const BUFFER_BYTES: usize = 64 * 1024;
const RETRY_WAIT: Duration = Duration::from_secs(2);

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

fn new_owner() -> u64 {
    NEXT_OWNER.fetch_add(1, Ordering::Relaxed)
}

pub fn now_text() -> String {
    chrono::Local::now().format("%b %d %H:%M:%S").to_string()
}

pub fn start(h: &Handle) {
    {
        let mut d = h.d.lock().unwrap();
        if d.is_running() || d.status == Status::Complete {
            return;
        }
        d.status = Status::Connecting;
        d.message = "Connecting...".into();
        d.last_try = now_text();
    }
    h.stop.store(false, Ordering::SeqCst);
    let h = h.clone();
    let url = h.d.lock().unwrap().url.clone();
    let (hls, dash, ftp) = (crate::hls::is_hls_url(&url), crate::dash::is_dash_url(&url), crate::ftp::is_ftp_url(&url));
    thread::spawn(move || {
        if ftp {
            crate::ftp::run(h)
        } else if hls {
            crate::hls::run(h)
        } else if dash {
            crate::dash::run(h)
        } else {
            run(h)
        }
    });
}

pub fn stop(h: &Handle) {
    h.stop.store(true, Ordering::SeqCst);
}

fn run(h: Handle) {
    if let Err(e) = prepare(&h) {
        finish(&h, Some(e));
        return;
    }
    let mut workers = Vec::new();
    {
        let mut d = h.d.lock().unwrap();
        let open: Vec<usize> = (0..d.parts.len()).filter(|&i| !d.parts[i].is_done()).collect();
        for idx in open {
            let owner = new_owner();
            d.parts[idx].owner = owner;
            let h2 = h.clone();
            workers.push(thread::spawn(move || worker(h2, idx, owner)));
        }
    }
    let mut first_error = None;
    for w in workers {
        if let Ok(Err(e)) = w.join() {
            first_error.get_or_insert(e);
        }
    }
    finish(&h, first_error);
}

/// Finds the size, cuts the file into parts the first time and makes sure the temporary file exists.
fn prepare(h: &Handle) -> Result<(), String> {
    let (needs_probe, url) = {
        let d = h.d.lock().unwrap();
        (d.parts.is_empty(), d.url.clone())
    };
    if needs_probe {
        let info = probe(&url)?;
        let mut d = h.d.lock().unwrap();
        d.size = info.size;
        d.server_date = info.last_modified.clone();
        d.resumable = info.resumable && info.size.is_some();
        d.parts = plan_parts(d.size, d.resumable, d.connections);
    }
    let mut d = h.d.lock().unwrap();
    fs::create_dir_all(&d.save_dir).map_err(|e| format!("Cannot create folder: {e}"))?;
    let temp = d.temp_path();
    if let Some(parent) = temp.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create folder: {e}"))?;
    }
    if !temp.exists() || !d.resumable {
        if d.downloaded() > 0 && !d.resumable {
            d.message = "The server does not support resume; downloading from the beginning.".into();
        }
        d.parts = plan_parts(d.size, d.resumable, d.connections);
        let file = fs::File::create(&temp).map_err(|e| format!("Cannot create file: {e}"))?;
        if let Some(size) = d.size {
            file.set_len(size).map_err(|e| format!("Not enough disk space: {e}"))?;
        }
    }
    Ok(())
}

pub fn plan_parts(size: Option<u64>, resumable: bool, connections: u32) -> Vec<Part> {
    let part = |start, end| Part { start, end, pos: start, owner: 0, info: String::new() };
    match size {
        Some(size) if resumable && size > 0 => {
            let wanted = connections.max(1) as u64;
            let n = wanted.min((size / MIN_SPLIT_BYTES).max(1));
            let len = size / n;
            (0..n).map(|i| part(i * len, Some(if i == n - 1 { size } else { (i + 1) * len }))).collect()
        }
        _ => vec![part(0, size)],
    }
}

fn worker(h: Handle, mut idx: usize, owner: u64) -> Result<(), String> {
    h.d.lock().unwrap().active_connections += 1;
    let result = (|| loop {
        if !fetch_with_retries(&h, idx, owner)? {
            return Ok(());
        }
        let mut d = h.d.lock().unwrap();
        if h.stop.load(Ordering::SeqCst) || d.parts[idx].owner != owner {
            return Ok(());
        }
        d.parts[idx].info = "Complete".into();
        match pick_next(&mut d, idx, owner) {
            Some(next) => idx = next,
            None => return Ok(()),
        }
    })();
    let mut d = h.d.lock().unwrap();
    d.active_connections -= 1;
    match result {
        // A refused connection hands its part back; the others pick it up (fail only when it was the last one).
        Err(e) => {
            if d.parts[idx].owner == owner {
                d.parts[idx].owner = 0;
            }
            if d.active_connections > 0 {
                d.parts[idx].info = format!("Connection closed: {e}");
                Ok(())
            } else {
                Err(e)
            }
        }
        ok => ok,
    }
}

/// After part `idx` is finished: take over the following part when it has barely started,
/// otherwise split the biggest remaining part.
pub fn pick_next(d: &mut Download, idx: usize, owner: u64) -> Option<usize> {
    let end = d.parts[idx].end?;
    if let Some(j) = d.parts.iter().position(|p| p.start == end && !p.is_done() && p.pos - p.start < PART_TAKEOVER_BYTES) {
        d.parts[j].owner = owner;
        d.parts[j].info = "Taken over".into();
        return Some(j);
    }
    if !d.resumable {
        return None;
    }
    if let Some(j) = d.parts.iter().position(|p| p.owner == 0 && !p.is_done()) {
        d.parts[j].owner = owner;
        return Some(j);
    }
    let (k, rem) = d.parts.iter().enumerate().filter(|(_, p)| p.owner != 0).filter_map(|(i, p)| p.remaining().map(|r| (i, r))).max_by_key(|&(_, r)| r)?;
    if rem < 2 * MIN_SPLIT_BYTES {
        return None;
    }
    let mid = d.parts[k].pos + rem / 2;
    let old_end = d.parts[k].end;
    d.parts[k].end = Some(mid);
    d.parts.push(Part { start: mid, end: old_end, pos: mid, owner, info: String::new() });
    Some(d.parts.len() - 1)
}

/// Ok(true): part finished. Ok(false): stopped or the part went to another connection.
fn fetch_with_retries(h: &Handle, idx: usize, owner: u64) -> Result<bool, String> {
    let mut attempt = 0;
    loop {
        match fetch_once(h, idx, owner) {
            Ok(v) => return Ok(v),
            Err(e) => {
                attempt += 1;
                if attempt > MAX_RETRIES || h.stop.load(Ordering::SeqCst) {
                    h.d.lock().unwrap().parts[idx].info = e.clone();
                    return Err(e);
                }
                h.d.lock().unwrap().parts[idx].info = "Connection failed, restarting...".into();
                let until = Instant::now() + RETRY_WAIT;
                while Instant::now() < until {
                    if h.stop.load(Ordering::SeqCst) {
                        return Ok(false);
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

fn fetch_once(h: &Handle, idx: usize, owner: u64) -> Result<bool, String> {
    let (url, pos, end, resumable, path): (String, u64, Option<u64>, bool, PathBuf) = {
        let mut d = h.d.lock().unwrap();
        let p = &d.parts[idx];
        if p.is_done() {
            return Ok(true);
        }
        let v = (d.url.clone(), p.pos, p.end, d.resumable, d.temp_path());
        d.parts[idx].info = "Connecting...".into();
        v
    };
    let mut req = crate::net::get(&url);
    if resumable {
        let range = match end {
            Some(e) => format!("bytes={pos}-{}", e - 1),
            None => format!("bytes={pos}-"),
        };
        req = req.set("Range", &range);
    }
    let resp = req.call().map_err(describe_error)?;
    if resumable && resp.status() != 206 {
        return Err("The server does not support resume".into());
    }
    let mut file = OpenOptions::new().write(true).open(&path).map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
    {
        let mut d = h.d.lock().unwrap();
        d.status = Status::Downloading;
        d.message = "Receiving data...".into();
        d.parts[idx].info = "Receiving data...".into();
    }
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; BUFFER_BYTES];
    loop {
        if h.stop.load(Ordering::SeqCst) {
            return Ok(false);
        }
        let n = reader.read(&mut buf).map_err(|e| format!("Connection failed. {e}"))?;
        let allowed = {
            let mut d = h.d.lock().unwrap();
            let p = &mut d.parts[idx];
            if p.owner != owner {
                return Ok(false);
            }
            if n == 0 {
                if p.end.is_none() {
                    p.end = Some(p.pos);
                    let size = p.pos;
                    d.size = Some(size);
                    return Ok(true);
                }
                if p.is_done() {
                    return Ok(true);
                }
                return Err("Connection has been closed by the server.".into());
            }
            match p.end {
                Some(e) => n.min((e - p.pos) as usize),
                None => n,
            }
        };
        file.write_all(&buf[..allowed]).map_err(|e| format!("Cannot write the file: {e}"))?;
        let done = {
            let mut d = h.d.lock().unwrap();
            let p = &mut d.parts[idx];
            if p.owner != owner {
                return Ok(false);
            }
            p.pos += allowed as u64;
            p.is_done()
        };
        throttle(h, allowed);
        if done {
            return Ok(true);
        }
    }
}

/// Overall limit in KB/s (0 = none), shared by every download.
pub static GLOBAL_LIMIT_KBPS: AtomicU64 = AtomicU64::new(0);
static GLOBAL_LIMITER: std::sync::Mutex<Limiter> = std::sync::Mutex::new(Limiter { tokens: 0.0, last: None });

pub fn throttle_global(bytes: usize) {
    let kbps = GLOBAL_LIMIT_KBPS.load(Ordering::Relaxed);
    if kbps == 0 {
        return;
    }
    let rate = kbps as f64 * 1024.0;
    let wait = {
        let mut l = GLOBAL_LIMITER.lock().unwrap();
        let now = Instant::now();
        let dt = l.last.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        l.tokens = (l.tokens + rate * dt).min(rate);
        l.last = Some(now);
        l.tokens -= bytes as f64;
        if l.tokens < 0.0 { -l.tokens / rate } else { 0.0 }
    };
    if wait > 0.0 {
        thread::sleep(Duration::from_secs_f64(wait.min(5.0)));
    }
}

pub(crate) fn throttle(h: &Handle, bytes: usize) {
    throttle_global(bytes);
    let Some(kbps) = h.d.lock().unwrap().speed_limit_kbps else { return };
    let rate = (kbps.max(1) as f64) * 1024.0;
    let wait = {
        let mut l = h.limiter.lock().unwrap();
        let now = Instant::now();
        let dt = l.last.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        l.tokens = (l.tokens + rate * dt).min(rate);
        l.last = Some(now);
        l.tokens -= bytes as f64;
        if l.tokens < 0.0 { -l.tokens / rate } else { 0.0 }
    };
    if wait > 0.0 {
        thread::sleep(Duration::from_secs_f64(wait.min(5.0)));
    }
}

pub(crate) fn finish(h: &Handle, error: Option<String>) {
    let mut d = h.d.lock().unwrap();
    d.active_connections = 0;
    if d.speed > MIN_STORED_SPEED {
        d.last_speed = d.speed;
    }
    d.speed = 0.0;
    let all_done = !d.parts.is_empty() && d.parts.iter().all(Part::is_done);
    if all_done {
        let target = unique_path(d.final_path());
        match move_file_any(&d.temp_path(), &target) {
            Ok(()) => {
                if USE_SERVER_DATE.load(Ordering::Relaxed) {
                    if let Some(date) = &d.server_date {
                        apply_server_date(&target, date);
                    }
                }
                d.file_name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                d.status = Status::Complete;
                d.in_queue = false;
                d.message = format!("Download complete. Downloaded {} Bytes Total", d.downloaded());
            }
            Err(e) => {
                d.status = Status::Error;
                d.message = format!("Cannot rename the downloaded file: {e}");
            }
        }
    } else if let Some(e) = error.filter(|_| !h.stop.load(Ordering::SeqCst)) {
        d.status = Status::Error;
        d.message = e;
    } else {
        d.status = Status::Stopped;
        d.message = "Stopped".into();
    }
}

fn unique_path(path: PathBuf) -> PathBuf {
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

    fn download_with(parts: Vec<Part>) -> Download {
        Download {
            id: 1,
            url: String::new(),
            file_name: "f.bin".into(),
            save_dir: PathBuf::new(),
            category: Category::General,
            size: Some(10 * MIN_SPLIT_BYTES),
            resumable: true,
            parts,
            status: Status::Downloading,
            last_try: String::new(),
            description: String::new(),
            connections: 2,
            speed_limit_kbps: None,
            in_queue: false,
            queue_id: 0,
            referer: String::new(),
            user_agent: String::new(),
            hls: None,
            temp_dir: None,
            server_date: None,
            message: String::new(),
            last_speed: 0.0,
            speed: 0.0,
            active_connections: 0,
        }
    }

    #[test]
    fn plan_parts_resumable_splits_evenly_and_covers_file() {
        let parts = plan_parts(Some(8 * MIN_SPLIT_BYTES + 3), true, 8);
        assert_eq!(parts.len(), 8);
        assert_eq!(parts[0].start, 0);
        assert_eq!(parts.last().unwrap().end, Some(8 * MIN_SPLIT_BYTES + 3));
        assert!(parts.windows(2).all(|w| w[0].end == Some(w[1].start)));
    }

    #[test]
    fn plan_parts_not_resumable_or_unknown_size_uses_one_connection() {
        assert_eq!(plan_parts(Some(100 * MIN_SPLIT_BYTES), false, 8).len(), 1);
        assert_eq!(plan_parts(None, true, 8).len(), 1);
        assert_eq!(plan_parts(Some(10), true, 8).len(), 1);
    }

    #[test]
    fn pick_next_takes_over_neighbour_with_fewer_than_50_bytes() {
        let mut d = download_with(plan_parts(Some(10 * MIN_SPLIT_BYTES), true, 2));
        d.parts[0].pos = d.parts[0].end.unwrap();
        d.parts[1].pos += PART_TAKEOVER_BYTES - 1;
        assert_eq!(pick_next(&mut d, 0, 7), Some(1));
        assert_eq!(d.parts[1].owner, 7);
    }

    #[test]
    fn pick_next_splits_biggest_part_when_neighbour_has_started() {
        let mut d = download_with(plan_parts(Some(10 * MIN_SPLIT_BYTES), true, 2));
        d.parts[0].pos = d.parts[0].end.unwrap();
        d.parts[1].pos += PART_TAKEOVER_BYTES;
        d.parts[1].owner = 4;
        let new = pick_next(&mut d, 0, 9).unwrap();
        assert_eq!(new, 2);
        assert_eq!(d.parts[1].end, Some(d.parts[2].start));
        assert_eq!(d.parts[2].end, Some(10 * MIN_SPLIT_BYTES));
    }

    #[test]
    fn pick_next_prefers_part_left_by_a_refused_connection() {
        let mut d = download_with(plan_parts(Some(10 * MIN_SPLIT_BYTES), true, 3));
        d.parts[0].pos = d.parts[0].end.unwrap();
        d.parts[1].pos += PART_TAKEOVER_BYTES;
        d.parts[1].owner = 4;
        assert_eq!(pick_next(&mut d, 0, 9), Some(2));
        assert_eq!(d.parts[2].owner, 9);
    }

    #[test]
    fn pick_next_small_remainder_is_not_split() {
        let mut d = download_with(vec![Part { start: 0, end: Some(MIN_SPLIT_BYTES), pos: MIN_SPLIT_BYTES, owner: 1, info: String::new() }, Part { start: MIN_SPLIT_BYTES, end: Some(2 * MIN_SPLIT_BYTES), pos: MIN_SPLIT_BYTES + 100, owner: 2, info: String::new() }]);
        assert_eq!(pick_next(&mut d, 0, 3), None);
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;

    /// Needs internet: `IDM_TEST_URL=<url> cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn start_real_url_downloads_whole_file_with_several_parts() {
        let url = std::env::var("IDM_TEST_URL").expect("IDM_TEST_URL");
        let dir = std::env::temp_dir().join("idm-rebuild-test");
        let _ = fs::remove_dir_all(&dir);
        let mut d = tests_support::blank(&url, &dir);
        d.connections = 8;
        let h = new_handle(d);
        start(&h);
        let started = Instant::now();
        let mut max_parts = 0;
        loop {
            thread::sleep(Duration::from_millis(200));
            let d = h.d.lock().unwrap();
            max_parts = max_parts.max(d.parts.len());
            if !d.is_running() {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(600), "too slow");
        }
        let d = h.d.lock().unwrap();
        assert_eq!(d.status, Status::Complete, "{}", d.message);
        assert_eq!(fs::metadata(d.final_path()).unwrap().len(), d.size.unwrap());
        assert!(max_parts >= 8, "parts: {max_parts}");
        println!("saved {} parts={} in {:?}", d.final_path().display(), d.parts.len(), started.elapsed());
    }
}

#[cfg(test)]
pub mod tests_support {
    use super::*;

    pub fn blank(url: &str, dir: &std::path::Path) -> Download {
        Download {
            id: 1,
            url: url.into(),
            file_name: crate::net::file_name_from_url(url),
            save_dir: dir.to_path_buf(),
            category: Category::General,
            size: None,
            resumable: false,
            parts: Vec::new(),
            status: Status::Stopped,
            last_try: String::new(),
            description: String::new(),
            connections: DEFAULT_CONNECTIONS,
            speed_limit_kbps: None,
            in_queue: false,
            queue_id: 0,
            referer: String::new(),
            user_agent: String::new(),
            hls: None,
            temp_dir: None,
            server_date: None,
            message: String::new(),
            last_speed: 0.0,
            speed: 0.0,
            active_connections: 0,
        }
    }
}
