//! MPEG-DASH (`.mpd`) downloads: reads the manifest, downloads the best video and audio
//! tracks and merges them into one fragmented MP4 (no ffmpeg needed).
use crate::hls::{fetch, finish};
use crate::model::*;
use crate::net;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

const PARALLEL_SEGMENTS: usize = 4;
const MAX_SEGMENT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

pub fn is_dash_url(url: &str) -> bool {
    url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase().ends_with(".mpd")
}

// ── Minimal XML reader (enough for MPD manifests) ────────────────────────────

#[derive(Debug, Default, Clone)]
struct Node {
    name: String,
    attrs: HashMap<String, String>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.get(k).map(String::as_str)
    }
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }
}

fn local_name(n: &str) -> &str {
    n.rsplit(':').next().unwrap_or(n)
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn parse_tag(inner: &str) -> (String, HashMap<String, String>) {
    let inner = inner.trim_end_matches('/').trim();
    let name_end = inner.find(char::is_whitespace).unwrap_or(inner.len());
    let name = local_name(&inner[..name_end]).to_string();
    let mut attrs = HashMap::new();
    let mut rest = &inner[name_end..];
    while let Some(eq) = rest.find('=') {
        let key = local_name(rest[..eq].trim()).to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
        let Some(end) = after[1..].find(q) else { break };
        attrs.insert(key, unescape(&after[1..1 + end]));
        rest = &after[end + 2..];
    }
    (name, attrs)
}

fn parse_xml(text: &str) -> Result<Node, String> {
    let mut stack: Vec<Node> = vec![Node::default()];
    let mut i = 0;
    while let Some(lt) = text[i..].find('<') {
        let start = i + lt;
        let chunk = text[i..start].trim();
        if !chunk.is_empty() {
            stack.last_mut().unwrap().text.push_str(&unescape(chunk));
        }
        let rest = &text[start..];
        if rest.starts_with("<!--") {
            i = start + rest.find("-->").ok_or("Broken manifest")? + 3;
        } else if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").ok_or("Broken manifest")?;
            stack.last_mut().unwrap().text.push_str(&rest[9..end]);
            i = start + end + 3;
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            i = start + rest.find('>').ok_or("Broken manifest")? + 1;
        } else if rest.starts_with("</") {
            let end = rest.find('>').ok_or("Broken manifest")?;
            let node = stack.pop().ok_or("Broken manifest")?;
            stack.last_mut().ok_or("Broken manifest")?.children.push(node);
            i = start + end + 1;
        } else {
            // find the closing '>' outside quotes
            let (mut quote, mut end) = (None, None);
            for (off, c) in rest.char_indices().skip(1) {
                match (quote, c) {
                    (None, '"' | '\'') => quote = Some(c),
                    (Some(q), c) if c == q => quote = None,
                    (None, '>') => { end = Some(off); break; }
                    _ => {}
                }
            }
            let end = end.ok_or("Broken manifest")?;
            let inner = &rest[1..end];
            let (name, attrs) = parse_tag(inner);
            let node = Node { name, attrs, ..Default::default() };
            if inner.ends_with('/') {
                stack.last_mut().unwrap().children.push(node);
            } else {
                stack.push(node);
            }
            i = start + end + 1;
        }
    }
    if stack.len() != 1 {
        return Err("Broken manifest".into());
    }
    stack.pop().unwrap().children.into_iter().next().ok_or_else(|| "Empty manifest".to_string())
}

// ── Manifest → tracks ────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Kind {
    Video,
    Audio,
}

#[derive(Debug)]
pub struct Track {
    pub kind: Kind,
    pub bandwidth: u64,
    /// fMP4 initialization segment (segmented tracks).
    pub init: Option<String>,
    pub segments: Vec<String>,
    /// A whole-file track (`BaseURL` only).
    pub single: Option<String>,
}

