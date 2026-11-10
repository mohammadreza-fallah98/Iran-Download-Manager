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
pub struct HlsState {
    pub total: usize,
    pub done: usize,
    pub bytes: u64,
    /// Segments are fMP4 (init + fragments), so the result is an .mp4.
    #[serde(default)]
    pub fragmented: bool,
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
    #[serde(default)]
    pub queue_id: u32,
    /// Page the link came from; some sites refuse downloads without it.
    #[serde(default)]
    pub referer: String,
    #[serde(default)]
    pub user_agent: String,
    /// Set for HLS streams: progress is counted in segments.
    #[serde(default)]
    pub hls: Option<HlsState>,
    /// Folder holding the unfinished file; `None` means next to the final file.
    #[serde(default)]
    pub temp_dir: Option<PathBuf>,
    /// `Last-Modified` of the file on the server.
    #[serde(default)]
    pub server_date: Option<String>,
    pub message: String,
    pub last_speed: f64,
    #[serde(skip)]
    pub speed: f64,
    #[serde(skip)]
    pub active_connections: u32,
}

impl Download {
    pub fn downloaded(&self) -> u64 {
        match &self.hls {
            Some(h) => h.bytes,
            None => self.parts.iter().map(|p| p.pos - p.start).sum(),
        }
    }

    pub fn final_path(&self) -> PathBuf {
        self.save_dir.join(&self.file_name)
    }

    pub fn temp_path(&self) -> PathBuf {
        match &self.temp_dir {
            // the id keeps two files with the same name apart in a shared folder
            Some(dir) => dir.join(format!("{}-{}.part", self.id, self.file_name)),
            None => self.save_dir.join(format!("{}.part", self.file_name)),
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, Status::Connecting | Status::Downloading)
    }

    pub fn progress(&self) -> Option<f32> {
        if let Some(h) = self.hls.as_ref().filter(|h| h.total > 0) {
            return Some(if self.status == Status::Complete { 1.0 } else { h.done as f32 / h.total as f32 });
        }
        match (self.status, self.size) {
            (Status::Complete, _) => Some(1.0),
            (_, Some(s)) if s > 0 => Some(self.downloaded() as f32 / s as f32),
            _ => None,
        }
    }

    pub fn time_left_secs(&self) -> Option<u64> {
        if let Some(h) = self.hls.as_ref().filter(|h| h.total > 0 && h.done > 0) {
            // Estimated from the average segment size
            let per_seg = h.bytes as f64 / h.done as f64;
            if self.speed < 1.0 || !self.is_running() {
                return None;
            }
            return Some((per_seg * (h.total - h.done) as f64 / self.speed) as u64);
        }
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
    #[serde(default = "default_true")]
    pub show_start_dialog: bool,
    #[serde(default)]
    pub add_to_queue_only: bool,
    #[serde(default = "default_true")]
    pub show_complete_dialog: bool,
    #[serde(default = "default_auto_extensions")]
    pub auto_extensions: String,
    #[serde(default = "default_excluded_sites")]
    pub excluded_sites: String,
    #[serde(default)]
    pub proxy: ProxyConfig,
    #[serde(default)]
    pub server_limits: Vec<ServerLimit>,
    #[serde(default)]
    pub sounds: Sounds,
    #[serde(default)]
    pub credentials: Vec<Credential>,
    #[serde(default)]
    pub quota: Quota,
    #[serde(default = "default_queues")]
    pub queues: Vec<QueueDef>,
    /// Overall speed limit in KB/s for all downloads together; 0 means no limit.
    #[serde(default)]
    pub global_speed_limit_kbps: u32,
    #[serde(default)]
    pub duplicate_action: DuplicateAction,
    /// Empty means the built-in User-Agent.
    #[serde(default)]
    pub user_agent: String,
    /// Program run on every finished file; `{path}` is replaced by the file path. Empty means none.
    #[serde(default)]
    pub antivirus_cmd: String,
    /// Folder for unfinished `.part` files; empty means next to the final file.
    #[serde(default)]
    pub temp_dir: String,
    #[serde(default)]
    pub use_server_date: bool,
    #[serde(default = "default_true")]
    pub remember_last_dir: bool,
    #[serde(default)]
    pub block_key: BlockKey,
    #[serde(default = "default_true")]
    pub ext_intercept: bool,
    #[serde(default = "default_true")]
    pub ext_video_button: bool,
    #[serde(default)]
    pub language: crate::i18n::Language,
}

/// What to do when the same address is added twice.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum DuplicateAction {
    #[default]
    Ask,
    AutoRename,
    Skip,
}

/// Key that, held while clicking a link, keeps the browser from handing the download over.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum BlockKey {
    #[default]
    Alt,
    Ctrl,
    Shift,
    Off,
}

