use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Values from MECHANICS.md (the decompiled original); keep both in sync.
pub const PART_TAKEOVER_BYTES: u64 = 50; // function 0x5a4df0
pub const MIN_STORED_SPEED: f64 = 100.0; // function 0x5ba210
pub const DEFAULT_CONNECTIONS: u32 = 8; // estimated
pub const CONNECTION_CHOICES: [u32; 7] = [1, 2, 4, 8, 16, 24, 32]; // estimated
pub const DEFAULT_MAX_AT_ONCE: u32 = 4; // estimated
pub const MIN_SPLIT_BYTES: u64 = 256 * 1024; // estimated: smallest half a part may be cut into
pub const MAX_RETRIES: u32 = 5; // estimated

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Compressed,
    Documents,
    Music,
    Programs,
    Video,
    General,
}

impl Category {
    pub const ALL: [Category; 5] = [Category::Compressed, Category::Documents, Category::Music, Category::Programs, Category::Video];

    pub fn name(self) -> &'static str {
        match self {
            Category::Compressed => "Compressed",
            Category::Documents => "Documents",
            Category::Music => "Music",
            Category::Programs => "Programs",
            Category::Video => "Video",
            Category::General => "General",
        }
    }

    /// Default extension lists, function 0x4c94f0.
    pub fn extensions(self) -> &'static str {
        match self {
            Category::Compressed => "zip rar r0* r1* arj gz sit sitx sea ace bz2 7z",
            Category::Documents => "doc pdf ppt pps docx pptx",
            Category::Music => "mp3 wav wma mpa ram ra aac aif m4a tsa",
            Category::Programs => "exe msi",
            Category::Video => "avi mpg mpe mpeg asf wmv mov qt rm mp4 flv m4v webm ogv ogg mkv ts tsv",
            Category::General => "",
        }
    }

    pub fn for_file(file_name: &str) -> Category {
        let ext = match file_name.rsplit_once('.') {
            Some((_, e)) => e.to_ascii_lowercase(),
            None => return Category::General,
        };
        Category::ALL
            .into_iter()
            .find(|c| c.extensions().split(' ').any(|p| extension_matches(p, &ext)))
            .unwrap_or(Category::General)
    }
}

/// `r0*` style patterns match any extension with that prefix.
pub fn extension_matches(pattern: &str, ext: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => ext.starts_with(prefix),
        None => pattern == ext,
    }
}

pub fn is_known_file_type(file_name: &str) -> bool {
    Category::for_file(file_name) != Category::General
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Stopped,
    Connecting,
    Downloading,
    Complete,
    Error,
}

/// One byte range of the file; `end` is exclusive, `None` while the size is unknown.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Part {
    pub start: u64,
    pub end: Option<u64>,
    pub pos: u64,
    #[serde(skip)]
    pub owner: u64,
    #[serde(skip)]
    pub info: String,
}

impl Part {
    pub fn remaining(&self) -> Option<u64> {
        self.end.map(|e| e.saturating_sub(self.pos))
    }

    pub fn is_done(&self) -> bool {
        matches!(self.remaining(), Some(0))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Download {
    pub id: u64,
    pub url: String,
    pub file_name: String,
    pub save_dir: PathBuf,
    pub category: Category,
    pub size: Option<u64>,
    pub resumable: bool,
    pub parts: Vec<Part>,
    pub status: Status,
    pub last_try: String,
    pub description: String,
    pub connections: u32,
    pub speed_limit_kbps: Option<u32>,
    pub in_queue: bool,
    pub message: String,
    pub last_speed: f64,
    #[serde(skip)]
    pub speed: f64,
    #[serde(skip)]
    pub active_connections: u32,
}

impl Download {
    pub fn downloaded(&self) -> u64 {
        self.parts.iter().map(|p| p.pos - p.start).sum()
    }

    pub fn final_path(&self) -> PathBuf {
        self.save_dir.join(&self.file_name)
    }

    pub fn temp_path(&self) -> PathBuf {
        self.save_dir.join(format!("{}.part", self.file_name))
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, Status::Connecting | Status::Downloading)
    }

    pub fn progress(&self) -> Option<f32> {
        match (self.status, self.size) {
            (Status::Complete, _) => Some(1.0),
            (_, Some(s)) if s > 0 => Some(self.downloaded() as f32 / s as f32),
            _ => None,
        }
    }

    pub fn time_left_secs(&self) -> Option<u64> {
        let size = self.size?;
        if self.speed < 1.0 || !self.is_running() {
            return None;
        }
        Some((size.saturating_sub(self.downloaded()) as f64 / self.speed) as u64)
    }
}

/// Shared between the window and the download threads.
pub struct Shared {
    pub d: Mutex<Download>,
    pub stop: AtomicBool,
    pub limiter: Mutex<Limiter>,
}

pub type Handle = Arc<Shared>;

pub fn new_handle(d: Download) -> Handle {
    Arc::new(Shared { d: Mutex::new(d), stop: AtomicBool::new(false), limiter: Mutex::new(Limiter::default()) })
}

#[derive(Default)]
pub struct Limiter {
    pub tokens: f64,
    pub last: Option<std::time::Instant>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Dark,
    Light,
}

impl Default for Theme {
    fn default() -> Self { Theme::Dark }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    pub base_dir: PathBuf,
    pub connections: u32,
    pub max_at_once: u32,
    pub watch_clipboard: bool,
    pub start_at_enabled: bool,
    pub start_at: String,
    pub stop_at_enabled: bool,
    pub stop_at: String,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub run_on_startup: bool,
    #[serde(default)]
    pub category_dirs: HashMap<String, PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        Settings {
            base_dir: home.join("Downloads"),
            connections: DEFAULT_CONNECTIONS,
            max_at_once: DEFAULT_MAX_AT_ONCE,
            watch_clipboard: true,
            start_at_enabled: false,
            start_at: "02:00".into(),
            stop_at_enabled: false,
            stop_at: "08:00".into(),
            theme: Theme::Dark,
            run_on_startup: false,
            category_dirs: HashMap::new(),
        }
    }
}

impl Settings {
    /// One folder per category; respects per-category overrides.
    pub fn folder_for(&self, c: Category) -> PathBuf {
        if let Some(p) = self.category_dirs.get(c.name()) {
            return p.clone();
        }
        match c {
            Category::General => self.base_dir.clone(),
            other => self.base_dir.join(other.name()),
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct SavedState {
    pub settings: Settings,
    pub downloads: Vec<Download>,
    pub next_id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_for_file_known_extension_matches_original_lists() {
        assert_eq!(Category::for_file("a.ZIP"), Category::Compressed);
        assert_eq!(Category::for_file("movie.part.r01"), Category::Compressed);
        assert_eq!(Category::for_file("song.mp3"), Category::Music);
        assert_eq!(Category::for_file("setup.msi"), Category::Programs);
        assert_eq!(Category::for_file("clip.webm"), Category::Video);
        assert_eq!(Category::for_file("paper.pdf"), Category::Documents);
    }

    #[test]
    fn category_for_file_unknown_or_missing_extension_is_general() {
        assert_eq!(Category::for_file("readme"), Category::General);
        assert_eq!(Category::for_file("page.html"), Category::General);
        assert_eq!(Category::for_file(""), Category::General);
    }
}