/// ISO-8601 duration such as `PT1H2M3.5S`, in seconds.
fn parse_duration(s: &str) -> Option<f64> {
    let s = s.strip_prefix('P')?;
    let (date, time) = s.split_once('T').unwrap_or((s, ""));
    let mut total = 0.0;
    let mut num = String::new();
    for c in date.chars() {
        match c {
            '0'..='9' | '.' => num.push(c),
            'D' => { total += num.parse::<f64>().ok()? * 86400.0; num.clear(); }
            _ => return None,
        }
    }
    for c in time.chars() {
        match c {
            '0'..='9' | '.' => num.push(c),
            'H' => { total += num.parse::<f64>().ok()? * 3600.0; num.clear(); }
            'M' => { total += num.parse::<f64>().ok()? * 60.0; num.clear(); }
            'S' => { total += num.parse::<f64>().ok()?; num.clear(); }
            _ => return None,
        }
    }
    Some(total)
}

/// Fills `$RepresentationID$`, `$Number$`, `$Time$`, `$Bandwidth$` (with optional `%0Nd`).
fn expand(template: &str, id: &str, bandwidth: u64, number: u64, time: u64) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(j) = after.find('$') else { out.push_str(&rest[i..]); return out };
        let token = &after[..j];
        if token.is_empty() {
            out.push('$');
        } else {
            let (name, fmt) = token.split_once('%').map(|(n, f)| (n, Some(f))).unwrap_or((token, None));
            let value = match name {
                "RepresentationID" => id.to_string(),
                "Number" => pad(number, fmt),
                "Time" => pad(time, fmt),
                "Bandwidth" => pad(bandwidth, fmt),
                other => format!("${other}$"),
            };
            out.push_str(&value);
        }
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    out
}

fn pad(v: u64, fmt: Option<&str>) -> String {
    let width = fmt.and_then(|f| f.trim_start_matches('0').trim_end_matches('d').parse::<usize>().ok()).unwrap_or(0);
    format!("{v:0width$}")
}

fn base_url(node: &Node, parent: &str) -> String {
    match node.child("BaseURL") {
        Some(b) if !b.text.trim().is_empty() => net::join_url(parent, b.text.trim()),
        _ => parent.to_string(),
    }
}