impl BlockKey {
    pub fn name(self) -> &'static str {
        match self {
            BlockKey::Alt => "Alt",
            BlockKey::Ctrl => "Ctrl",
            BlockKey::Shift => "Shift",
            BlockKey::Off => "None",
        }
    }
}

/// Set from the settings so the engine can use it without a reference to them.
pub static USE_SERVER_DATE: AtomicBool = AtomicBool::new(false);

/// The queue every download without an explicit queue belongs to.
pub const MAIN_QUEUE_ID: u32 = 0;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QueueDef {
    pub id: u32,
    pub name: String,
}

fn default_queues() -> Vec<QueueDef> {
    vec![QueueDef { id: MAIN_QUEUE_ID, name: "Main download queue".into() }]
}

/// Login for sites that ask for a user name and password. On disk only `pass_enc` (DPAPI) is stored.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Credential {
    pub pattern: String,
    pub user: String,
    #[serde(skip)]
    pub pass: String,
    #[serde(default)]
    pub pass_enc: String,
}

/// Limit on how much may be downloaded in a time window.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Quota {
    pub enabled: bool,
    pub megabytes: u64,
    pub hours: u64,
}

impl Default for Quota {
    fn default() -> Self {
        Quota { enabled: false, megabytes: 200, hours: 5 }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ProxyMode {
    #[default]
    None,
    System,
    /// An automatic configuration script (`.pac`), resolved by Windows.
    Pac,
    Manual,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ProxyKind {
    #[default]
    Http,
    Socks5,
}

/// The password is never written to disk.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct ProxyConfig {
    pub mode: ProxyMode,
    #[serde(default)]
    pub kind: ProxyKind,
    #[serde(default)]
    pub pac_url: String,
    pub host: String,
    pub port: String,
    pub user: String,
    #[serde(skip)]
    pub pass: String,
}

/// Max connections for one server (`*.example.com` allowed).
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct ServerLimit {
    pub host: String,
    pub connections: u32,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct SoundSetting {
    pub enabled: bool,
    /// Empty means the system beep.
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Sounds {
    pub complete: SoundSetting,
    pub failed: SoundSetting,
    pub scheduler_start: SoundSetting,
    pub scheduler_stop: SoundSetting,
}

fn default_true() -> bool { true }

pub fn default_auto_extensions() -> String {
    Category::ALL.iter().map(|c| c.extensions()).collect::<Vec<_>>().join(" ")
}

pub fn default_excluded_sites() -> String {
    "*.update.microsoft.com download.windowsupdate.com siteseal.thawte.com ecom.cimetz.com *.voice2page.com karboom.io".into()
}

/// Renames, or copies then deletes when the target is on another drive.
pub fn move_file_any(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    std::fs::remove_file(from)
}

/// Gives the file the date the server reported (`Last-Modified`), when it can be read.
pub fn apply_server_date(path: &std::path::Path, http_date: &str) {
    let Ok(t) = chrono::DateTime::parse_from_rfc2822(http_date.trim()) else { return };
    let secs = t.timestamp();
    if secs <= 0 {
        return;
    }
    let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64);
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(when);
    }
}

pub fn host_of(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or("");
    authority.split(':').next().unwrap_or("").to_ascii_lowercase()
}

/// `*.example.com` matches the domain and all of its subdomains.
pub fn host_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    match pattern.strip_prefix("*.") {
        Some(domain) => host == domain || host.ends_with(&format!(".{domain}")),
        None => host == pattern,
    }
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
            show_start_dialog: true,
            add_to_queue_only: false,
            show_complete_dialog: true,
            auto_extensions: default_auto_extensions(),
            excluded_sites: default_excluded_sites(),
            proxy: ProxyConfig::default(),
            server_limits: Vec::new(),
            sounds: Sounds::default(),
            credentials: Vec::new(),
            quota: Quota::default(),
            queues: default_queues(),
            global_speed_limit_kbps: 0,
            duplicate_action: DuplicateAction::Ask,
            user_agent: String::new(),
            antivirus_cmd: String::new(),
            temp_dir: String::new(),
            use_server_date: false,
            remember_last_dir: true,
            block_key: BlockKey::Alt,
            ext_intercept: true,
            ext_video_button: true,
            language: crate::i18n::Language::English,
        }
    }
}

impl Settings {
    pub fn queue_name(&self, id: u32) -> String {
        self.queues.iter().find(|q| q.id == id).map(|q| q.name.clone()).unwrap_or_else(|| "Main download queue".into())
    }

