use crate::engine;
use crate::i18n::{self, tr, Language};
use crate::model::*;
use crate::net::{self, ProbeInfo};
use crate::server::AddRequest;
use crate::sound;
use crate::tray::TrayHandle;
use crate::widgets::{self, Icon};
use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

const SCREENSHOT_FRAME: u32 = 120;
const SAVE_EVERY: Duration = Duration::from_secs(3);
const CLIPBOARD_EVERY: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq)]
enum View {
    All(Option<Category>),
    Unfinished(Option<Category>),
    Finished(Option<Category>),
    Queue(u32),
}

struct FileInfo {
    top_frames: u8,
    url: String,
    file_name: String,
    category: Category,
    save_dir: String,
    description: String,
    probe: Option<ProbeInfo>,
    waiting: Option<Receiver<Result<ProbeInfo, String>>>,
    error: Option<String>,
    duplicate: bool,
}

struct Grabber {
    page: String,
    types: String,
    links: Vec<(bool, String)>,
    waiting: Option<Receiver<Result<Vec<String>, String>>>,
    error: Option<String>,
}

struct Shot {
    path: PathBuf,
    frames: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum OptionsTab {
    General,
    Connection,
    Downloads,
    SaveTo,
    FileTypes,
    Proxy,
    Sounds,
    Passwords,
    Antivirus,
}

/// What to do once every running download has finished.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterAll {
    Nothing,
    ExitApp,
    Shutdown,
    Restart,
    Sleep,
    Hibernate,
    Disconnect,
}

impl AfterAll {
    const POWER: [AfterAll; 4] = [AfterAll::Shutdown, AfterAll::Restart, AfterAll::Sleep, AfterAll::Hibernate];

    fn label(self) -> &'static str {
        match self {
            AfterAll::Nothing => "Do nothing",
            AfterAll::ExitApp => "Exit the program",
            AfterAll::Shutdown => "Shut down",
            AfterAll::Restart => "Restart",
            AfterAll::Sleep => "Sleep",
            AfterAll::Hibernate => "Hibernate",
            AfterAll::Disconnect => "Disconnect from the internet",
        }
    }

    fn run(self, force: bool) {
        if self == AfterAll::Disconnect {
            std::thread::spawn(disconnect_internet);
            return;
        }
        #[cfg(windows)]
        {
            let mut cmd = match self {
                AfterAll::Shutdown => { let mut c = std::process::Command::new("shutdown"); c.args(["/s", "/t", "0"]); c }
                AfterAll::Restart => { let mut c = std::process::Command::new("shutdown"); c.args(["/r", "/t", "0"]); c }
                AfterAll::Hibernate => { let mut c = std::process::Command::new("shutdown"); c.arg("/h"); c }
                AfterAll::Sleep => { let mut c = std::process::Command::new("rundll32"); c.args(["powrprof.dll,SetSuspendState", "0,1,0"]); c }
                _ => return,
            };
            if force && matches!(self, AfterAll::Shutdown | AfterAll::Restart) {
                cmd.arg("/f");
            }
            let _ = cmd.spawn();
        }
        #[cfg(not(windows))]
        let _ = force;
    }
}

/// Drops VPN/dial-up connections and releases the IP address (the latter may need administrator rights).
fn disconnect_internet() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x08000000;
        let script = "Get-VpnConnection -ErrorAction SilentlyContinue | Where-Object { $_.ConnectionStatus -eq 'Connected' } | ForEach-Object { rasdial $_.Name /disconnect }";
        let _ = std::process::Command::new("powershell").args(["-NoProfile", "-Command", script]).creation_flags(NO_WINDOW).status();
        let _ = std::process::Command::new("ipconfig").arg("/release").creation_flags(NO_WINDOW).status();
    }
}

const POWER_COUNTDOWN_SECS: u64 = 30;
const UPDATE_API_URL: &str = "https://api.github.com/repos/mohammadreza-fallah98/Iran-Download-Manager/releases/latest";
const UPDATE_PAGE_URL: &str = "https://github.com/mohammadreza-fallah98/Iran-Download-Manager/releases";

struct RenameDlg {
    id: u64,
    name: String,
    dir: String,
    error: Option<String>,
}

pub struct App {
    queue_pick: Option<(Vec<u64>, u32)>,
    update_rx: Option<Receiver<Result<String, String>>>,
    refresh_check: Option<(u64, String, Receiver<Result<ProbeInfo, String>>)>,
    notice: Option<String>,
    quota_window_start: Instant,
    quota_bytes: u64,
    quota_last_total: u64,
    quota_hit: bool,
    after_all: AfterAll,
    after_force: bool,
    had_running: bool,
    power_countdown: Option<(AfterAll, bool, Instant)>,
    progress_tab_done: HashSet<u64>,
    rename_dlg: Option<RenameDlg>,
    props: Option<u64>,
    refresh_dlg: Option<(u64, String)>,
    downloads: Vec<Handle>,
    settings: Settings,
    next_id: u64,
    view: View,
    open_all: bool,
    open_unfinished: bool,
    open_finished: bool,
    open_queues: bool,
    selected: HashSet<u64>,
    samples: HashMap<u64, (Instant, u64)>,
    statuses: HashMap<u64, Status>,
    add_url: Option<String>,
    file_info: Option<FileInfo>,
    progress: Vec<u64>,
    details: HashSet<u64>,
    progress_tab_limiter: HashSet<u64>,
    complete: Vec<u64>,
    options: Option<(Settings, OptionsTab)>,
    scheduler: bool,
    grabber: Option<Grabber>,
    tell_friend: bool,
    about: bool,
    confirm_delete: Option<(Vec<u64>, bool)>,
    queues_running: HashSet<u32>,
    queue_edit: Option<(Option<u32>, String)>,
    clipboard: Option<arboard::Clipboard>,
    last_clip: String,
    last_clip_check: Instant,
    last_save: Instant,
    last_minute: String,
    data_path: Option<PathBuf>,
    shot: Option<Shot>,
    server_rx: Receiver<AddRequest>,
    tray: TrayHandle,
    force_top_frames: u32,
    really_exit: bool,
}

fn data_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(base).join("IranDownloadManager").join("downloads.json"))
}

/// Open a native folder-picker dialog and return the chosen path.
fn pick_folder(start: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x08000000;
        let start_str = start.display().to_string().replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $f = New-Object System.Windows.Forms.FolderBrowserDialog; \
             $f.SelectedPath = '{}'; \
             if ($f.ShowDialog() -eq 'OK') {{ Write-Output $f.SelectedPath }}",
            start_str
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &script])
            .creation_flags(NO_WINDOW)
            .output()
            .ok()?;
        let path = std::str::from_utf8(&out.stdout).ok()?.trim().to_string();
        if path.is_empty() { None } else { Some(PathBuf::from(path)) }
    }
    #[cfg(not(windows))]
    { let _ = start; None }
}

/// Write / remove the Windows startup registry value.
fn set_run_on_startup(enabled: bool) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x08000000;
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        if enabled {
            if let Ok(exe) = std::env::current_exe() {
                let path_str = exe.display().to_string();
                let _ = std::process::Command::new("reg")
                    .args(["add", key, "/v", "IranDownloadManager", "/t", "REG_SZ", "/d", &path_str, "/f"])
                    .creation_flags(NO_WINDOW)
                    .output();
            }
        } else {
            let _ = std::process::Command::new("reg")
                .args(["delete", key, "/v", "IranDownloadManager", "/f"])
                .creation_flags(NO_WINDOW)
                .output();
        }
    }
    let _ = enabled;
}

fn startup_is_registered() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x08000000;
        return std::process::Command::new("reg")
            .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "IranDownloadManager"])
            .creation_flags(NO_WINDOW)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
    }
    #[cfg(not(windows))]
    false
}

fn open_in_shell(path: &Path, select: bool) {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("explorer");
        if select {
            cmd.arg(format!("/select,{}", path.display()));
        } else {
            cmd.arg(path);
        }
        let _ = cmd.spawn();
    }
    #[cfg(not(windows))]
    {
        let target = if select { path.parent().unwrap_or(path) } else { path };
        let _ = std::process::Command::new("xdg-open").arg(target).spawn();
    }
}

/// Pushes the settings that other threads read (network client, limits, extension config).
fn apply_runtime_settings(s: &Settings) {
    net::set_user_agent(&s.user_agent);
    net::configure_proxy(&s.proxy);
    net::configure_credentials(&s.credentials);
    engine::GLOBAL_LIMIT_KBPS.store(s.global_speed_limit_kbps as u64, std::sync::atomic::Ordering::Relaxed);
    USE_SERVER_DATE.store(s.use_server_date, std::sync::atomic::Ordering::Relaxed);
    crate::server::publish_config(s);
    i18n::set_language(s.language);
}

/// Runs the user's virus scanner (or any program) on a finished file; `{path}` is replaced.
fn run_after_download(command: &str, path: &Path) {
    let command = command.trim();
    if command.is_empty() {
        return;
    }
    let line = command.replace("{path}", &path.display().to_string());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x08000000;
        let _ = std::process::Command::new("cmd").args(["/C", &line]).creation_flags(NO_WINDOW).spawn();
    }
    #[cfg(not(windows))]
    let _ = std::process::Command::new("sh").args(["-c", &line]).spawn();
}

/// Opens (or saves to) a file chosen in the Windows file dialog.
fn pick_file(save: bool, default_name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x08000000;
        let class = if save { "SaveFileDialog" } else { "OpenFileDialog" };
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $f = New-Object System.Windows.Forms.{class}; \
             $f.Filter = 'Download list (*.json)|*.json|All files (*.*)|*.*'; \
             $f.FileName = '{}'; \
             if ($f.ShowDialog() -eq 'OK') {{ Write-Output $f.FileName }}",
            default_name.replace('\'', "''")
        );
        let out = std::process::Command::new("powershell").args(["-NoProfile", "-STA", "-Command", &script]).creation_flags(NO_WINDOW).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        return if text.is_empty() { None } else { Some(PathBuf::from(text)) };
    }
    #[cfg(not(windows))]
    {
        let _ = (save, default_name);
        None
    }
}

/// Streams are saved as .ts (or .mp4 once known); a playlist name like index.m3u8 would be wrong.
fn stream_file_name(url: &str, name: &str) -> String {
    let ext = if crate::hls::is_hls_url(url) {
        "ts"
    } else if crate::dash::is_dash_url(url) {
        "mp4"
    } else {
        return name.to_string();
    };
    let stem = match name.rsplit_once('.') {
        Some((s, e)) if matches!(e.to_ascii_lowercase().as_str(), "m3u8" | "m3u" | "mpd" | "ts" | "mp4") => s,
        _ => name,
    };
    format!("{stem}.{ext}")
}

fn open_with(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("rundll32").arg("shell32.dll,OpenAs_RunDLL").arg(path).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

/// Renames within a drive, or copies then deletes across drives.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if from == to || !from.exists() {
        return Ok(());
    }
    if to.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "A file with this name already exists"));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(from, to).is_err() {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)?;
    }
    Ok(())
}