pub fn parse_manifest(text: &str, mpd_url: &str) -> Result<Vec<Track>, String> {
    let mpd = parse_xml(text)?;
    if mpd.name != "MPD" {
        return Err("This address is not a DASH manifest".into());
    }
    if mpd.attr("type") == Some("dynamic") {
        return Err("Live streams are not supported".into());
    }
    let total_secs = mpd.attr("mediaPresentationDuration").and_then(parse_duration);
    let mpd_base = base_url(&mpd, mpd_url);
    let mut tracks = Vec::new();
    for period in mpd.all("Period") {
        let period_base = base_url(period, &mpd_base);
        let period_secs = period.attr("duration").and_then(parse_duration).or(total_secs);
        for set in period.all("AdaptationSet") {
            if set.child("ContentProtection").is_some() || set.all("Representation").any(|r| r.child("ContentProtection").is_some()) {
                return Err("Protected (DRM) streams are not supported".into());
            }
            let set_base = base_url(set, &period_base);
            for rep in set.all("Representation") {
                let mime = rep.attr("mimeType").or(set.attr("mimeType")).or(set.attr("contentType")).unwrap_or("");
                let kind = if mime.starts_with("video") {
                    Kind::Video
                } else if mime.starts_with("audio") {
                    Kind::Audio
                } else {
                    continue;
                };
                let id = rep.attr("id").unwrap_or("");
                let bandwidth: u64 = rep.attr("bandwidth").and_then(|b| b.parse().ok()).unwrap_or(0);
                let rep_base = base_url(rep, &set_base);
                let template = rep.child("SegmentTemplate").or_else(|| set.child("SegmentTemplate"));
                let track = match template {
                    Some(t) => {
                        let timescale: u64 = t.attr("timescale").and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
                        let start: u64 = t.attr("startNumber").and_then(|v| v.parse().ok()).unwrap_or(1);
                        let media = t.attr("media").ok_or("The manifest has no segment address")?;
                        let mut segments = Vec::new();
                        if let Some(tl) = t.child("SegmentTimeline") {
                            let (mut time, mut number) = (0u64, start);
                            for s in tl.all("S") {
                                if let Some(tt) = s.attr("t").and_then(|v| v.parse().ok()) {
                                    time = tt;
                                }
                                let d: u64 = s.attr("d").and_then(|v| v.parse().ok()).ok_or("Bad segment timeline")?;
                                let r: i64 = s.attr("r").and_then(|v| v.parse().ok()).unwrap_or(0);
                                if r < 0 {
                                    return Err("Open-ended segment timelines are not supported".into());
                                }
                                for _ in 0..=r {
                                    segments.push(net::join_url(&rep_base, &expand(media, id, bandwidth, number, time)));
                                    time += d;
                                    number += 1;
                                }
                            }
                        } else {
                            let d: u64 = t.attr("duration").and_then(|v| v.parse().ok()).ok_or("The manifest has no segment duration")?;
                            let secs = period_secs.ok_or("The manifest has no total duration")?;
                            let count = ((secs * timescale as f64) / d as f64).ceil() as u64;
                            for n in 0..count {
                                segments.push(net::join_url(&rep_base, &expand(media, id, bandwidth, start + n, n * d)));
                            }
                        }
                        let init = t.attr("initialization").map(|i| net::join_url(&rep_base, &expand(i, id, bandwidth, 0, 0)));
                        Track { kind, bandwidth, init, segments, single: None }
                    }
                    None if rep.child("BaseURL").is_some() => Track { kind, bandwidth, init: None, segments: Vec::new(), single: Some(rep_base) },
                    None => continue,
                };
                tracks.push(track);
            }
        }
    }
    if tracks.is_empty() {
        return Err("The manifest has no downloadable video or audio".into());
    }
    Ok(tracks)
}

fn best(tracks: &mut Vec<Track>, kind: Kind) -> Option<Track> {
    let idx = tracks.iter().enumerate().filter(|(_, t)| t.kind == kind).max_by_key(|(_, t)| t.bandwidth).map(|(i, _)| i)?;
    Some(tracks.remove(idx))
}

// ── MP4 boxes and merging two fragmented tracks ──────────────────────────────

type BoxType = [u8; 4];

fn make_box(kind: &BoxType, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}

/// Splits a buffer into its boxes (type, payload).
fn children(data: &[u8]) -> Result<Vec<(BoxType, Vec<u8>)>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 8 <= data.len() {
        let size = u32::from_be_bytes(data[i..i + 4].try_into().unwrap()) as usize;
        let kind: BoxType = data[i + 4..i + 8].try_into().unwrap();
        if size < 8 || i + size > data.len() {
            return Err("Damaged MP4 data".into());
        }
        out.push((kind, data[i + 8..i + size].to_vec()));
        i += size;
    }
    Ok(out)
}

fn join(parts: Vec<(BoxType, Vec<u8>)>) -> Vec<u8> {
    parts.iter().flat_map(|(k, p)| make_box(k, p)).collect()
}

fn set_u32(buf: &mut [u8], at: usize, v: u32) -> Result<(), String> {
    buf.get_mut(at..at + 4).ok_or("Damaged MP4 data")?.copy_from_slice(&v.to_be_bytes());
    Ok(())
}

#[cfg(test)]
fn get_u32(buf: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(buf.get(at..at + 4)?.try_into().ok()?))
}