    /// Adds a queue and returns its id.
    pub fn add_queue(&mut self, name: &str) -> u32 {
        let id = self.queues.iter().map(|q| q.id).max().map_or(MAIN_QUEUE_ID, |m| m + 1);
        self.queues.push(QueueDef { id, name: name.trim().to_string() });
        id
    }

    /// Fills in passwords after loading (they are stored encrypted).
    pub fn decrypt_secrets(&mut self) {
        for c in &mut self.credentials {
            c.pass = crate::secret::unprotect(&c.pass_enc);
        }
    }

    /// A copy that is safe to write to disk: plain passwords are replaced by their encrypted form.
    pub fn for_disk(&self) -> Settings {
        let mut s = self.clone();
        for c in &mut s.credentials {
            c.pass_enc = crate::secret::protect(&c.pass);
        }
        s
    }

    /// Connection count for this URL: a matching server exception, else the default.
    pub fn connections_for(&self, url: &str) -> u32 {
        let host = host_of(url);
        self.server_limits
            .iter()
            .find(|l| !l.host.trim().is_empty() && l.connections > 0 && host_matches(l.host.trim(), &host))
            .map(|l| l.connections)
            .unwrap_or(self.connections)
    }

    /// True when the file extension is in the user's auto-download list.
    pub fn is_auto_type(&self, file_name: &str) -> bool {
        let Some((_, ext)) = file_name.rsplit_once('.') else { return false };
        let ext = ext.to_ascii_lowercase();
        self.auto_extensions.split_whitespace().any(|p| extension_matches(&p.to_ascii_lowercase(), &ext))
    }

    /// True when the URL's site is in the "never download automatically" list.
    pub fn is_excluded_site(&self, url: &str) -> bool {
        let host = host_of(url);
        !host.is_empty() && self.excluded_sites.split_whitespace().any(|p| host_matches(p, &host))
    }

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
    fn temp_path_uses_temp_dir_with_id_prefix_when_set() {
        let mut d = Download { id: 7, url: String::new(), file_name: "a.zip".into(), save_dir: PathBuf::from("D:/dl"), category: Category::General, size: None, resumable: false, parts: vec![], status: Status::Stopped, last_try: String::new(), description: String::new(), connections: 1, speed_limit_kbps: None, in_queue: false, queue_id: 0, referer: String::new(), user_agent: String::new(), hls: None, temp_dir: None, server_date: None, message: String::new(), last_speed: 0.0, speed: 0.0, active_connections: 0 };
        assert_eq!(d.temp_path(), PathBuf::from("D:/dl").join("a.zip.part"));
        d.temp_dir = Some(PathBuf::from("T:/tmp"));
        assert_eq!(d.temp_path(), PathBuf::from("T:/tmp").join("7-a.zip.part"));
    }

    #[test]
    fn move_file_any_moves_within_a_drive_and_keeps_content() {
        let dir = std::env::temp_dir().join(format!("idm-move-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.bin"), dir.join("b.bin"));
        std::fs::write(&a, b"xyz").unwrap();
        move_file_any(&a, &b).unwrap();
        assert!(!a.exists());
        assert_eq!(std::fs::read(&b).unwrap(), b"xyz");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn add_queue_gives_unique_ids_and_names() {
        let mut s = Settings::default();
        let a = s.add_queue(" Night ");
        let b = s.add_queue("Work");
        assert_ne!(a, b);
        assert_eq!(s.queue_name(a), "Night");
        assert_eq!(s.queue_name(999), "Main download queue");
    }

    #[test]
    fn server_limit_overrides_default_connections_for_matching_host() {
        let mut s = Settings::default();
        s.server_limits.push(ServerLimit { host: "*.cdn.ir".into(), connections: 2 });
        assert_eq!(s.connections_for("https://edge1.cdn.ir/a.rar"), 2);
        assert_eq!(s.connections_for("https://other.com/a.rar"), s.connections);
    }

    #[test]
    fn excluded_site_wildcard_matches_subdomains_only_for_star_patterns() {
        let s = Settings::default();
        assert!(s.is_excluded_site("https://x.update.microsoft.com/a.exe"));
        assert!(s.is_excluded_site("http://download.windowsupdate.com/a.cab"));
        assert!(!s.is_excluded_site("https://evil-download.windowsupdate.com.example.org/a.exe"));
        assert!(!s.is_excluded_site("https://example.com/a.zip"));
    }

    #[test]
    fn auto_type_uses_editable_extension_list() {
        let mut s = Settings::default();
        assert!(s.is_auto_type("a.ZIP"));
        assert!(!s.is_auto_type("a.html"));
        s.auto_extensions = "html".into();
        assert!(s.is_auto_type("a.html"));
        assert!(!s.is_auto_type("a.zip"));
    }

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