fn valid_time(t: &str) -> bool {
    chrono::NaiveTime::parse_from_str(t.trim(), "%H:%M").is_ok()
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>, server_rx: Receiver<AddRequest>) -> Self {
        let shot = screenshot.map(|path| Shot { path, frames: 0 });
        let data_path = if shot.is_some() { None } else { data_path() };
        let mut saved: SavedState = data_path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        saved.settings.run_on_startup = startup_is_registered();
        saved.settings.decrypt_secrets();
        if shot.is_some() && std::env::var("IDM_LANG").is_ok_and(|v| v == "fa") {
            saved.settings.language = Language::Persian;
        }
        apply_runtime_settings(&saved.settings);
        for d in &saved.downloads {
            if !d.referer.is_empty() || !d.user_agent.is_empty() {
                net::set_extra(&d.url, net::Extra { referer: d.referer.clone(), cookies: String::new(), user_agent: d.user_agent.clone() });
            }
        }
        cc.egui_ctx.set_visuals(widgets::make_visuals(saved.settings.theme));
        i18n::install_fonts(&cc.egui_ctx);
        let downloads = saved
            .downloads
            .into_iter()
            .map(|mut d| {
                if d.is_running() {
                    d.status = Status::Stopped;
                }
                new_handle(d)
            })
            .collect();
        let mut clipboard = if shot.is_some() { None } else { arboard::Clipboard::new().ok() };
        let last_clip = clipboard.as_mut().and_then(|c| c.get_text().ok()).unwrap_or_default();
        App {
            downloads,
            settings: saved.settings,
            next_id: saved.next_id.max(1),
            view: View::All(None),
            open_all: true,
            open_unfinished: false,
            open_finished: false,
            open_queues: true,
            selected: HashSet::new(),
            samples: HashMap::new(),
            statuses: HashMap::new(),
            add_url: None,
            notice: None,
            queue_pick: None,
            update_rx: None,
            refresh_check: None,
            quota_window_start: Instant::now(),
            quota_bytes: 0,
            quota_last_total: 0,
            quota_hit: false,
            after_all: AfterAll::Nothing,
            after_force: false,
            had_running: false,
            power_countdown: None,
            progress_tab_done: HashSet::new(),
            rename_dlg: None,
            props: None,
            refresh_dlg: None,
            file_info: None,
            progress: Vec::new(),
            details: HashSet::new(),
            progress_tab_limiter: HashSet::new(),
            complete: Vec::new(),
            options: None,
            scheduler: false,
            grabber: None,
            tell_friend: false,
            about: false,
            confirm_delete: None,
            queues_running: HashSet::new(),
            queue_edit: None,
            clipboard,
            last_clip,
            last_clip_check: Instant::now(),
            last_save: Instant::now(),
            last_minute: String::new(),
            data_path,
            shot,
            server_rx,
            tray: crate::tray::start(cc.egui_ctx.clone()),
            force_top_frames: 0,
            really_exit: false,
        }
    }

    fn find(&self, id: u64) -> Option<Handle> {
        self.downloads.iter().find(|h| h.d.lock().unwrap().id == id).cloned()
    }

    fn save_state(&self) {
        let Some(path) = &self.data_path else { return };
        let state = SavedState { settings: self.settings.for_disk(), downloads: self.downloads.iter().map(|h| h.d.lock().unwrap().clone()).collect(), next_id: self.next_id };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&state) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    fn new_download(&mut self, url: &str, file_name: &str, category: Category, save_dir: PathBuf, description: String, probe: Option<&ProbeInfo>) -> Handle {
        let id = self.next_id;
        self.next_id += 1;
        let d = Download {
            id,
            url: url.trim().to_string(),
            file_name: net::sanitize_file_name(file_name),
            save_dir,
            category,
            size: probe.and_then(|p| p.size),
            resumable: probe.is_some_and(|p| p.resumable),
            parts: Vec::new(),
            status: Status::Stopped,
            last_try: String::new(),
            description,
            connections: self.settings.connections_for(url),
            speed_limit_kbps: None,
            in_queue: false,
            queue_id: MAIN_QUEUE_ID,
            referer: net::get_extra(url.trim()).referer,
            user_agent: net::get_extra(url.trim()).user_agent,
            hls: None,
            temp_dir: (!self.settings.temp_dir.trim().is_empty()).then(|| PathBuf::from(self.settings.temp_dir.trim())),
            server_date: probe.and_then(|p| p.last_modified.clone()),
            message: String::new(),
            last_speed: 0.0,
            speed: 0.0,
            active_connections: 0,
        };
        let h = new_handle(d);
        self.downloads.push(h.clone());
        h
    }

    /// Adds a download without the info window (Options > Downloads).
    fn add_direct(&mut self, url: &str, file_name: &str, description: String) {
        let url = url.trim();
        let name = stream_file_name(url, &if file_name.is_empty() { net::file_name_from_url(url) } else { file_name.to_string() });
        let category = Category::for_file(&name);
        let dir = self.settings.folder_for(category);
        let h = self.new_download(url, &name, category, dir, description, None);
        if self.settings.add_to_queue_only {
            h.d.lock().unwrap().in_queue = true;
        } else {
            engine::start(&h);
        }
    }

    fn open_file_info(&mut self, url: String) {
        let url = url.trim().to_string();
        let file_name = stream_file_name(&url, &net::file_name_from_url(&url));
        let category = Category::for_file(&file_name);
        let existing = self.downloads.iter().find_map(|h| { let d = h.d.lock().unwrap(); (d.url == url).then_some(d.id) });
        if let Some(id) = existing {
            if self.settings.duplicate_action == DuplicateAction::Skip {
                self.selected.clear();
                self.selected.insert(id);
                self.view = View::All(None);
                self.notice = Some("This address is already in the download list.".into());
                return;
            }
        }
        let duplicate = existing.is_some() && self.settings.duplicate_action == DuplicateAction::Ask;
        let (tx, rx) = channel();
        let u = url.clone();
        // A playlist is tiny and says nothing about the video, so streams are not probed
        let is_stream = crate::hls::is_hls_url(&url) || crate::dash::is_dash_url(&url);
        if !is_stream {
            std::thread::spawn(move || {
                let _ = tx.send(net::probe(&u));
            });
        }
        self.file_info = Some(FileInfo {
            top_frames: 4,
            url,
            file_name,
            category,
            save_dir: self.settings.folder_for(category).display().to_string(),
            description: String::new(),
            probe: None,
            waiting: if is_stream { None } else { Some(rx) },
            error: None,
            duplicate,
        });
    }

    fn selected_handles(&self) -> Vec<Handle> {
        self.downloads.iter().filter(|h| self.selected.contains(&h.d.lock().unwrap().id)).cloned().collect()
    }

    fn resume_selected(&mut self) {
        for h in self.selected_handles() {
            engine::start(&h);
        }
    }

    fn stop_selected(&mut self) {
        for h in self.selected_handles() {
            engine::stop(&h);
        }
    }

    fn stop_all(&mut self) {
        self.queues_running.clear();
        for h in &self.downloads {
            engine::stop(h);
        }
    }

    fn delete(&mut self, ids: &[u64], with_files: bool) {
        self.downloads.retain(|h| {
            let d = h.d.lock().unwrap();
            if !ids.contains(&d.id) {
                return true;
            }
            engine::stop(h);
            if d.status != Status::Complete {
                let _ = std::fs::remove_file(d.temp_path());
            } else if with_files {
                let _ = std::fs::remove_file(d.final_path());
            }
            false
        });
        for id in ids {
            self.selected.remove(id);
        }
        self.progress.retain(|id| !ids.contains(id));
    }

    /// Starts every queue (scheduler, menus).
    fn start_queue(&mut self) {
        let ids: Vec<u32> = self.settings.queues.iter().map(|q| q.id).collect();
        self.queues_running.extend(ids);
    }

    fn stop_queue(&mut self) {
        self.queues_running.clear();
        for h in &self.downloads {
            if h.d.lock().unwrap().in_queue {
                engine::stop(h);
            }
        }
    }

    fn start_queue_id(&mut self, id: u32) {
        self.queues_running.insert(id);
    }

    fn stop_queue_id(&mut self, id: u32) {
        self.queues_running.remove(&id);
        for h in &self.downloads {
            let d = h.d.lock().unwrap();
            if d.in_queue && d.queue_id == id {
                engine::stop(h);
            }
        }
    }

    fn delete_queue(&mut self, id: u32) {
        if id == MAIN_QUEUE_ID {
            return;
        }
        self.queues_running.remove(&id);
        for h in &self.downloads {
            let mut d = h.d.lock().unwrap();
            if d.queue_id == id {
                d.queue_id = MAIN_QUEUE_ID;
            }
        }
        self.settings.queues.retain(|q| q.id != id);
        self.view = View::Queue(MAIN_QUEUE_ID);
    }

    fn queue_edit_dialog(&mut self, ctx: &egui::Context) {
        let Some((target, name)) = &mut self.queue_edit else { return };
        let target = *target;
        let mut ok = false;
        let mut cancel = false;
        egui::Window::new(if target.is_some() { "Rename queue" } else { "New queue" }).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            let r = ui.add_sized([280.0, 22.0], egui::TextEdit::singleline(name).hint_text(tr("Queue name")));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ok |= ui.add_enabled(!name.trim().is_empty(), egui::Button::new(tr("    OK    "))).clicked();
                cancel = ui.button(tr("Cancel")).clicked();
            });
        });
        if cancel {
            self.queue_edit = None;
        } else if ok {
            let (_, name) = self.queue_edit.take().unwrap();
            if name.trim().is_empty() {
                return;
            }
            match target {
                Some(id) => {
                    if let Some(q) = self.settings.queues.iter_mut().find(|q| q.id == id) {
                        q.name = name.trim().to_string();
                    }
                }
                None => {
                    let id = self.settings.add_queue(&name);
                    self.view = View::Queue(id);
                }
            }
            self.save_state();
        }
    }

    /// The queue the toolbar buttons act on: the one shown in the list, else the main queue.
    fn current_queue(&self) -> u32 {
        match self.view {
            View::Queue(id) => id,
            _ => MAIN_QUEUE_ID,
        }
    }

    fn matches_view(&self, d: &Download) -> bool {
        let cat_ok = |c: Option<Category>| c.is_none_or(|c| c == d.category);
        match self.view {
            View::All(c) => cat_ok(c),
            View::Unfinished(c) => d.status != Status::Complete && cat_ok(c),
            View::Finished(c) => d.status == Status::Complete && cat_ok(c),
            View::Queue(id) => d.in_queue && d.queue_id == id,
        }
    }

    /// Runs every frame: speed samples, queue, scheduler, clipboard, saving, finished downloads.
    fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let mut any_running = false;
        for h in &self.downloads {
            let mut d = h.d.lock().unwrap();
            let bytes = d.downloaded();
            let entry = self.samples.entry(d.id).or_insert((now, bytes));
            let dt = now.duration_since(entry.0).as_secs_f64();
            if dt >= 1.0 {
                let inst = bytes.saturating_sub(entry.1) as f64 / dt;
                d.speed = if d.is_running() { if d.speed > 0.0 { d.speed * 0.4 + inst * 0.6 } else { inst } } else { 0.0 };
                *entry = (now, bytes);
            }
            any_running |= d.is_running();
            let prev = self.statuses.insert(d.id, d.status);
            if d.status == Status::Error && prev.is_some_and(|p| p != Status::Error) {
                sound::play(&self.settings.sounds.failed);
            }
            if d.status == Status::Complete && prev.is_some_and(|p| p != Status::Complete) {
                sound::play(&self.settings.sounds.complete);
                if self.settings.should_scan(&d.file_name) {
                    run_after_download(&self.settings.antivirus_cmd, &d.final_path());
                }
            }
            if d.status == Status::Complete && prev.is_some_and(|p| p != Status::Complete) {
                if self.settings.show_complete_dialog && self.progress.contains(&d.id) && !self.complete.contains(&d.id) {
                    self.complete.push(d.id);
                }
                self.progress.retain(|id| *id != d.id);
            }
        }

        let minute = chrono::Local::now().format("%H:%M").to_string();
        if minute != self.last_minute {
            if !self.last_minute.is_empty() {
                if self.settings.start_at_enabled && minute == self.settings.start_at.trim() {
                    sound::play(&self.settings.sounds.scheduler_start);
                    self.start_queue();
                }
                if self.settings.stop_at_enabled && minute == self.settings.stop_at.trim() {
                    sound::play(&self.settings.sounds.scheduler_stop);
                    self.stop_queue();
                }
            }
            self.last_minute = minute;
        }

        for qid in self.queues_running.clone() {
            let running = self.downloads.iter().filter(|h| { let d = h.d.lock().unwrap(); d.in_queue && d.queue_id == qid && d.is_running() }).count() as u32;
            let waiting: Vec<Handle> = self.downloads.iter().filter(|h| { let d = h.d.lock().unwrap(); d.in_queue && d.queue_id == qid && d.status == Status::Stopped }).cloned().collect();
            if running == 0 && waiting.is_empty() {
                self.queues_running.remove(&qid);
            }
            for h in waiting.into_iter().take(self.settings.max_at_once.saturating_sub(running) as usize) {
                engine::start(&h);
            }
        }

        if self.settings.watch_clipboard && now.duration_since(self.last_clip_check) >= CLIPBOARD_EVERY && self.file_info.is_none() && self.add_url.is_none() {
            self.last_clip_check = now;
            if let Some(text) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) {
                if text != self.last_clip {
                    self.last_clip = text.clone();
                    if net::looks_like_url(&text) && self.settings.is_auto_type(&net::file_name_from_url(text.trim())) && !self.settings.is_excluded_site(text.trim()) {
                        self.open_file_info(text);
                    }
                }
            }
        }

        if now.duration_since(self.last_save) >= SAVE_EVERY {
            self.last_save = now;
            self.save_state();
        }

        self.poll_background_checks();

        // Pick up downloads sent by the browser extension
        while let Ok(req) = self.server_rx.try_recv() {
            if net::looks_like_url(&req.url) && !self.settings.show_start_dialog {
                let description = if req.description.is_empty() { req.referrer.clone() } else { req.description.clone() };
                self.add_direct(&req.url, &req.file_name, description);
                continue;
            }
            if net::looks_like_url(&req.url) && self.file_info.is_none() {
                self.open_file_info(req.url.clone());
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                self.force_top_frames = 3;
                // Pre-fill description with referrer so the user can see where it came from
                if let Some(fi) = self.file_info.as_mut() {
                    if !req.description.is_empty() {
                        fi.description = req.description.clone();
                    } else if !req.referrer.is_empty() {
                        fi.description = req.referrer.clone();
                    }
                    if !req.file_name.is_empty() {
                        fi.file_name = stream_file_name(&fi.url, &req.file_name);
                    }
                }
            }
        }

        // Download quota: stop everything once the limit of the current time window is used up
        let total: u64 = self.downloads.iter().map(|h| h.d.lock().unwrap().downloaded()).sum();
        if self.settings.quota.enabled {
            if now.duration_since(self.quota_window_start) >= Duration::from_secs(self.settings.quota.hours * 3600) {
                self.quota_window_start = now;
                self.quota_bytes = 0;
                self.quota_hit = false;
            }
            self.quota_bytes += total.saturating_sub(self.quota_last_total);
            if !self.quota_hit && self.quota_bytes >= self.settings.quota.megabytes * 1024 * 1024 {
                self.quota_hit = true;
                for h in &self.downloads {
                    engine::stop(h);
                }
                self.stop_queue();
            }
        } else {
            self.quota_hit = false;
        }
        self.quota_last_total = total;
        if self.quota_hit {
            let mut ok = false;
            egui::Window::new(tr("Download limit reached")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(format!("{} MB were downloaded in the last {} hour(s). Downloads were stopped.", self.settings.quota.megabytes, self.settings.quota.hours));
                ok = ui.button(tr("OK")).clicked();
            });
            if ok {
                self.quota_hit = false;
                self.quota_window_start = now;
                self.quota_bytes = 0;
            }
        }

        // "When done" action: fires once, when the last running download finishes
        let queue_waiting = !self.queues_running.is_empty() && self.downloads.iter().any(|h| { let d = h.d.lock().unwrap(); d.in_queue && d.status != Status::Complete });
        if any_running || queue_waiting {
            self.had_running = true;
        } else if self.had_running {
            self.had_running = false;
            match self.after_all {
                AfterAll::Nothing => {}
                AfterAll::ExitApp => {
                    self.really_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                power => self.power_countdown = Some((power, self.after_force, Instant::now())),
            }
        }
        if let Some((action, force, started)) = self.power_countdown {
            let left = POWER_COUNTDOWN_SECS.saturating_sub(started.elapsed().as_secs());
            let mut cancel = false;
            egui::Window::new(tr("Downloads finished")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(format!("The computer will do: {} in {left} seconds.", action.label()));
                cancel = ui.button(tr("Cancel")).clicked();
            });
            if cancel {
                self.power_countdown = None;
                self.after_all = AfterAll::Nothing;
            } else if left == 0 {
                self.power_countdown = None;
                self.after_all = AfterAll::Nothing;
                action.run(force);
            }
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        ctx.request_repaint_after(if any_running { Duration::from_millis(250) } else { Duration::from_secs(1) });
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button(tr("Tasks"), |ui| {
                if ui.button(tr("Add new download...")).clicked() {
                    self.show_add_url();
                    ui.close_menu();
                }
                if ui.button(tr("Run site grabber")).clicked() {
                    self.show_grabber();
                    ui.close_menu();
                }
                if ui.button(tr("Add batch download from clipboard")).clicked() {
                    self.add_batch_from_clipboard();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(tr("Export list...")).clicked() {
                    self.export_list();
                    ui.close_menu();
                }
                if ui.button(tr("Import list...")).clicked() {
                    self.import_list();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(tr("Exit")).clicked() {
                    self.really_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(tr("File"), |ui| {
                let has = !self.selected.is_empty();
                if ui.add_enabled(has, egui::Button::new(tr("Download now"))).clicked() {
                    self.resume_selected();
                    ui.close_menu();
                }
                if ui.add_enabled(has, egui::Button::new(tr("Stop download"))).clicked() {
                    self.stop_selected();
                    ui.close_menu();
                }
                if ui.add_enabled(has, egui::Button::new(tr("Remove"))).clicked() {
                    self.confirm_delete = Some((self.selected.iter().copied().collect(), false));
                    ui.close_menu();
                }
            });
            ui.menu_button(tr("Downloads"), |ui| {
                if ui.button(tr("Stop all")).clicked() {
                    self.stop_all();
                    ui.close_menu();
                }
                if ui.button(tr("Delete all completed")).clicked() {
                    let ids: Vec<u64> = self.downloads.iter().filter_map(|h| { let d = h.d.lock().unwrap(); (d.status == Status::Complete).then_some(d.id) }).collect();
                    self.delete(&ids, false);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(tr("Start queue")).clicked() {
                    self.start_queue();
                    ui.close_menu();
                }
                if ui.button(tr("Stop queue")).clicked() {
                    self.stop_queue();
                    ui.close_menu();
                }
                if ui.button(tr("Scheduler...")).clicked() {
                    self.scheduler = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(tr("Options...")).clicked() {
                    self.options = Some((self.settings.clone(), OptionsTab::General));
                    ui.close_menu();
                }
            });
            ui.menu_button(tr("View"), |ui| {
                if ui.button(tr("All downloads")).clicked() {
                    self.view = View::All(None);
                    ui.close_menu();
                }
                if ui.button(tr("Main download queue")).clicked() {
                    self.view = View::Queue(MAIN_QUEUE_ID);
                    ui.close_menu();
                }
            });
            ui.menu_button(tr("Help"), |ui| {
                if ui.button(tr("Check for updates...")).clicked() {
                    self.check_for_updates();
                    ui.close_menu();
                }
                if ui.button(tr("About")).clicked() {
                    self.about = true;
                    ui.close_menu();
                }
            });
            ui.menu_button(tr("Registration"), |ui| {
                ui.label(tr("This rebuild is free; nothing to register."));
            });
        });
    }

    fn show_add_url(&mut self) {
        let clip = self.clipboard.as_mut().and_then(|c| c.get_text().ok()).filter(|t| net::looks_like_url(t)).unwrap_or_default();
        self.add_url = Some(clip.trim().to_string());
    }

    fn show_grabber(&mut self) {
        if self.grabber.is_none() {
            let types = Category::ALL.iter().map(|c| c.extensions()).collect::<Vec<_>>().join(" ");
            self.grabber = Some(Grabber { page: String::new(), types, links: Vec::new(), waiting: None, error: None });
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let has = !self.selected.is_empty();
        let any_running = self.downloads.iter().any(|h| h.d.lock().unwrap().is_running());
        let theme = self.settings.theme;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if widgets::tool_button(ui, Icon::AddUrl, &tr("Add URL"), true, theme).clicked() {
                self.show_add_url();
            }
            if widgets::tool_button(ui, Icon::Resume, &tr("Resume"), has, theme).clicked() {
                self.resume_selected();
            }
            if widgets::tool_button(ui, Icon::Stop, &tr("Stop"), has, theme).clicked() {
                self.stop_selected();
            }
            if widgets::tool_button(ui, Icon::StopAll, &tr("Stop All"), any_running, theme).clicked() {
                self.stop_all();
            }
            if widgets::tool_button(ui, Icon::Delete, &tr("Delete"), has, theme).clicked() {
                self.confirm_delete = Some((self.selected.iter().copied().collect(), false));
            }
            ui.separator();
            if widgets::tool_button(ui, Icon::Options, &tr("Options"), true, theme).clicked() {
                self.options = Some((self.settings.clone(), OptionsTab::General));
            }
            if widgets::tool_button(ui, Icon::Scheduler, &tr("Scheduler"), true, theme).clicked() {
                self.scheduler = true;
            }
            let cur_queue = self.current_queue();
            let cur_running = self.queues_running.contains(&cur_queue);
            if widgets::tool_button(ui, Icon::StartQueue, &tr("Start Queue"), !cur_running, theme).clicked() {
                self.start_queue_id(cur_queue);
            }
            if widgets::tool_button(ui, Icon::StopQueue, &tr("Stop Queue"), cur_running, theme).clicked() {
                self.stop_queue_id(cur_queue);
            }
            ui.separator();
            if widgets::tool_button(ui, Icon::Grabber, &tr("Grabber"), true, theme).clicked() {
                self.show_grabber();
            }
            if widgets::tool_button(ui, Icon::TellFriend, &tr("Tell a Friend"), true, theme).clicked() {
                self.tell_friend = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = tr(if theme == crate::model::Theme::Dark { "Light" } else { "Dark" });
                if widgets::tool_button(ui, Icon::ThemeToggle, &label, true, theme).clicked() {
                    self.settings.theme = match theme {
                        crate::model::Theme::Dark  => crate::model::Theme::Light,
                        crate::model::Theme::Light => crate::model::Theme::Dark,
                    };
                    ctx.set_visuals(widgets::make_visuals(self.settings.theme));
                }
            });
        });
    }

    fn categories(&mut self, ui: &mut egui::Ui) {
        let theme = self.settings.theme;
        let pal = widgets::palette(theme);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(tr("Categories")).strong().color(pal.text));
        });
        ui.separator();
        let folder = Color32::from_rgb(200, 165, 60);
        let snapshot: Vec<(Status, bool)> = self.downloads.iter().map(|h| { let d = h.d.lock().unwrap(); (d.status, d.in_queue) }).collect();
        let count = |f: &dyn Fn(Status, bool) -> bool| snapshot.iter().filter(|(s, q)| f(*s, *q)).count();
        let groups: [(fn(Option<Category>) -> View, &str, bool); 3] = [(View::All, "All Downloads", self.open_all), (View::Unfinished, "Unfinished", self.open_unfinished), (View::Finished, "Finished", self.open_finished)];
        for (gi, (make, name, open)) in groups.into_iter().enumerate() {
            let n = match gi {
                0 => count(&|_, _| true),
                1 => count(&|s, _| s != Status::Complete),
                _ => count(&|s, _| s == Status::Complete),
            };
            let label = if n > 0 { format!("{} ({n})", tr(name)) } else { tr(name) };
            let (clicked, arrow) = widgets::tree_item(ui, 0, Some(open), folder, &label, self.view == make(None), theme);
            if clicked {
                self.view = make(None);
            }
            if arrow {
                match gi {
                    0 => self.open_all = !self.open_all,
                    1 => self.open_unfinished = !self.open_unfinished,
                    _ => self.open_finished = !self.open_finished,
                }
            }
            if open {
                for c in Category::ALL {
                    if widgets::tree_item(ui, 1, None, widgets::category_color(c), &tr(c.name()), self.view == make(Some(c)), theme).0 {
                        self.view = make(Some(c));
                    }
                }
            }
        }
        if widgets::tree_item(ui, 0, None, folder, &tr("Grabber projects"), false, theme).0 {
            self.show_grabber();
        }
        let (_, arrow) = widgets::tree_item(ui, 0, Some(self.open_queues), folder, &tr("Queues"), false, theme);
        if arrow {
            self.open_queues = !self.open_queues;
        }
        if self.open_queues {
            let queues = self.settings.queues.clone();
            for q in &queues {
                let n = self.downloads.iter().filter(|h| { let d = h.d.lock().unwrap(); d.in_queue && d.queue_id == q.id }).count();
                let label = if n > 0 { format!("{} ({n})", tr(&q.name)) } else { tr(&q.name) };
                if widgets::tree_item(ui, 1, None, Color32::from_rgb(80, 150, 220), &label, self.view == View::Queue(q.id), theme).0 {
                    self.view = View::Queue(q.id);
                }
            }
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                if ui.small_button(tr("New")).on_hover_text(tr("Create a queue")).clicked() {
                    self.queue_edit = Some((None, String::new()));
                }
                if let View::Queue(id) = self.view {
                    if ui.small_button(tr("Rename")).clicked() {
                        self.queue_edit = Some((Some(id), self.settings.queue_name(id)));
                    }
                    if id != MAIN_QUEUE_ID && ui.small_button(tr("Delete")).on_hover_text(tr("Delete this queue; its files go back to the main queue")).clicked() {
                        self.delete_queue(id);
                    }
                }
            });
        }
    }

    fn list(&mut self, ui: &mut egui::Ui) {
        let rows: Vec<Download> = self.downloads.iter().map(|h| h.d.lock().unwrap().clone()).filter(|d| self.matches_view(d)).collect();
        let mut clicked: Option<(u64, bool)> = None;
        let mut double: Option<u64> = None;
        let mut context: Option<(u64, &'static str)> = None;
        let mut queue_move: Option<(u64, u32)> = None;
        let queues = self.settings.queues.clone();
        let pal = widgets::palette(self.settings.theme);
        let painter = ui.painter().clone();
        let sep_color = pal.border;
        ui.style_mut().interaction.selectable_labels = false;
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(290.0).at_least(120.0).clip(true))
            .column(Column::initial(85.0).at_least(50.0))
            .column(Column::initial(90.0).at_least(50.0))
            .column(Column::initial(95.0).at_least(50.0))
            .column(Column::initial(95.0).at_least(50.0))
            .column(Column::initial(115.0).at_least(60.0))
            .column(Column::initial(55.0).at_least(40.0))
            .column(Column::remainder().clip(true))
            .header(22.0, |mut h| {
                let hdr_color = widgets::palette(self.settings.theme).text_header;
                for title in ["File Name", "Size", "Status", "Time left", "Transfer rate", "Last Try Date", "Queue", "Description"] {
                    h.col(|ui| {
                        ui.label(RichText::new(tr(title)).color(hdr_color).strong());
                    });
                }
            })
            .body(|body| {
                body.rows(21.0, rows.len(), |mut row| {
                    let d = &rows[row.index()];
                    row.set_selected(self.selected.contains(&d.id));
                    row.col(|ui| {
                        widgets::file_icon(ui, d.category);
                        ui.label(&d.file_name);
                    });
                    row.col(|ui| {
                        ui.label(d.size.map(widgets::fmt_size).unwrap_or_default());
                    });
                    row.col(|ui| {
                        ui.label(status_text(d));
                    });
                    row.col(|ui| {
                        ui.label(d.time_left_secs().map(widgets::fmt_time_left).unwrap_or_default());
                    });
                    row.col(|ui| {
                        ui.label(widgets::fmt_speed(d.speed));
                    });
                    row.col(|ui| {
                        ui.label(&d.last_try);
                    });
                    row.col(|ui| {
                        if d.in_queue {
                            ui.label(RichText::new(tr("✓")).color(Color32::from_rgb(0, 200, 120)));
                        }
                    });
                    row.col(|ui| {
                        ui.label(&d.description);
                    });
                    let resp = row.response();
                    // Row separator line
                    let r = resp.rect;
                    painter.line_segment(
                        [egui::pos2(r.min.x, r.max.y - 0.5), egui::pos2(r.max.x, r.max.y - 0.5)],
                        egui::Stroke::new(1.0_f32, sep_color),
                    );
                    if resp.clicked() {
                        clicked = Some((d.id, resp.ctx.input(|i| i.modifiers.command)));
                    }
                    if resp.double_clicked() {
                        double = Some(d.id);
                    }
                    let id = d.id;
                    let complete = d.status == Status::Complete;
                    let in_queue = d.in_queue;
                    let running = d.is_running();
                    resp.context_menu(|ui| {
                        let mut item = |ui: &mut egui::Ui, label: &str, enabled: bool, key: &'static str| {
                            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                                context = Some((id, key));
                                ui.close_menu();
                            }
                        };
                        item(ui, "Open", complete, "Open");
                        item(ui, "Open with...", complete, "Open with");
                        item(ui, "Open folder", true, "Open folder");
                        ui.separator();
                        item(ui, "Move/Rename...    (Ctrl+M)", !running, "Rename");
                        item(ui, "Redownload", !running, "Redownload");
                        ui.separator();
                        item(ui, "Resume", !complete && !running, "Resume");
                        item(ui, "Stop", running, "Stop");
                        item(ui, "Show progress", !complete, "Show progress");
                        item(ui, "Refresh download address...", !complete && !running, "Refresh address");
                        ui.separator();
                        let queue_label = if in_queue { "Remove from queue" } else { "Add to queue" };
                        item(ui, queue_label, !complete, queue_label);
                        ui.add_enabled_ui(!complete, |ui| {
                            ui.menu_button(tr("Move to queue"), |ui| {
                                for q in &queues {
                                    if ui.button(&q.name).clicked() {
                                        queue_move = Some((id, q.id));
                                        ui.close_menu();
                                    }
                                }
                            });
                        });
                        item(ui, "Delete", true, "Delete");
                        ui.separator();
                        item(ui, "Copy URL", true, "Copy URL");
                        item(ui, "Properties", true, "Properties");
                    });
                });
            });
        if let Some((id, add)) = clicked {
            if !add {
                self.selected.clear();
            }
            if !self.selected.insert(id) {
                self.selected.remove(&id);
            }
        }
        if let Some(id) = double {
            self.activate(id);
        }
        if let Some((id, action)) = context {
            self.context_action(id, action);
        }
        if let Some((id, qid)) = queue_move {
            if let Some(h) = self.find(id) {
                let mut d = h.d.lock().unwrap();
                d.in_queue = true;
                d.queue_id = qid;
            }
        }
    }

    fn activate(&mut self, id: u64) {
        let Some(h) = self.find(id) else { return };
        let (complete, path) = { let d = h.d.lock().unwrap(); (d.status == Status::Complete, d.final_path()) };
        if complete {
            open_in_shell(&path, false);
        } else if !self.progress.contains(&id) {
            self.progress.push(id);
        }
    }

    fn context_action(&mut self, id: u64, action: &str) {
        let Some(h) = self.find(id) else { return };
        match action {
            "Open" => open_in_shell(&h.d.lock().unwrap().final_path(), false),
            "Open folder" => open_in_shell(&h.d.lock().unwrap().final_path(), true),
            "Resume" => {
                engine::start(&h);
                if !self.progress.contains(&id) {
                    self.progress.push(id);
                }
            }
            "Stop" => engine::stop(&h),
            "Show progress" => self.activate(id),
            "Add to queue" => h.d.lock().unwrap().in_queue = true,
            "Remove from queue" => h.d.lock().unwrap().in_queue = false,
            "Delete" => self.confirm_delete = Some((vec![id], false)),
            "Open with" => open_with(&h.d.lock().unwrap().final_path()),
            "Rename" => self.open_rename(id),
            "Redownload" => {
                {
                    let mut d = h.d.lock().unwrap();
                    if d.is_running() {
                        return;
                    }
                    let _ = std::fs::remove_file(d.temp_path());
                    let _ = std::fs::remove_file(d.final_path());
                    d.parts.clear();
                    d.size = None;
                    d.status = Status::Stopped;
                    d.message.clear();
                }
                engine::start(&h);
                if !self.progress.contains(&id) {
                    self.progress.push(id);
                }
            }
            "Refresh address" => self.refresh_dlg = Some((id, h.d.lock().unwrap().url.clone())),
            "Copy URL" => {
                let url = h.d.lock().unwrap().url.clone();
                if let Some(c) = self.clipboard.as_mut() {
                    let _ = c.set_text(url.clone());
                }
                self.last_clip = url;
            }
            "Properties" => self.props = Some(id),
            _ => {}
        }
    }

    /// Adds every address found in the clipboard text to the queue (nothing starts).
    fn add_batch_from_clipboard(&mut self) {
        let text = self.clipboard.as_mut().and_then(|c| c.get_text().ok()).unwrap_or_default();
        let mut urls: Vec<String> = Vec::new();
        for word in text.split(|c: char| c.is_whitespace() || c == '"' || c == '<' || c == '>') {
            let w = word.trim_matches(|c| c == ',' || c == ';');
            if net::looks_like_url(w) && !urls.iter().any(|u| u == w) {
                urls.push(w.to_string());
            }
        }
        let (mut added, mut skipped) = (0, 0);
        for url in urls {
            if self.downloads.iter().any(|h| h.d.lock().unwrap().url == url) {
                skipped += 1;
                continue;
            }
            let name = stream_file_name(&url, &net::file_name_from_url(&url));
            let category = Category::for_file(&name);
            let dir = self.settings.folder_for(category);
            let h = self.new_download(&url, &name, category, dir, "Added from clipboard".into(), None);
            h.d.lock().unwrap().in_queue = true;
            added += 1;
        }
        self.view = View::Queue(MAIN_QUEUE_ID);
        self.notice = Some(if added == 0 && skipped == 0 { "No download addresses were found in the clipboard.".to_string() } else { format!("{added} address(es) added to the queue, {skipped} already in the list.") });
    }

    fn export_list(&mut self) {
        let Some(path) = pick_file(true, "downloads-export.json") else { return };
        let list: Vec<Download> = self.downloads.iter().map(|h| h.d.lock().unwrap().clone()).collect();
        let body = serde_json::json!({ "version": 1, "downloads": list });
        self.notice = Some(match serde_json::to_string_pretty(&body).map_err(|e| e.to_string()).and_then(|t| std::fs::write(&path, t).map_err(|e| e.to_string())) {
            Ok(()) => format!("{} download(s) exported to {}", list.len(), path.display()),
            Err(e) => format!("Export failed: {e}"),
        });
    }

    fn import_list(&mut self) {
        let Some(path) = pick_file(false, "") else { return };
        #[derive(serde::Deserialize)]
        struct Export {
            downloads: Vec<Download>,
        }
        let parsed = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Export>(&t).map_err(|e| e.to_string()));
        let list = match parsed {
            Ok(e) => e.downloads,
            Err(e) => {
                self.notice = Some(format!("Import failed: {e}"));
                return;
            }
        };
        let (mut added, mut skipped) = (0, 0);
        for mut d in list {
            if self.downloads.iter().any(|h| h.d.lock().unwrap().url == d.url) {
                skipped += 1;
                continue;
            }
            d.id = self.next_id;
            self.next_id += 1;
            if d.is_running() {
                d.status = Status::Stopped;
            }
            if !self.settings.queues.iter().any(|q| q.id == d.queue_id) {
                d.queue_id = MAIN_QUEUE_ID;
            }
            self.downloads.push(new_handle(d));
            added += 1;
        }
        self.notice = Some(format!("{added} download(s) imported, {skipped} already in the list."));
    }

    fn notice_dialog(&mut self, ctx: &egui::Context) {
        let Some(text) = &self.notice else { return };
        let mut ok = false;
        egui::Window::new(tr("Iran Download Manager")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(text);
            ui.add_space(6.0);
            ok = ui.button(tr("    OK    ")).clicked();
        });
        if ok {
            self.notice = None;
        }
    }

    fn open_rename(&mut self, id: u64) {
        let Some(h) = self.find(id) else { return };
        let d = h.d.lock().unwrap();
        if d.is_running() {
            return;
        }
        self.rename_dlg = Some(RenameDlg { id, name: d.file_name.clone(), dir: d.save_dir.display().to_string(), error: None });
    }

    fn apply_rename(&self, r: &RenameDlg) -> Result<(), String> {
        let name = r.name.trim();
        let dir = r.dir.trim();
        if name.is_empty() || dir.is_empty() {
            return Err("File name and folder are required".into());
        }
        if name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) {
            return Err("The file name contains invalid characters".into());
        }
        let h = self.find(r.id).ok_or("Download not found")?;
        let mut d = h.d.lock().unwrap();
        if d.is_running() {
            return Err("Stop the download first".into());
        }
        let new_dir = PathBuf::from(dir);
        let (from, to) = if d.status == Status::Complete {
            (d.final_path(), new_dir.join(name))
        } else {
            let mut renamed = d.clone();
            renamed.file_name = name.to_string();
            renamed.save_dir = new_dir.clone();
            (d.temp_path(), renamed.temp_path())
        };
        move_file(&from, &to).map_err(|e| e.to_string())?;
        d.file_name = name.to_string();
        d.save_dir = new_dir;
        Ok(())
    }

    fn rename_dialog(&mut self, ctx: &egui::Context) {
        let Some(r) = &mut self.rename_dlg else { return };
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(tr("Move / Rename")).collapsible(false).resizable(false).constrain(true).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            egui::Grid::new("rename").num_columns(3).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label(tr("File name"));
                ui.add_sized([400.0, 22.0], egui::TextEdit::singleline(&mut r.name));
                ui.label(tr(""));
                ui.end_row();
                ui.label(tr("Save To"));
                ui.add_sized([400.0, 22.0], egui::TextEdit::singleline(&mut r.dir));
                if ui.button(tr("...")).clicked() {
                    if let Some(p) = pick_folder(Path::new(r.dir.trim())) {
                        r.dir = p.display().to_string();
                    }
                }
                ui.end_row();
            });
            if let Some(e) = &r.error {
                ui.colored_label(Color32::from_rgb(190, 40, 30), e);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                apply = ui.button(tr("    OK    ")).clicked();
                cancel = ui.button(tr("Cancel")).clicked();
            });
        });
        if cancel {
            self.rename_dlg = None;
        } else if apply {
            let r = self.rename_dlg.take().unwrap();
            if let Err(e) = self.apply_rename(&r) {
                self.rename_dlg = Some(RenameDlg { error: Some(e), ..r });
            }
        }
    }

    fn refresh_dialog(&mut self, ctx: &egui::Context) {
        let Some((id, url)) = &mut self.refresh_dlg else { return };
        let id = *id;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(tr("Refresh download address")).collapsible(false).resizable(false).constrain(true).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(tr("Paste the new address of this file. Already downloaded parts are kept."));
            ui.add_sized([520.0, 22.0], egui::TextEdit::singleline(url).hint_text(tr("https://")));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                apply = ui.add_enabled(net::looks_like_url(url), egui::Button::new(tr("    OK    "))).clicked();
                cancel = ui.button(tr("Cancel")).clicked();
            });
        });
        if cancel {
            self.refresh_dlg = None;
        } else if apply {
            let (_, url) = self.refresh_dlg.take().unwrap();
            let url = url.trim().to_string();
            let (tx, rx) = channel();
            let u = url.clone();
            std::thread::spawn(move || {
                let _ = tx.send(net::probe(&u));
            });
            self.refresh_check = Some((id, url, rx));
            self.notice = Some(tr("Checking the new address..."));
        }
    }

    /// "Add to queue" panel: moves just-added files into the queue the user picks.
    fn queue_pick_dialog(&mut self, ctx: &egui::Context) {
        let Some((ids, target)) = &mut self.queue_pick else { return };
        let queues = self.settings.queues.clone();
        let mut ok = false;
        let mut skip = false;
        egui::Window::new(tr("Add to queue")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(format!("{} {}", ids.len(), tr("file(s) were added to a queue. Choose the queue:")));
            egui::ComboBox::from_id_salt("queue_pick").selected_text(queues.iter().find(|q| q.id == *target).map(|q| tr(&q.name)).unwrap_or_default()).show_ui(ui, |ui| {
                for q in &queues {
                    ui.selectable_value(target, q.id, tr(&q.name));
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ok = ui.button(tr("    OK    ")).clicked();
                skip = ui.button(tr("Keep in the main queue")).clicked();
            });
        });
        if ok {
            let (ids, target) = self.queue_pick.take().unwrap();
            for id in ids {
                if let Some(h) = self.find(id) {
                    let mut d = h.d.lock().unwrap();
                    d.in_queue = true;
                    d.queue_id = target;
                }
            }
        } else if skip {
            self.queue_pick = None;
        }
    }

    fn check_for_updates(&mut self) {
        if self.update_rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(net::latest_release_tag(UPDATE_API_URL));
        });
        self.update_rx = Some(rx);
        self.notice = Some(tr("Checking for updates..."));
    }

    /// Polls the background update check and the new-address check.
    fn poll_background_checks(&mut self) {
        if let Some(rx) = &self.update_rx {
            if let Ok(result) = rx.try_recv() {
                self.update_rx = None;
                let current = env!("CARGO_PKG_VERSION");
                self.notice = Some(match result {
                    Ok(tag) if net::is_newer_version(&tag, current) => format!("{} {tag} ({} {current}). {}", tr("A newer version is available:"), tr("you have"), UPDATE_PAGE_URL),
                    Ok(_) => format!("{} ({current})", tr("You have the latest version")),
                    Err(e) => format!("{}: {e}", tr("Could not check for updates")),
                });
            }
        }
        if let Some((id, url, rx)) = &self.refresh_check {
            if let Ok(result) = rx.try_recv() {
                let (id, url) = (*id, url.clone());
                self.refresh_check = None;
                self.notice = Some(self.apply_refreshed_address(id, &url, result));
            }
        }
    }

    /// Accepts the new address only when it points to the same file (same size, resume still possible).
    fn apply_refreshed_address(&mut self, id: u64, url: &str, probe: Result<ProbeInfo, String>) -> String {
        let Some(h) = self.find(id) else { return tr("The download no longer exists.") };
        let info = match probe {
            Ok(i) => i,
            Err(e) => return format!("{}: {e}", tr("The new address could not be checked")),
        };
        let mut d = h.d.lock().unwrap();
        if let (Some(old), Some(new)) = (d.size, info.size) {
            if old != new {
                return format!("{} ({} / {}). {}", tr("The new address points to a file of a different size"), widgets::fmt_size(new), widgets::fmt_size(old), tr("The address was not changed."));
            }
        }
        if d.downloaded() > 0 && d.resumable && !info.resumable {
            return tr("The new server does not support resume, so the parts already downloaded could not be kept. The address was not changed.");
        }
        d.url = url.to_string();
        tr("The address was updated. The download can be resumed.")
    }

    fn props_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.props else { return };
        let Some(h) = self.find(id) else {
            self.props = None;
            return;
        };
        let d = h.d.lock().unwrap().clone();
        let mut open = true;
        egui::Window::new(tr("Properties")).open(&mut open).collapsible(false).resizable(false).constrain(true).show(ctx, |ui| {
            egui::Grid::new("props").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
                let row = |ui: &mut egui::Ui, k: &str, v: String| {
                    ui.label(RichText::new(k).strong());
                    ui.add(egui::Label::new(v).wrap());
                    ui.end_row();
                };
                row(ui, "File name", d.file_name.clone());
                row(ui, "Save To", d.save_dir.display().to_string());
                row(ui, "URL", d.url.clone());
                row(ui, "Category", d.category.name().to_string());
                row(ui, "Size", d.size.map(widgets::fmt_size).unwrap_or_else(|| "Unknown".into()));
                row(ui, "Downloaded", widgets::fmt_size(d.downloaded()));
                row(ui, "Resume capability", if d.resumable { "Yes" } else { "No" }.into());
                row(ui, "Connections", d.connections.to_string());
                row(ui, "Speed limit", d.speed_limit_kbps.map(|k| format!("{k} KB/s")).unwrap_or_else(|| "None".into()));
                row(ui, "In queue", if d.in_queue { self.settings.queue_name(d.queue_id) } else { "No".into() });
                row(ui, "Last try", d.last_try.clone());
                row(ui, "Description", d.description.clone());
            });
        });
        if !open {
            self.props = None;
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        self.queue_pick_dialog(ctx);
        self.notice_dialog(ctx);
        self.queue_edit_dialog(ctx);
        self.rename_dialog(ctx);
        self.refresh_dialog(ctx);
        self.props_dialog(ctx);
        self.add_url_dialog(ctx);
        self.file_info_dialog(ctx);
        self.progress_windows(ctx);
        self.complete_dialogs(ctx);
        self.options_dialog(ctx);
        self.scheduler_dialog(ctx);
        self.grabber_dialog(ctx);
        self.small_dialogs(ctx);
    }

    fn add_url_dialog(&mut self, ctx: &egui::Context) {
        let Some(url) = &mut self.add_url else { return };
        let mut ok = false;
        let mut cancel = false;
        egui::Window::new(tr("Enter new address to download")).collapsible(false).resizable(false).constrain(true).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr("Address:"));
                let r = ui.add(egui::TextEdit::singleline(url).desired_width(420.0).hint_text(tr("https://")));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    ok = true;
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ok |= ui.add_enabled(net::looks_like_url(url), egui::Button::new(tr("    OK    "))).clicked();
                cancel = ui.button(tr("Cancel")).clicked();
            });
        });
        if ok && net::looks_like_url(url) {
            let u = url.clone();
            self.add_url = None;
            self.open_file_info(u);
        } else if cancel {
            self.add_url = None;
        }
    }

    fn file_info_dialog(&mut self, ctx: &egui::Context) {
        let Some(fi) = &mut self.file_info else { return };
        if let Some(rx) = &fi.waiting {
            if let Ok(res) = rx.try_recv() {
                fi.waiting = None;
                match res {
                    Ok(p) => {
                        fi.file_name = p.file_name.clone();
                        let c = Category::for_file(&fi.file_name);
                        if c != fi.category {
                            fi.category = c;
                            fi.save_dir = self.settings.folder_for(c).display().to_string();
                        }
                        fi.probe = Some(p);
                    }
                    Err(e) => fi.error = Some(e),
                }
            }
        }
        let mut action = None;
        let mut cancel = false;
        const DLG_SIZE: egui::Vec2 = egui::vec2(640.0, 330.0);
        let mut vp = egui::ViewportBuilder::default().with_title("Download File Info").with_inner_size(DLG_SIZE).with_resizable(false).with_minimize_button(false).with_maximize_button(false);
        // Centered on the monitor, not the top-left corner
        if let Some(mon) = ctx.input(|i| i.viewport().monitor_size) {
            vp = vp.with_position(egui::pos2(((mon.x - DLG_SIZE.x) / 2.0).max(0.0), ((mon.y - DLG_SIZE.y) / 2.0).max(0.0)));
        }
        let mut content = |ui: &mut egui::Ui| {
            // Pop to the front for the first frames only, then behave like a normal window
            if fi.top_frames > 0 {
                fi.top_frames -= 1;
                let c = ui.ctx();
                c.send_viewport_cmd(egui::ViewportCommand::Focus);
                crate::tray::raise_window("Download File Info");
                c.request_repaint();
            }
            egui::Grid::new("fi").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label(tr("URL"));
                ui.add(egui::TextEdit::singleline(&mut fi.url.clone()).desired_width(430.0).interactive(false));
                ui.end_row();
                ui.label(tr("Category"));
                let before = fi.category;
                egui::ComboBox::from_id_salt("fi_cat").selected_text(fi.category.name()).width(200.0).show_ui(ui, |ui| {
                    for c in Category::ALL.into_iter().chain([Category::General]) {
                        ui.selectable_value(&mut fi.category, c, tr(c.name()));
                    }
                });
                if before != fi.category {
                    fi.save_dir = self.settings.folder_for(fi.category).display().to_string();
                }
                ui.end_row();
                ui.label(tr("Save To"));
                ui.add(egui::TextEdit::singleline(&mut fi.save_dir).desired_width(430.0));
                ui.end_row();
                ui.label(tr("File name"));
                ui.add(egui::TextEdit::singleline(&mut fi.file_name).desired_width(430.0));
                ui.end_row();
                ui.label(tr("Description"));
                ui.add(egui::TextEdit::singleline(&mut fi.description).desired_width(430.0));
                ui.end_row();
                ui.label(tr("Size"));
                match (&fi.probe, &fi.error, fi.waiting.is_some()) {
                    (_, _, true) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr("Getting file information..."));
                        });
                    }
                    (Some(p), _, _) => {
                        let resume = if p.resumable { "Yes" } else { "No" };
                        ui.label(format!("{}   (resume capability: {resume})", p.size.map(widgets::fmt_size).unwrap_or_else(|| "Unknown".into())));
                    }
                    (_, Some(e), _) => {
                        ui.colored_label(Color32::from_rgb(190, 40, 30), e);
                    }
                    _ => {
                        ui.label(tr("Unknown"));
                    }
                }
                ui.end_row();
            });
            if fi.duplicate {
                ui.colored_label(Color32::from_rgb(170, 100, 0), "This address is already in the download list. It will be downloaded again.");
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ok = !fi.file_name.trim().is_empty() && !fi.save_dir.trim().is_empty();
                if ui.add_enabled(ok, egui::Button::new(RichText::new(tr("Start Download")).strong())).clicked() {
                    action = Some(true);
                }
                if ui.add_enabled(ok, egui::Button::new(tr("Download Later"))).clicked() {
                    action = Some(false);
                }
                cancel = ui.button(tr("Cancel")).clicked();
            });
        };
        // Own OS window: movable and independent of the main window
        let mut close_req = false;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("file_info"), vp, |ctx, class| {
            if class == egui::ViewportClass::Embedded {
                egui::Window::new(tr("Download File Info")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| content(ui));
            } else {
                egui::CentralPanel::default().show(ctx, |ui| content(ui));
                close_req = ctx.input(|i| i.viewport().close_requested());
            }
        });
        cancel |= close_req;
        if cancel {
            self.file_info = None;
        }
        if let Some(now) = action {
            let fi = self.file_info.take().unwrap();
            if self.settings.remember_last_dir && fi.category != Category::General {
                let dir = PathBuf::from(fi.save_dir.trim());
                if !dir.as_os_str().is_empty() && dir != self.settings.folder_for(fi.category) {
                    self.settings.category_dirs.insert(fi.category.name().to_string(), dir);
                }
            }
            let h = self.new_download(&fi.url, &fi.file_name, fi.category, PathBuf::from(fi.save_dir.trim()), fi.description, fi.probe.as_ref());
            let id = h.d.lock().unwrap().id;
            if now {
                engine::start(&h);
                self.progress.push(id);
            } else {
                h.d.lock().unwrap().in_queue = true;
                if self.settings.queue_panel_on_later && self.settings.queues.len() > 1 {
                    self.queue_pick = Some((vec![id], MAIN_QUEUE_ID));
                }
            }
        }
    }

    fn progress_windows(&mut self, ctx: &egui::Context) {
        let ids = self.progress.clone();
        for id in ids {
            let Some(h) = self.find(id) else { continue };
            let mut open = true;
            let mut toggle = false;
            let mut cancel = false;
            let d = h.d.lock().unwrap().clone();
            let pct = d.progress().map(|p| format!("{:.0}% ", p * 100.0)).unwrap_or_default();
            egui::Window::new(format!("{pct}{}", d.file_name)).id(egui::Id::new(("progress", id))).open(&mut open).collapsible(true).resizable(false).constrain(true).default_width(480.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let done_tab = self.progress_tab_done.contains(&id);
                    let limiter = self.progress_tab_limiter.contains(&id) && !done_tab;
                    if ui.selectable_label(!limiter && !done_tab, tr("Download status")).clicked() {
                        self.progress_tab_limiter.remove(&id);
                        self.progress_tab_done.remove(&id);
                    }
                    if ui.selectable_label(limiter, tr("Speed Limiter")).clicked() {
                        self.progress_tab_limiter.insert(id);
                        self.progress_tab_done.remove(&id);
                    }
                    if ui.selectable_label(done_tab, tr("When done")).clicked() {
                        self.progress_tab_done.insert(id);
                    }
                });
                ui.separator();
                if self.progress_tab_done.contains(&id) {
                    ui.label(format!("Save To: {}", d.save_dir.join(&d.file_name).display()));
                    ui.add_space(6.0);
                    ui.label(tr("After all downloads are finished:"));
                    ui.radio_value(&mut self.after_all, AfterAll::Nothing, AfterAll::Nothing.label());
                    ui.radio_value(&mut self.after_all, AfterAll::ExitApp, AfterAll::ExitApp.label());
                    ui.radio_value(&mut self.after_all, AfterAll::Disconnect, AfterAll::Disconnect.label());
                    ui.horizontal(|ui| {
                        let power = AfterAll::POWER.contains(&self.after_all);
                        if ui.radio(power, tr("Turn off / restart the computer:")).clicked() && !power {
                            self.after_all = AfterAll::Shutdown;
                        }
                        egui::ComboBox::from_id_salt(("after_all", id)).selected_text(if power { self.after_all.label() } else { "Shut down" }).show_ui(ui, |ui| {
                            for a in AfterAll::POWER {
                                ui.selectable_value(&mut self.after_all, a, a.label());
                            }
                        });
                    });
                    ui.add_enabled(AfterAll::POWER.contains(&self.after_all), egui::Checkbox::new(&mut self.after_force, tr("Force close running applications")));
                    ui.label(RichText::new(tr("A 30 second countdown with a Cancel button is shown before the computer is turned off.")).small().weak());
                } else if self.progress_tab_limiter.contains(&id) {
                    let mut on = d.speed_limit_kbps.is_some();
                    let mut kbps = d.speed_limit_kbps.unwrap_or(500);
                    ui.checkbox(&mut on, tr("Use Speed Limiter"));
                    ui.add_enabled_ui(on, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(tr("Maximum download speed"));
                            ui.add(egui::DragValue::new(&mut kbps).range(1..=1_000_000).speed(10));
                            ui.label(tr("KBytes/sec"));
                        });
                    });
                    ui.label(RichText::new(tr("The setting is kept for this file when it is stopped and resumed.")).small().weak());
                    h.d.lock().unwrap().speed_limit_kbps = on.then_some(kbps);
                } else {
                    ui.label(RichText::new(&d.url).small().color(Color32::from_rgb(20, 70, 160)));
                    ui.add_space(4.0);
                    egui::Grid::new(("pg", id)).num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                        ui.label(tr("Status"));
                        ui.label(if d.message.is_empty() { status_text(&d) } else { d.message.clone() });
                        ui.end_row();
                        ui.label(tr("File size"));
                        ui.label(d.size.map(widgets::fmt_size).unwrap_or_else(|| "Unknown".into()));
                        ui.end_row();
                        ui.label(tr("Downloaded"));
                        ui.label(match d.progress() {
                            Some(p) => format!("{}  ( {:.2} % )", widgets::fmt_size(d.downloaded()), p * 100.0),
                            None => widgets::fmt_size(d.downloaded()),
                        });
                        ui.end_row();
                        ui.label(tr("Transfer rate"));
                        ui.label(widgets::fmt_speed(d.speed));
                        ui.end_row();
                        ui.label(tr("Time left"));
                        ui.label(d.time_left_secs().map(widgets::fmt_time_left).unwrap_or_default());
                        ui.end_row();
                        ui.label(tr("Resume capability"));
                        ui.label(if d.parts.is_empty() && d.size.is_none() { "Unknown" } else if d.resumable { "Yes" } else { "No" });
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.add(egui::ProgressBar::new(d.progress().unwrap_or(0.0)).fill(widgets::PROGRESS_BLUE).desired_height(16.0));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let showing = self.details.contains(&id);
                        if ui.button(if showing { "<< Hide details" } else { "Show details >>" }).clicked() {
                            if showing {
                                self.details.remove(&id);
                            } else {
                                self.details.insert(id);
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            cancel = ui.button(tr("Cancel")).clicked();
                            toggle = ui.button(if d.is_running() { "Pause" } else { "Resume" }).clicked();
                        });
                    });
                    if self.details.contains(&id) {
                        ui.add_space(6.0);
                        ui.label(format!("Start positions and download progress by connections ({} active)", d.active_connections));
                        widgets::segment_bar(ui, &d.parts, d.size, self.settings.theme);
                        let mut parts: Vec<&Part> = d.parts.iter().collect();
                        parts.sort_by_key(|p| p.start);
                        egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                            egui::Grid::new(("conns", id)).striped(true).num_columns(3).spacing([24.0, 2.0]).show(ui, |ui| {
                                ui.strong(tr("N."));
                                ui.strong(tr("Downloaded"));
                                ui.strong(tr("Info"));
                                ui.end_row();
                                for (i, p) in parts.iter().enumerate() {
                                    ui.label((i + 1).to_string());
                                    ui.label(widgets::fmt_size(p.pos - p.start));
                                    ui.label(&p.info);
                                    ui.end_row();
                                }
                            });
                        });
                    }
                }
            });
            if toggle {
                if d.is_running() { engine::stop(&h) } else { engine::start(&h) }
            }
            if cancel {
                engine::stop(&h);
                open = false;
            }
            if !open {
                self.progress.retain(|x| *x != id);
            }
        }
    }

    fn complete_dialogs(&mut self, ctx: &egui::Context) {
        let ids = self.complete.clone();
        for id in ids {
            let Some(h) = self.find(id) else { continue };
            let d = h.d.lock().unwrap().clone();
            let mut close = false;
            egui::Window::new(tr("Download complete")).id(egui::Id::new(("done", id))).collapsible(false).resizable(false).constrain(true).show(ctx, |ui| {
                egui::Grid::new(("dg", id)).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    ui.label(tr("Address"));
                    ui.label(RichText::new(&d.url).small());
                    ui.end_row();
                    ui.label(tr("Downloaded"));
                    ui.label(format!("{} ({} Bytes)", widgets::fmt_size(d.downloaded()), d.downloaded()));
                    ui.end_row();
                    ui.label(tr("The file saved as"));
                    ui.label(d.final_path().display().to_string());
                    ui.end_row();
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(tr("Open")).clicked() {
                        open_in_shell(&d.final_path(), false);
                        close = true;
                    }
                    if ui.button(tr("Open folder")).clicked() {
                        open_in_shell(&d.final_path(), true);
                        close = true;
                    }
                    close |= ui.button(tr("Close")).clicked();
                });
            });
            if close {
                self.complete.retain(|x| *x != id);
            }
        }
    }

    fn options_dialog(&mut self, ctx: &egui::Context) {
        let Some((s, tab)) = &mut self.options else { return };
        let mut result = None;
        let mut window_open = true;
        egui::Window::new(tr("Options")).open(&mut window_open).collapsible(false).resizable(true).constrain(true).default_width(900.0).min_width(900.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(tab, OptionsTab::General, tr("General"));
                ui.selectable_value(tab, OptionsTab::Connection, tr("Connection"));
                ui.selectable_value(tab, OptionsTab::Downloads, tr("Downloads"));
                ui.selectable_value(tab, OptionsTab::SaveTo, tr("Save To"));
                ui.selectable_value(tab, OptionsTab::FileTypes, tr("File Types"));
                ui.selectable_value(tab, OptionsTab::Proxy, tr("Proxy"));
                ui.selectable_value(tab, OptionsTab::Sounds, tr("Sounds"));
                ui.selectable_value(tab, OptionsTab::Passwords, tr("Password Sites"));
                ui.selectable_value(tab, OptionsTab::Antivirus, tr("Antivirus"));
            });
            ui.separator();
            match tab {
                OptionsTab::General => {
                    ui.checkbox(&mut s.watch_clipboard, tr("Automatically start downloading of URLs placed to clipboard"));
                    ui.label(RichText::new(tr("Copied links to known file types open the Download File Info dialog.")).small().weak());
                    ui.add_space(6.0);
                    ui.checkbox(&mut s.run_on_startup, tr("Run Iran Download Manager when Windows starts"));
                    ui.horizontal(|ui| {
                        ui.label(tr("Language"));
                        egui::ComboBox::from_id_salt("language").selected_text(s.language.label()).show_ui(ui, |ui| {
                            for l in [Language::English, Language::Persian] {
                                ui.selectable_value(&mut s.language, l, l.label());
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.strong(tr("Browser integration"));
                    match crate::server::seconds_since_extension_seen() {
                        Some(secs) if secs < 600 => ui.colored_label(Color32::from_rgb(0, 150, 90), format!("Browser extension: connected (last seen {secs} s ago)")),
                        Some(secs) => ui.colored_label(Color32::from_rgb(170, 100, 0), format!("Browser extension: last seen {} min ago", secs / 60)),
                        None => ui.colored_label(Color32::from_rgb(170, 100, 0), "Browser extension: not seen yet since the program started"),
                    };
                    ui.label(RichText::new(tr("Install it from the Chrome/Edge or Firefox package in the Release\\Extensions folder, then reload the browser tab.")).small().weak());
                    ui.checkbox(&mut s.ext_intercept, tr("Take over downloads of the file types listed in the File Types tab"));
                    ui.checkbox(&mut s.ext_video_button, tr("Show the \"Download this video\" button on videos"));
                    ui.horizontal(|ui| {
                        ui.label(tr("Hold this key while clicking a link to let the browser download it:"));
                        egui::ComboBox::from_id_salt("block_key").selected_text(s.block_key.name()).show_ui(ui, |ui| {
                            for k in [BlockKey::Alt, BlockKey::Ctrl, BlockKey::Shift, BlockKey::Off] {
                                ui.selectable_value(&mut s.block_key, k, k.name());
                            }
                        });
                    });
                }
                OptionsTab::Connection => {
                    ui.horizontal(|ui| {
                        ui.label(tr("Default max. conn. number"));
                        egui::ComboBox::from_id_salt("conns").selected_text(s.connections.to_string()).show_ui(ui, |ui| {
                            for n in CONNECTION_CHOICES {
                                ui.selectable_value(&mut s.connections, n, n.to_string());
                            }
                        });
                    });
                    if s.connections > DEFAULT_CONNECTIONS {
                        ui.colored_label(Color32::from_rgb(170, 100, 0), "Use many connections with care: some servers limit or block clients that open too many.");
                    }
                    ui.horizontal(|ui| {
                        ui.label(tr("Speed limit for all downloads together (0 = no limit)"));
                        ui.add(egui::DragValue::new(&mut s.global_speed_limit_kbps).range(0..=10_000_000).speed(10));
                        ui.label(tr("KBytes/sec"));
                    });
                    ui.add_space(8.0);
                    ui.checkbox(&mut s.quota.enabled, tr("Limit downloads"));
                    ui.add_enabled_ui(s.quota.enabled, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(tr("Stop after more than"));
                            ui.add(egui::DragValue::new(&mut s.quota.megabytes).range(1..=10_000_000));
                            ui.label(tr("MB in each"));
                            ui.add(egui::DragValue::new(&mut s.quota.hours).range(1..=720));
                            ui.label(tr("hour(s)"));
                        });
                    });
                    ui.add_space(8.0);
                    ui.strong(tr("Exceptions (max. connections for specific servers)"));
                    let mut remove = None;
                    egui::Grid::new("server_limits").num_columns(3).spacing([10.0, 4.0]).show(ui, |ui| {
                        for (i, l) in s.server_limits.iter_mut().enumerate() {
                            ui.add_sized([260.0, 22.0], egui::TextEdit::singleline(&mut l.host).hint_text(tr("server or *.domain.com")));
                            ui.add(egui::DragValue::new(&mut l.connections).range(1..=32));
                            if ui.button(tr("Delete")).clicked() {
                                remove = Some(i);
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(i) = remove {
                        s.server_limits.remove(i);
                    }
                    if ui.button(tr("New")).clicked() {
                        s.server_limits.push(ServerLimit { host: String::new(), connections: DEFAULT_CONNECTIONS });
                    }
                    ui.horizontal(|ui| {
                        ui.label(tr("Files downloaded at the same time in the queue"));
                        ui.add(egui::DragValue::new(&mut s.max_at_once).range(1..=20));
                    });
                }
                OptionsTab::Downloads => {
                    ui.checkbox(&mut s.show_start_dialog, tr("Show the \"Download File Info\" window when a download starts"));
                    ui.add_enabled_ui(!s.show_start_dialog, |ui| {
                        ui.indent("q_only", |ui| {
                            ui.checkbox(&mut s.add_to_queue_only, tr("Do not start the download, only add the file to the queue"));
                        });
                    });
                    ui.checkbox(&mut s.show_complete_dialog, tr("Show the \"Download complete\" window"));
                    ui.label(RichText::new(tr("These settings do not apply to the scheduler.")).small().weak());
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(tr("If the address is already in the list:"));
                        egui::ComboBox::from_id_salt("dup").selected_text(match s.duplicate_action {
                            DuplicateAction::Ask => "Show a warning in the info window",
                            DuplicateAction::AutoRename => "Download again with a new name",
                            DuplicateAction::Skip => "Do not add it, select the existing one",
                        }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut s.duplicate_action, DuplicateAction::Ask, tr("Show a warning in the info window"));
                            ui.selectable_value(&mut s.duplicate_action, DuplicateAction::AutoRename, tr("Download again with a new name"));
                            ui.selectable_value(&mut s.duplicate_action, DuplicateAction::Skip, tr("Do not add it, select the existing one"));
                        });
                    });
                    ui.add_space(8.0);
                    ui.label(tr("User-Agent for downloads (empty = default):"));
                    ui.add_sized([520.0, 22.0], egui::TextEdit::singleline(&mut s.user_agent).hint_text(tr("Mozilla/5.0 (Windows NT 10.0; Win64; x64) ...")));
                    ui.add_space(8.0);
                    ui.checkbox(&mut s.queue_panel_on_later, tr("Ask which queue to use after \"Download Later\""));
                    ui.checkbox(&mut s.queue_panel_on_batch, tr("Ask which queue to use when adding a group of files"));
                }
                OptionsTab::Antivirus => {
                    ui.checkbox(&mut s.antivirus_enabled, tr("Scan finished files with a virus scanner"));
                    ui.add_enabled_ui(s.antivirus_enabled, |ui| {
                        ui.label(tr("Program to run ({path} = the finished file):"));
                        ui.add_sized([560.0, 22.0], egui::TextEdit::singleline(&mut s.antivirus_cmd));
                        ui.horizontal(|ui| {
                            if ui.button(tr("Use Windows Defender")).clicked() {
                                s.antivirus_cmd = "\"%ProgramFiles%\\Windows Defender\\MpCmdRun.exe\" -Scan -ScanType 3 -File \"{path}\" -DisableRemediation".into();
                            }
                        });
                        ui.add_space(6.0);
                        ui.label(tr("Scan only files with these extensions (empty = every file):"));
                        ui.add(egui::TextEdit::multiline(&mut s.antivirus_extensions).desired_width(f32::INFINITY).desired_rows(2));
                        if ui.button(tr("Default settings")).clicked() {
                            s.antivirus_extensions = default_scan_extensions();
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr("The scanner runs in the background after each download finishes; its result is not shown here.")).small().weak());
                }
                OptionsTab::Proxy => {
                    ui.radio_value(&mut s.proxy.mode, ProxyMode::None, tr("Do not use a proxy"));
                    ui.radio_value(&mut s.proxy.mode, ProxyMode::System, tr("Use the Windows proxy settings"));
                    ui.radio_value(&mut s.proxy.mode, ProxyMode::Pac, tr("Use an automatic configuration script (PAC)"));
                    ui.add_enabled_ui(s.proxy.mode == ProxyMode::Pac, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(tr("Script address"));
                            ui.add_sized([420.0, 22.0], egui::TextEdit::singleline(&mut s.proxy.pac_url).hint_text(tr("http://server/proxy.pac")));
                        });
                    });
                    ui.radio_value(&mut s.proxy.mode, ProxyMode::Manual, tr("Manual proxy settings"));
                    ui.add_enabled_ui(s.proxy.mode == ProxyMode::Manual, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(tr("Type"));
                            ui.selectable_value(&mut s.proxy.kind, ProxyKind::Http, tr("HTTP"));
                            ui.selectable_value(&mut s.proxy.kind, ProxyKind::Socks5, tr("SOCKS5"));
                        });
                        egui::Grid::new("proxy").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                            ui.label(tr("Proxy"));
                            ui.add_sized([260.0, 22.0], egui::TextEdit::singleline(&mut s.proxy.host).hint_text(tr("host")));
                            ui.end_row();
                            ui.label(tr("Port"));
                            ui.add_sized([80.0, 22.0], egui::TextEdit::singleline(&mut s.proxy.port).hint_text(tr("8080")));
                            ui.end_row();
                            ui.label(tr("User name"));
                            ui.add_sized([260.0, 22.0], egui::TextEdit::singleline(&mut s.proxy.user));
                            ui.end_row();
                            ui.label(tr("Password"));
                            ui.add_sized([260.0, 22.0], egui::TextEdit::singleline(&mut s.proxy.pass).password(true));
                            ui.end_row();
                        });
                        if ui.button(tr("Get from Windows")).clicked() {
                            if let Some((h, p)) = net::system_proxy() {
                                s.proxy.host = h;
                                s.proxy.port = p;
                            }
                        }
                        ui.label(RichText::new(tr("The password is kept only while the program runs; it is not saved to disk.")).small().weak());
                    });
                }
                OptionsTab::Passwords => {
                    ui.label(tr("Sites that need a user name and password to download:"));
                    let mut remove = None;
                    egui::Grid::new("creds").striped(true).num_columns(4).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.strong(tr("Site / path"));
                        ui.strong(tr("User name"));
                        ui.strong(tr("Password"));
                        ui.strong(tr(""));
                        ui.end_row();
                        for (i, c) in s.credentials.iter_mut().enumerate() {
                            ui.add_sized([280.0, 22.0], egui::TextEdit::singleline(&mut c.pattern).hint_text(tr("http://192.168.10.21")));
                            ui.add_sized([140.0, 22.0], egui::TextEdit::singleline(&mut c.user));
                            ui.add_sized([140.0, 22.0], egui::TextEdit::singleline(&mut c.pass).password(true));
                            if ui.button(tr("Delete")).clicked() {
                                remove = Some(i);
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(i) = remove {
                        s.credentials.remove(i);
                    }
                    if ui.button(tr("New...")).clicked() {
                        s.credentials.push(Credential::default());
                    }
                    ui.label(RichText::new(tr("Passwords are encrypted with your Windows account.")).small().weak());
                }
                OptionsTab::Sounds => {
                    ui.label(tr("Play a sound when:"));
                    egui::Grid::new("sounds").num_columns(4).spacing([10.0, 6.0]).show(ui, |ui| {
                        let rows: [(&str, &mut SoundSetting); 4] = [
                            ("A download is complete", &mut s.sounds.complete),
                            ("A download failed", &mut s.sounds.failed),
                            ("The scheduler started", &mut s.sounds.scheduler_start),
                            ("The scheduler stopped", &mut s.sounds.scheduler_stop),
                        ];
                        for (label, snd) in rows {
                            ui.checkbox(&mut snd.enabled, label);
                            ui.add_sized([300.0, 22.0], egui::TextEdit::singleline(&mut snd.path).hint_text(tr("empty = system beep, or a .wav file")));
                            if ui.button(tr("Play")).clicked() {
                                sound::play_path(&snd.path);
                            }
                            ui.end_row();
                        }
                    });
                }
                OptionsTab::FileTypes => {
                    ui.label(tr("Start downloading automatically files with these extensions (separated by spaces):"));
                    ui.add(egui::TextEdit::multiline(&mut s.auto_extensions).desired_width(f32::INFINITY).desired_rows(4));
                    if ui.button(tr("Default settings")).clicked() {
                        s.auto_extensions = default_auto_extensions();
                    }
                    ui.add_space(8.0);
                    ui.label(tr("Do not start downloading automatically from these sites (separated by spaces, * allowed):"));
                    ui.add(egui::TextEdit::multiline(&mut s.excluded_sites).desired_width(f32::INFINITY).desired_rows(3));
                    if ui.button(tr("Default settings")).on_hover_text(tr("Restore the default site list")).clicked() {
                        s.excluded_sites = default_excluded_sites();
                    }
                }
                OptionsTab::SaveTo => {
                    let mut base = s.base_dir.display().to_string();
                    ui.horizontal(|ui| {
                        ui.label(tr("Main download folder"));
                        if ui.add(egui::TextEdit::singleline(&mut base).desired_width(600.0)).changed() {
                            s.base_dir = PathBuf::from(base.trim());
                        }
                        if ui.button(tr("...")).clicked() {
                            if let Some(path) = pick_folder(&s.base_dir) {
                                s.base_dir = path;
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(tr("Temporary folder for unfinished files"));
                        ui.add_sized([360.0, 22.0], egui::TextEdit::singleline(&mut s.temp_dir).hint_text(tr("empty = next to the final file")));
                        if ui.button(tr("...")).clicked() {
                            let start = PathBuf::from(&s.temp_dir);
                            if let Some(p) = pick_folder(&start) {
                                s.temp_dir = p.display().to_string();
                            }
                        }
                    });
                    ui.checkbox(&mut s.use_server_date, tr("Give the file the date reported by the server"));
                    ui.checkbox(&mut s.remember_last_dir, tr("Remember the last folder chosen for each category"));
                    ui.add_space(4.0);
                    egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| { egui::Grid::new("cats").striped(true).num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
                        ui.strong(tr("Category"));
                        ui.strong(tr("Folder"));
                        ui.strong(tr(""));
                        ui.strong(tr("File types"));
                        ui.end_row();
                        for c in Category::ALL {
                            ui.label(tr(c.name()));
                            let current = s.folder_for(c);
                            let mut dir_str = current.display().to_string();
                            if ui.add_sized([360.0, 22.0], egui::TextEdit::singleline(&mut dir_str)).changed() {
                                s.category_dirs.insert(c.name().to_string(), PathBuf::from(dir_str.trim()));
                            }
                            if ui.button(tr("...")).on_hover_text(tr("Browse folder")).clicked() {
                                let start = s.folder_for(c);
                                if let Some(path) = pick_folder(&start) {
                                    s.category_dirs.insert(c.name().to_string(), path);
                                }
                            }
                            ui.label(RichText::new(c.extensions()).small());
                            ui.end_row();
                        }
                    }); });
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(tr("    OK    ")).clicked() {
                    result = Some(true);
                }
                if ui.button(tr("Cancel")).clicked() {
                    result = Some(false);
                }
            });
        });
        if !window_open && result.is_none() {
            result = Some(false);
        }
        match result {
            Some(true) => {
                let (new_settings, _) = self.options.take().unwrap();
                set_run_on_startup(new_settings.run_on_startup);
                apply_runtime_settings(&new_settings);
                self.settings = new_settings;
                self.save_state();
            }
            Some(false) => self.options = None,
            None => {}
        }
    }

    fn scheduler_dialog(&mut self, ctx: &egui::Context) {
        if !self.scheduler {
            return;
        }
        let mut open = true;
        let mut start = false;
        let mut stop = false;
        let s = &mut self.settings;
        egui::Window::new(tr("Scheduler")).open(&mut open).collapsible(false).resizable(false).constrain(true).show(ctx, |ui| {
            ui.label(RichText::new(tr("Main download queue")).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.start_at_enabled, tr("Start download at"));
                ui.add(egui::TextEdit::singleline(&mut s.start_at).desired_width(60.0));
                if !valid_time(&s.start_at) {
                    ui.colored_label(Color32::RED, tr("use HH:MM"));
                }
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.stop_at_enabled, tr("Stop download at"));
                ui.add(egui::TextEdit::singleline(&mut s.stop_at).desired_width(60.0));
                if !valid_time(&s.stop_at) {
                    ui.colored_label(Color32::RED, tr("use HH:MM"));
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr("Files downloaded at the same time"));
                ui.add(egui::DragValue::new(&mut s.max_at_once).range(1..=20));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                start = ui.button(tr("Start now")).clicked();
                stop = ui.button(tr("Stop")).clicked();
            });
        });
        if start {
            self.start_queue();
        }
        if stop {
            self.stop_queue();
        }
        if !open {
            self.scheduler = false;
            self.save_state();
        }
    }

    fn grabber_dialog(&mut self, ctx: &egui::Context) {
        let Some(g) = &mut self.grabber else { return };
        if let Some(rx) = &g.waiting {
            if let Ok(res) = rx.try_recv() {
                g.waiting = None;
                match res {
                    Ok(links) => {
                        let types: Vec<String> = g.types.split_whitespace().map(|t| t.to_ascii_lowercase()).collect();
                        g.links = links
                            .into_iter()
                            .filter(|l| {
                                let name = net::file_name_from_url(l);
                                let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
                                types.iter().any(|t| extension_matches(t, &ext))
                            })
                            .map(|l| (true, l))
                            .collect();
                        g.error = g.links.is_empty().then(|| "No matching files were found on this page.".to_string());
                    }
                    Err(e) => g.error = Some(e),
                }
            }
        }
        let mut open = true;
        let mut add: Option<bool> = None;
        egui::Window::new(tr("Grabber")).open(&mut open).default_width(640.0).constrain(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr("Start page"));
                ui.add(egui::TextEdit::singleline(&mut g.page).desired_width(420.0).hint_text(tr("https://")));
                if ui.add_enabled(g.waiting.is_none() && net::looks_like_url(&g.page), egui::Button::new(tr("Find files"))).clicked() {
                    let (tx, rx) = channel();
                    let page = g.page.trim().to_string();
                    std::thread::spawn(move || {
                        let _ = tx.send(net::grab_links(&page));
                    });
                    g.waiting = Some(rx);
                    g.error = None;
                    g.links.clear();
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr("File types"));
                ui.add(egui::TextEdit::singleline(&mut g.types).desired_width(500.0));
            });
            ui.label(RichText::new(tr("Type file extensions separated by space (e.g. avi mpg mpeg)")).small().weak());
            ui.separator();
            if g.waiting.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr("Exploring the page..."));
                });
            }
            if let Some(e) = &g.error {
                ui.colored_label(Color32::from_rgb(190, 40, 30), e);
            }
            if !g.links.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(format!("{} files found", g.links.len()));
                    if ui.small_button(tr("Select all")).clicked() {
                        g.links.iter_mut().for_each(|l| l.0 = true);
                    }
                    if ui.small_button(tr("Select none")).clicked() {
                        g.links.iter_mut().for_each(|l| l.0 = false);
                    }
                });
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for (on, link) in &mut g.links {
                        ui.horizontal(|ui| {
                            ui.checkbox(on, net::file_name_from_url(link));
                            ui.label(RichText::new(link.as_str()).small().weak());
                        });
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(tr("Download selected now")).clicked() {
                        add = Some(true);
                    }
                    if ui.button(tr("Add selected to queue")).clicked() {
                        add = Some(false);
                    }
                });
            }
        });
        if let Some(now) = add {
            let g = self.grabber.as_ref().unwrap();
            let page = g.page.trim().to_string();
            let chosen: Vec<String> = g.links.iter().filter(|l| l.0).map(|l| l.1.clone()).collect();
            let mut added = Vec::new();
            for link in chosen {
                let name = net::file_name_from_url(&link);
                let c = Category::for_file(&name);
                let dir = self.settings.folder_for(c);
                let h = self.new_download(&link, &name, c, dir, format!("Grabbed from {page}"), None);
                let mut d = h.d.lock().unwrap();
                d.in_queue = true;
                added.push(d.id);
            }
            if !now && self.settings.queue_panel_on_batch && self.settings.queues.len() > 1 && !added.is_empty() {
                self.queue_pick = Some((added, MAIN_QUEUE_ID));
            }
            if now {
                self.start_queue();
            }
            self.grabber = None;
            self.view = View::Queue(MAIN_QUEUE_ID);
        } else if !open {
            self.grabber = None;
        }
    }

    fn small_dialogs(&mut self, ctx: &egui::Context) {
        if let Some((ids, with_files)) = &mut self.confirm_delete {
            let mut answer = None;
            egui::Window::new(tr("Confirm")).collapsible(false).resizable(false).constrain(true).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(format!("Are you sure you want to delete {} selected download(s) from the list?", ids.len()));
                ui.checkbox(with_files, "Also delete finished files from disk");
                ui.horizontal(|ui| {
                    if ui.button(tr("  Yes  ")).clicked() {
                        answer = Some(true);
                    }
                    if ui.button(tr("  No  ")).clicked() {
                        answer = Some(false);
                    }
                });
            });
            match answer {
                Some(true) => {
                    let (ids, with_files) = self.confirm_delete.take().unwrap();
                    self.delete(&ids, with_files);
                }
                Some(false) => self.confirm_delete = None,
                None => {}
            }
        }
        if self.tell_friend {
            let mut open = true;
            let message = "I use Iran Download Manager to download files faster, with pause and resume. Give it a try!";
            egui::Window::new(tr("Tell a Friend")).open(&mut open).collapsible(false).resizable(false).constrain(true).show(ctx, |ui| {
                ui.label(message);
                if ui.button(tr("Copy message")).clicked() {
                    ui.ctx().copy_text(message.to_string());
                }
            });
            self.tell_friend = open;
        }
        if self.about {
            let mut open = true;
            egui::Window::new(tr("About")).open(&mut open).collapsible(false).resizable(false).constrain(true).show(ctx, |ui| {
                ui.heading(tr("Iran Download Manager"));
                ui.label(tr("Version 0.1"));
                ui.label(tr("Splits each file into parts and downloads them at the same time, with pause and resume, categories, a queue, a scheduler and a speed limiter."));
            });
            self.about = open;
        }
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        shot.frames += 1;
        ctx.request_repaint();
        if shot.frames == SCREENSHOT_FRAME {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| i.raw.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }));
        if let Some(img) = image {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                if let Some(dir) = shot.path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                if let Err(e) = buf.save(&shot.path) {
                    eprintln!("Cannot save screenshot: {e}");
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if shot.frames > SCREENSHOT_FRAME * 5 {
            eprintln!("Screenshot was not delivered");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn status_text(d: &Download) -> String {
    tr(&status_text_en(d))
}

fn status_text_en(d: &Download) -> String {
    match d.status {
        Status::Complete => "Complete".into(),
        Status::Error => "Error".into(),
        Status::Connecting => "Connecting...".into(),
        Status::Downloading | Status::Stopped => match d.progress() {
            Some(p) if d.downloaded() > 0 => format!("{:.2}%", p * 100.0),
            _ if d.is_running() => "Receiving data...".into(),
            _ if d.in_queue => "Queued".into(),
            _ => String::new(),
        },
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // System tray: X button hides to tray; tray signals control show/exit
        #[cfg(windows)]
        {
            use std::sync::atomic::Ordering;
            if !self.really_exit && self.tray.exit_requested.load(Ordering::Relaxed) {
                self.really_exit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if self.tray.show_requested.swap(false, Ordering::Relaxed) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                self.force_top_frames = 3;
            }
            if ctx.input(|i| i.viewport().close_requested()) {
                if self.really_exit {
                    // allow normal close
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            }
        }
        // Briefly force window to front (after tray restore or extension trigger)
        if self.force_top_frames > 0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
            self.force_top_frames -= 1;
            if self.force_top_frames == 0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
            }
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::M)) && self.selected.len() == 1 && self.rename_dlg.is_none() {
            let id = *self.selected.iter().next().unwrap();
            self.open_rename(id);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Delete)) && !self.selected.is_empty() && self.confirm_delete.is_none() {
            self.confirm_delete = Some((self.selected.iter().copied().collect(), false));
        }
        self.tick(ctx);
        let menu_bg = widgets::palette(self.settings.theme).menu_bg;
        egui::TopBottomPanel::top("menu").frame(egui::Frame::new().fill(menu_bg)).show(ctx, |ui| self.menu_bar(ui, ctx));
        let toolbar_bg = widgets::palette(self.settings.theme).toolbar_bg;
        egui::TopBottomPanel::top("toolbar").frame(egui::Frame::new().fill(toolbar_bg).inner_margin(egui::Margin::symmetric(6, 3))).show(ctx, |ui| self.toolbar(ui, ctx));
        let pal = widgets::palette(self.settings.theme);
        egui::TopBottomPanel::bottom("status").frame(egui::Frame::new().fill(pal.status_bg).inner_margin(egui::Margin::symmetric(6, 3))).show(ctx, |ui| {
            let running = self.downloads.iter().filter(|h| h.d.lock().unwrap().is_running()).count();
            let speed: f64 = self.downloads.iter().map(|h| h.d.lock().unwrap().speed).sum();
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{} {}", self.downloads.len(), tr("downloads"))).color(pal.text_dim));
                ui.separator();
                ui.label(egui::RichText::new(format!("{running} {}", tr("active"))).color(pal.text_dim));
                // Total size of the files shown in the table (unknown sizes count as 0)
                let (total, unknown) = self.downloads.iter().fold((0u64, 0usize), |(t, u), h| {
                    let d = h.d.lock().unwrap();
                    if !self.matches_view(&d) { return (t, u); }
                    match d.size { Some(s) => (t + s, u), None => (t, u + 1) }
                });
                ui.separator();
                let suffix = if unknown > 0 { format!(" (+{unknown} unknown)") } else { String::new() };
                ui.label(egui::RichText::new(format!("{}: {}{suffix}", tr("Total size"), widgets::fmt_size(total))).color(pal.text_dim));
                if speed > 0.0 {
                    ui.separator();
                    ui.label(egui::RichText::new(widgets::fmt_speed(speed)).color(pal.progress_fill));
                }
                if !self.queues_running.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new(tr("Queue running")).color(pal.progress_fill));
                }
            });
        });
        egui::SidePanel::left("cats").resizable(true).default_width(200.0).frame(egui::Frame::new().fill(pal.sidebar_bg).stroke(egui::Stroke::new(1.0_f32, pal.border))).show(ctx, |ui| self.categories(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(pal.list_bg)).show(ctx, |ui| self.list(ui));
        self.dialogs(ctx);
        self.screenshot(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        for h in &self.downloads {
            engine::stop(h);
        }
        std::thread::sleep(Duration::from_millis(300));
        self.save_state();
    }
}