/// Sets the track id of a `trak` (inside `tkhd`).
fn patch_trak(payload: &[u8], id: u32) -> Result<Vec<u8>, String> {
    let mut parts = children(payload)?;
    for (k, p) in parts.iter_mut() {
        if k == b"tkhd" {
            let at = if p.first() == Some(&1) { 4 + 16 } else { 4 + 8 };
            set_u32(p, at, id)?;
        }
    }
    Ok(join(parts))
}

#[cfg(test)]
fn track_id_of(trak: &[u8]) -> Option<u32> {
    let parts = children(trak).ok()?;
    let tkhd = &parts.iter().find(|(k, _)| k == b"tkhd")?.1;
    get_u32(tkhd, if tkhd.first() == Some(&1) { 4 + 16 } else { 4 + 8 })
}

fn patch_trex(p: &mut [u8], id: u32) -> Result<(), String> {
    set_u32(p, 4, id)
}

/// Combines the init segments of one video and one audio track into a single `ftyp + moov`.
pub fn merge_init(video: &[u8], audio: &[u8]) -> Result<Vec<u8>, String> {
    let v = children(video)?;
    let a = children(audio)?;
    let ftyp = v.iter().find(|(k, _)| k == b"ftyp").ok_or("The video track has no header")?.clone();
    let vmoov = children(&v.iter().find(|(k, _)| k == b"moov").ok_or("The video track has no header")?.1)?;
    let amoov = children(&a.iter().find(|(k, _)| k == b"moov").ok_or("The audio track has no header")?.1)?;
    let atrak = amoov.iter().find(|(k, _)| k == b"trak").ok_or("The audio track has no header")?;
    let atrex = children(&amoov.iter().find(|(k, _)| k == b"mvex").ok_or("The audio track has no fragment header")?.1)?
        .into_iter()
        .find(|(k, _)| k == b"trex")
        .ok_or("The audio track has no fragment header")?;

    let mut out_moov = Vec::new();
    for (k, p) in vmoov {
        match &k {
            b"mvhd" => {
                let mut p = p;
                // next_track_id is the last field of mvhd
                let at = p.len().checked_sub(4).ok_or("Damaged MP4 data")?;
                set_u32(&mut p, at, 3)?;
                out_moov.push((k, p));
            }
            b"trak" => {
                out_moov.push((k, patch_trak(&p, 1)?));
                out_moov.push((*b"trak", patch_trak(&atrak.1, 2)?));
            }
            b"mvex" => {
                let mut parts = children(&p)?;
                for (ck, cp) in parts.iter_mut() {
                    if ck == b"trex" {
                        patch_trex(cp, 1)?;
                    }
                }
                let mut at = atrex.1.clone();
                patch_trex(&mut at, 2)?;
                parts.push((*b"trex", at));
                out_moov.push((k, join(parts)));
            }
            _ => out_moov.push((k, p)),
        }
    }
    let mut out = make_box(b"ftyp", &ftyp.1);
    out.extend(make_box(b"moov", &join(out_moov)));
    Ok(out)
}

/// Rewrites a `moof` so that its track id and sequence number fit the merged file.
pub fn patch_fragment(moof_payload: &[u8], track_id: u32, sequence: u32) -> Result<Vec<u8>, String> {
    let mut parts = children(moof_payload)?;
    for (k, p) in parts.iter_mut() {
        match &*k {
            b"mfhd" => set_u32(p, 4, sequence)?,
            b"traf" => {
                let mut inner = children(p)?;
                for (ik, ip) in inner.iter_mut() {
                    if ik == b"tfhd" {
                        set_u32(ip, 4, track_id)?;
                    }
                }
                *p = join(inner);
            }
            _ => {}
        }
    }
    Ok(join(parts))
}

fn read_box<R: Read>(r: &mut R) -> Result<Option<(BoxType, Vec<u8>)>, String> {
    let mut head = [0u8; 8];
    match r.read_exact(&mut head) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    let size = u32::from_be_bytes(head[..4].try_into().unwrap()) as u64;
    let kind: BoxType = head[4..].try_into().unwrap();
    if size < 8 {
        return Err("Unsupported MP4 box size".into());
    }
    let mut payload = vec![0u8; (size - 8) as usize];
    r.read_exact(&mut payload).map_err(|e| e.to_string())?;
    Ok(Some((kind, payload)))
}

/// Next `(moof payload, mdat payload)` pair; `styp`/`sidx` boxes are skipped.
fn next_fragment<R: Read>(r: &mut R) -> Result<Option<(Vec<u8>, Vec<u8>)>, String> {
    let mut moof = None;
    while let Some((kind, payload)) = read_box(r)? {
        match &kind {
            b"moof" => moof = Some(payload),
            b"mdat" => {
                if let Some(m) = moof.take() {
                    return Ok(Some((m, payload)));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

/// Writes `ftyp + moov` then the video and audio fragments alternately.
fn mux<R1: Read, R2: Read, W: Write>(video_init: &[u8], audio_init: &[u8], video: &mut R1, audio: &mut R2, out: &mut W) -> Result<u64, String> {
    let init = merge_init(video_init, audio_init)?;
    out.write_all(&init).map_err(|e| e.to_string())?;
    let mut written = init.len() as u64;
    let mut seq = 1u32;
    loop {
        let v = next_fragment(video)?;
        let a = next_fragment(audio)?;
        if v.is_none() && a.is_none() {
            break;
        }
        for (frag, id) in [(v, 1u32), (a, 2u32)] {
            if let Some((moof, mdat)) = frag {
                let patched = make_box(b"moof", &patch_fragment(&moof, id, seq)?);
                seq += 1;
                let data = make_box(b"mdat", &mdat);
                out.write_all(&patched).map_err(|e| e.to_string())?;
                out.write_all(&data).map_err(|e| e.to_string())?;
                written += (patched.len() + data.len()) as u64;
            }
        }
    }
    Ok(written)
}

// ── Download ─────────────────────────────────────────────────────────────────

fn read_init(path: &Path) -> Result<Vec<u8>, String> {
    // ftyp + moov are at the start of the track file
    let mut r = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
    let mut out = Vec::new();
    while let Some((kind, payload)) = read_box(&mut r)? {
        out.extend(make_box(&kind, &payload));
        if &kind == b"moov" {
            return Ok(out);
        }
    }
    Err("The track has no header".into())
}

fn bump(h: &Handle, bytes: u64) {
    let mut d = h.d.lock().unwrap();
    if let Some(s) = d.hls.as_mut() {
        s.bytes += bytes;
    }
}

/// Downloads one track (init + segments in order) into `path`; returns false when stopped.
fn download_track(h: &Handle, t: &Track, path: &Path) -> Result<bool, String> {
    let mut file = BufWriter::new(File::create(path).map_err(|e| format!("Cannot create file: {e}"))?);
    if let Some(single) = &t.single {
        let resp = net::get(single).call().map_err(net::describe_error)?;
        let mut reader = resp.into_reader();
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            if h.stop.load(Ordering::SeqCst) {
                return Ok(false);
            }
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            bump(h, n as u64);
        }
        file.flush().map_err(|e| e.to_string())?;
        h.d.lock().unwrap().hls.as_mut().unwrap().done += 1;
        return Ok(true);
    }
    if let Some(init) = &t.init {
        let data = fetch(init, MAX_SEGMENT_BYTES)?;
        file.write_all(&data).map_err(|e| e.to_string())?;
        bump(h, data.len() as u64);
    }
    for batch in t.segments.chunks(PARALLEL_SEGMENTS) {
        if h.stop.load(Ordering::SeqCst) {
            return Ok(false);
        }
        h.d.lock().unwrap().active_connections = batch.len() as u32;
        let results: Vec<Result<Vec<u8>, String>> = std::thread::scope(|scope| {
            let jobs: Vec<_> = batch.iter().map(|u| scope.spawn(move || fetch(u, MAX_SEGMENT_BYTES))).collect();
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
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(true)
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
    let text = String::from_utf8(fetch(&url, MAX_MANIFEST_BYTES)?).map_err(|_| "The manifest is not text".to_string())?;
    let mut tracks = parse_manifest(&text, &url)?;
    let video = best(&mut tracks, Kind::Video);
    let audio = best(&mut tracks, Kind::Audio);
    if video.is_none() && audio.is_none() {
        return Err("The manifest has no downloadable video or audio".into());
    }
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create folder: {e}"))?;
    if let Some(parent) = temp.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create folder: {e}"))?;
    }
    let units = |t: &Option<Track>| t.as_ref().map_or(0, |t| if t.single.is_some() { 1 } else { t.segments.len() });
    {
        let mut d = h.d.lock().unwrap();
        d.hls = Some(HlsState { total: units(&video) + units(&audio), done: 0, bytes: 0, fragmented: true });
        d.resumable = false;
    }
    let part = |ext: &str| PathBuf::from(format!("{}.{ext}", temp.display()));
    let (vpath, apath) = (part("v"), part("a"));
    let cleanup = || {
        let _ = fs::remove_file(&vpath);
        let _ = fs::remove_file(&apath);
    };

    let result = (|| -> Result<(), String> {
        let mut stopped = false;
        if let Some(v) = &video {
            stopped = !download_track(h, v, &vpath)?;
        }
        if !stopped {
            if let Some(a) = &audio {
                stopped = !download_track(h, a, &apath)?;
            }
        }
        if stopped {
            // keep done < total so the download shows as stopped
            h.d.lock().unwrap().hls.as_mut().unwrap().done = 0;
            return Ok(());
        }
        let segmented = |t: &Option<Track>| t.as_ref().is_some_and(|t| t.single.is_none());
        match (&video, &audio) {
            (Some(_), Some(_)) if segmented(&video) && segmented(&audio) => {
                let (vi, ai) = (read_init(&vpath)?, read_init(&apath)?);
                let mut vr = BufReader::new(File::open(&vpath).map_err(|e| e.to_string())?);
                let mut ar = BufReader::new(File::open(&apath).map_err(|e| e.to_string())?);
                // skip the init boxes already read
                skip_init(&mut vr)?;
                skip_init(&mut ar)?;
                let mut out = BufWriter::new(File::create(&temp).map_err(|e| e.to_string())?);
                mux(&vi, &ai, &mut vr, &mut ar, &mut out)?;
                out.flush().map_err(|e| e.to_string())?;
            }
            (Some(_), Some(_)) => {
                // whole-file tracks cannot be merged here: keep the video and save the audio beside it
                fs::rename(&vpath, &temp).map_err(|e| e.to_string())?;
                let stem = h.d.lock().unwrap().file_name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_default();
                let target = crate::hls::unique_path(dir.join(format!("{stem} (audio).m4a")));
                fs::rename(&apath, target).map_err(|e| e.to_string())?;
            }
            (Some(_), None) => fs::rename(&vpath, &temp).map_err(|e| e.to_string())?,
            (None, Some(_)) => fs::rename(&apath, &temp).map_err(|e| e.to_string())?,
            (None, None) => {}
        }
        let mut d = h.d.lock().unwrap();
        let total = d.hls.as_ref().map_or(0, |s| s.total);
        if let Some(s) = d.hls.as_mut() {
            s.done = total;
        }
        Ok(())
    })();
    cleanup();
    result
}

fn skip_init<R: Read>(r: &mut R) -> Result<(), String> {
    // ftyp, then everything up to and including moov
    while let Some((kind, _)) = read_box(r)? {
        if &kind == b"moov" {
            return Ok(());
        }
    }
    Err("The track has no header".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_dash_url_checks_path() {
        assert!(is_dash_url("https://a.com/x/manifest.mpd?token=1"));
        assert!(!is_dash_url("https://a.com/x.mp4"));
    }

    #[test]
    fn expand_fills_tokens_with_padding() {
        assert_eq!(expand("seg_$RepresentationID$_$Number%05d$.m4s", "v1", 100, 7, 0), "seg_v1_00007.m4s");
        assert_eq!(expand("t$Time$-$Bandwidth$-$$", "a", 64000, 1, 9000), "t9000-64000-$");
    }

    #[test]
    fn parse_duration_reads_iso_values() {
        assert_eq!(parse_duration("PT1H2M3.5S"), Some(3723.5));
        assert_eq!(parse_duration("PT30S"), Some(30.0));
        assert_eq!(parse_duration("bad"), None);
    }

    const MPD: &str = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT20S">
  <BaseURL>https://cdn.example.com/video/</BaseURL>
  <Period>
    <AdaptationSet mimeType="video/mp4">
      <SegmentTemplate timescale="1000" duration="4000" startNumber="1" initialization="$RepresentationID$/init.mp4" media="$RepresentationID$/seg-$Number$.m4s"/>
      <Representation id="v-low" bandwidth="500000"/>
      <Representation id="v-high" bandwidth="2000000"/>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4">
      <Representation id="a1" bandwidth="96000">
        <SegmentTemplate timescale="48000" initialization="a1/init.mp4" media="a1/$Time$.m4s">
          <SegmentTimeline><S t="0" d="192000" r="1"/><S d="96000"/></SegmentTimeline>
        </SegmentTemplate>
      </Representation>
    </AdaptationSet>
    <AdaptationSet mimeType="text/vtt"><Representation id="t" bandwidth="1"><BaseURL>sub.vtt</BaseURL></Representation></AdaptationSet>
  </Period>
</MPD>"#;

    #[test]
    fn parse_manifest_builds_segment_lists_for_video_and_audio() {
        let mut tracks = parse_manifest(MPD, "https://a.com/m.mpd").unwrap();
        let video = best(&mut tracks, Kind::Video).unwrap();
        assert_eq!(video.bandwidth, 2_000_000);
        assert_eq!(video.init.as_deref(), Some("https://cdn.example.com/video/v-high/init.mp4"));
        assert_eq!(video.segments.len(), 5);
        assert_eq!(video.segments[0], "https://cdn.example.com/video/v-high/seg-1.m4s");
        assert_eq!(video.segments[4], "https://cdn.example.com/video/v-high/seg-5.m4s");
        let audio = best(&mut tracks, Kind::Audio).unwrap();
        assert_eq!(audio.segments, vec![
            "https://cdn.example.com/video/a1/0.m4s".to_string(),
            "https://cdn.example.com/video/a1/192000.m4s".to_string(),
            "https://cdn.example.com/video/a1/384000.m4s".to_string(),
        ]);
    }

    #[test]
    fn parse_manifest_rejects_live_and_protected_streams() {
        assert!(parse_manifest("<MPD type=\"dynamic\"></MPD>", "https://a.com/m.mpd").unwrap_err().contains("Live"));
        let drm = "<MPD type=\"static\"><Period><AdaptationSet mimeType=\"video/mp4\"><ContentProtection schemeIdUri=\"x\"/></AdaptationSet></Period></MPD>";
        assert!(parse_manifest(drm, "https://a.com/m.mpd").unwrap_err().contains("DRM"));
        assert!(parse_manifest("<html></html>", "https://a.com/m.mpd").is_err());
    }

    // ---- MP4 merging with small synthetic boxes ----

    fn tkhd(id: u32) -> Vec<u8> {
        let mut p = vec![0u8; 4 + 8];
        p.extend_from_slice(&id.to_be_bytes());
        p.extend_from_slice(&[0u8; 60]);
        make_box(b"tkhd", &p)
    }

    fn init(track_id: u32) -> Vec<u8> {
        let mut mvhd = vec![0u8; 100];
        let n = mvhd.len();
        mvhd[n - 4..].copy_from_slice(&(track_id + 1).to_be_bytes());
        let trak = make_box(b"trak", &tkhd(track_id));
        let mut trex = vec![0u8; 4];
        trex.extend_from_slice(&track_id.to_be_bytes());
        trex.extend_from_slice(&[0u8; 16]);
        let mvex = make_box(b"mvex", &make_box(b"trex", &trex));
        let moov = make_box(b"moov", &[make_box(b"mvhd", &mvhd), trak, mvex].concat());
        [make_box(b"ftyp", b"isom0000"), moov].concat()
    }

    fn fragment(track_id: u32, seq: u32, data: &[u8]) -> Vec<u8> {
        let mut mfhd = vec![0u8; 4];
        mfhd.extend_from_slice(&seq.to_be_bytes());
        let mut tfhd = vec![0u8; 4];
        tfhd.extend_from_slice(&track_id.to_be_bytes());
        let traf = make_box(b"traf", &make_box(b"tfhd", &tfhd));
        [make_box(b"styp", b"msdh"), make_box(b"moof", &[make_box(b"mfhd", &mfhd), traf].concat()), make_box(b"mdat", data)].concat()
    }

    fn find_box(data: &[u8], path: &[&BoxType]) -> Vec<u8> {
        let mut cur = data.to_vec();
        for k in path {
            cur = children(&cur).unwrap().into_iter().find(|(t, _)| t == *k).unwrap().1;
        }
        cur
    }

    #[test]
    fn merge_init_gives_two_tracks_with_distinct_ids() {
        let merged = merge_init(&init(1), &init(1)).unwrap();
        let moov = find_box(&merged, &[b"moov"]);
        let parts = children(&moov).unwrap();
        let ids: Vec<u32> = parts.iter().filter(|(k, _)| k == b"trak").map(|(_, p)| track_id_of(p).unwrap()).collect();
        assert_eq!(ids, vec![1, 2]);
        let mvhd = &parts.iter().find(|(k, _)| k == b"mvhd").unwrap().1;
        assert_eq!(get_u32(mvhd, mvhd.len() - 4), Some(3));
        let mvex = children(&parts.iter().find(|(k, _)| k == b"mvex").unwrap().1).unwrap();
        let trex_ids: Vec<u32> = mvex.iter().filter(|(k, _)| k == b"trex").map(|(_, p)| get_u32(p, 4).unwrap()).collect();
        assert_eq!(trex_ids, vec![1, 2]);
    }

    #[test]
    fn mux_interleaves_fragments_with_new_track_ids_and_sequence() {
        let video = [fragment(1, 1, b"VVVV"), fragment(1, 2, b"WWWW")].concat();
        let audio = fragment(1, 1, b"AA");
        let mut out = Vec::new();
        mux(&init(1), &init(1), &mut video.as_slice(), &mut audio.as_slice(), &mut out).unwrap();
        // skip ftyp + moov, then read fragments back
        let mut r = out.as_slice();
        skip_init(&mut r).unwrap();
        let mut seen = Vec::new();
        while let Some((moof, mdat)) = next_fragment(&mut r).unwrap() {
            let parts = children(&moof).unwrap();
            let seq = get_u32(&parts.iter().find(|(k, _)| k == b"mfhd").unwrap().1, 4).unwrap();
            let traf = children(&parts.iter().find(|(k, _)| k == b"traf").unwrap().1).unwrap();
            let id = get_u32(&traf.iter().find(|(k, _)| k == b"tfhd").unwrap().1, 4).unwrap();
            seen.push((seq, id, mdat));
        }
        assert_eq!(seen, vec![(1, 1, b"VVVV".to_vec()), (2, 2, b"AA".to_vec()), (3, 1, b"WWWW".to_vec())]);
    }
}
