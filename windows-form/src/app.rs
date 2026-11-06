use crate::engine;
use crate::model::*;
use crate::net::{self, ProbeInfo};
use crate::server::AddRequest;
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
    Queue,
}

struct FileInfo {
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
    SaveTo,
}

pub struct App {
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
    queue_running: bool,
    clipboard: Option<arboard::Clipboard>,
    last_clip: String,
    last_clip_check: Instant,
    last_save: Instant,
    last_minute: String,
    data_path: Option<PathBuf>,
    shot: Option<Shot>,
    server_rx: Receiver<AddRequest>,
}

fn data_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(base).join("IranDownloadManager").join("downloads.json"))
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

fn valid_time(t: &str) -> bool {
    chrono::NaiveTime::parse_from_str(t.trim(), "%H:%M").is_ok()
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>, server_rx: Receiver<AddRequest>) -> Self {
        let shot = screenshot.map(|path| Shot { path, frames: 0 });
        let data_path = if shot.is_some() { None } else { data_path() };
        let saved: SavedState = data_path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        cc.egui_ctx.set_visuals(widgets::make_visuals(saved.settings.theme));
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
            queue_running: false,
            clipboard,
            last_clip,
            last_clip_check: Instant::now(),
            last_save: Instant::now(),
            last_minute: String::new(),
            data_path,
            shot,
            server_rx,
        }
    }

    fn find(&self, id: u64) -> Option<Handle> {
        self.downloads.iter().find(|h| h.d.lock().unwrap().id == id).cloned()
    }

    fn save_state(&self) {
        let Some(path) = &self.data_path else { return };
        let state = SavedState { settings: self.settings.clone(), downloads: self.downloads.iter().map(|h| h.d.lock().unwrap().clone()).collect(), next_id: self.next_id };
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
            connections: self.settings.connections,
            speed_limit_kbps: None,
            in_queue: false,
            message: String::new(),
            last_speed: 0.0,
            speed: 0.0,
            active_connections: 0,
        };
        let h = new_handle(d);
        self.downloads.push(h.clone());
        h
    }

    fn open_file_info(&mut self, url: String) {
        let url = url.trim().to_string();
        let file_name = net::file_name_from_url(&url);
        let category = Category::for_file(&file_name);
        let duplicate = self.downloads.iter().any(|h| h.d.lock().unwrap().url == url);
        let (tx, rx) = channel();
        let u = url.clone();
        std::thread::spawn(move || {
            let _ = tx.send(net::probe(&u));
        });
        self.file_info = Some(FileInfo {
            url,
            file_name,
            category,
            save_dir: self.settings.folder_for(category).display().to_string(),
            description: String::new(),
            probe: None,
            waiting: Some(rx),
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
        self.queue_running = false;
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

    fn start_queue(&mut self) {
        self.queue_running = true;
    }

    fn stop_queue(&mut self) {
        self.queue_running = false;
        for h in &self.downloads {
            if h.d.lock().unwrap().in_queue {
                engine::stop(h);
            }
        }
    }

    fn matches_view(&self, d: &Download) -> bool {
        let cat_ok = |c: Option<Category>| c.is_none_or(|c| c == d.category);
        match self.view {
            View::All(c) => cat_ok(c),
            View::Unfinished(c) => d.status != Status::Complete && cat_ok(c),
            View::Finished(c) => d.status == Status::Complete && cat_ok(c),
            View::Queue => d.in_queue,
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
            if d.status == Status::Complete && prev.is_some_and(|p| p != Status::Complete) {
                if self.progress.contains(&d.id) && !self.complete.contains(&d.id) {
                    self.complete.push(d.id);
                }
                self.progress.retain(|id| *id != d.id);
            }
        }

        let minute = chrono::Local::now().format("%H:%M").to_string();
        if minute != self.last_minute {
            if !self.last_minute.is_empty() {
                if self.settings.start_at_enabled && minute == self.settings.start_at.trim() {
                    self.start_queue();
                }
                if self.settings.stop_at_enabled && minute == self.settings.stop_at.trim() {
                    self.stop_queue();
                }
            }
            self.last_minute = minute;
        }

        if self.queue_running {
            let running = self.downloads.iter().filter(|h| { let d = h.d.lock().unwrap(); d.in_queue && d.is_running() }).count() as u32;
            let waiting: Vec<Handle> = self.downloads.iter().filter(|h| { let d = h.d.lock().unwrap(); d.in_queue && d.status == Status::Stopped }).cloned().collect();
            if running == 0 && waiting.is_empty() {
                self.queue_running = false;
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
                    if net::looks_like_url(&text) && is_known_file_type(&net::file_name_from_url(text.trim())) {
                        self.open_file_info(text);
                    }
                }
            }
        }

        if now.duration_since(self.last_save) >= SAVE_EVERY {
            self.last_save = now;
            self.save_state();
        }

        // Pick up downloads sent by the browser extension
        while let Ok(req) = self.server_rx.try_recv() {
            if net::looks_like_url(&req.url) && self.file_info.is_none() {
                self.open_file_info(req.url.clone());
                // Pre-fill description with referrer so the user can see where it came from
                if let Some(fi) = self.file_info.as_mut() {
                    if !req.description.is_empty() {
                        fi.description = req.description.clone();
                    } else if !req.referrer.is_empty() {
                        fi.description = req.referrer.clone();
                    }
                    if !req.file_name.is_empty() {
                        fi.file_name = req.file_name.clone();
                    }
                }
            }
        }

        ctx.request_repaint_after(if any_running { Duration::from_millis(250) } else { Duration::from_secs(1) });
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("Tasks", |ui| {
                if ui.button("Add new download...").clicked() {
                    self.show_add_url();
                    ui.close_menu();
                }
                if ui.button("Run site grabber").clicked() {
                    self.show_grabber();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("File", |ui| {
                let has = !self.selected.is_empty();
                if ui.add_enabled(has, egui::Button::new("Download now")).clicked() {
                    self.resume_selected();
                    ui.close_menu();
                }
                if ui.add_enabled(has, egui::Button::new("Stop download")).clicked() {
                    self.stop_selected();
                    ui.close_menu();
                }
                if ui.add_enabled(has, egui::Button::new("Remove")).clicked() {
                    self.confirm_delete = Some((self.selected.iter().copied().collect(), false));
                    ui.close_menu();
                }
            });
            ui.menu_button("Downloads", |ui| {
                if ui.button("Stop all").clicked() {
                    self.stop_all();
                    ui.close_menu();
                }
                if ui.button("Delete all completed").clicked() {
                    let ids: Vec<u64> = self.downloads.iter().filter_map(|h| { let d = h.d.lock().unwrap(); (d.status == Status::Complete).then_some(d.id) }).collect();
                    self.delete(&ids, false);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Start queue").clicked() {
                    self.start_queue();
                    ui.close_menu();
                }
                if ui.button("Stop queue").clicked() {
                    self.stop_queue();
                    ui.close_menu();
                }
                if ui.button("Scheduler...").clicked() {
                    self.scheduler = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Options...").clicked() {
                    self.options = Some((self.settings.clone(), OptionsTab::General));
                    ui.close_menu();
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("All downloads").clicked() {
                    self.view = View::All(None);
                    ui.close_menu();
                }
                if ui.button("Main download queue").clicked() {
                    self.view = View::Queue;
                    ui.close_menu();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    self.about = true;
                    ui.close_menu();
                }
            });
            ui.menu_button("Registration", |ui| {
                ui.label("This rebuild is free; nothing to register.");
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
            if widgets::tool_button(ui, Icon::AddUrl, "Add URL", true, theme).clicked() {
                self.show_add_url();
            }
            if widgets::tool_button(ui, Icon::Resume, "Resume", has, theme).clicked() {
                self.resume_selected();
            }
            if widgets::tool_button(ui, Icon::Stop, "Stop", has, theme).clicked() {
                self.stop_selected();
            }
            if widgets::tool_button(ui, Icon::StopAll, "Stop All", any_running, theme).clicked() {
                self.stop_all();
            }
            if widgets::tool_button(ui, Icon::Delete, "Delete", has, theme).clicked() {
                self.confirm_delete = Some((self.selected.iter().copied().collect(), false));
            }
            ui.separator();
            if widgets::tool_button(ui, Icon::Options, "Options", true, theme).clicked() {
                self.options = Some((self.settings.clone(), OptionsTab::General));
            }
            if widgets::tool_button(ui, Icon::Scheduler, "Scheduler", true, theme).clicked() {
                self.scheduler = true;
            }
            if widgets::tool_button(ui, Icon::StartQueue, "Start Queue", !self.queue_running, theme).clicked() {
                self.start_queue();
            }
            if widgets::tool_button(ui, Icon::StopQueue, "Stop Queue", self.queue_running, theme).clicked() {
                self.stop_queue();
            }
            ui.separator();
            if widgets::tool_button(ui, Icon::Grabber, "Grabber", true, theme).clicked() {
                self.show_grabber();
            }
            if widgets::tool_button(ui, Icon::TellFriend, "Tell a Friend", true, theme).clicked() {
                self.tell_friend = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if theme == crate::model::Theme::Dark { "Light" } else { "Dark" };
                if widgets::tool_button(ui, Icon::ThemeToggle, label, true, theme).clicked() {
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
            ui.label(RichText::new("Categories").strong().color(pal.text));
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
            let label = if n > 0 { format!("{name} ({n})") } else { name.to_string() };
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
                    if widgets::tree_item(ui, 1, None, widgets::category_color(c), c.name(), self.view == make(Some(c)), theme).0 {
                        self.view = make(Some(c));
                    }
                }
            }
        }
        if widgets::tree_item(ui, 0, None, folder, "Grabber projects", false, theme).0 {
            self.show_grabber();
        }
        let (_, arrow) = widgets::tree_item(ui, 0, Some(self.open_queues), folder, "Queues", false, theme);
        if arrow {
            self.open_queues = !self.open_queues;
        }
        if self.open_queues {
            let n = count(&|_, q| q);
            let label = if n > 0 { format!("Main download queue ({n})") } else { "Main download queue".into() };
            if widgets::tree_item(ui, 1, None, Color32::from_rgb(80, 150, 220), &label, self.view == View::Queue, theme).0 {
                self.view = View::Queue;
            }
        }
    }

    fn list(&mut self, ui: &mut egui::Ui) {
        let rows: Vec<Download> = self.downloads.iter().map(|h| h.d.lock().unwrap().clone()).filter(|d| self.matches_view(d)).collect();
        let mut clicked: Option<(u64, bool)> = None;
        let mut double: Option<u64> = None;
        let mut context: Option<(u64, &'static str)> = None;
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
                        ui.label(RichText::new(title).color(hdr_color).strong());
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
                            ui.label(RichText::new("✓").color(Color32::from_rgb(0, 200, 120)));
                        }
                    });
                    row.col(|ui| {
                        ui.label(&d.description);
                    });
                    let resp = row.response();
                    if resp.clicked() {
                        clicked = Some((d.id, resp.ctx.input(|i| i.modifiers.command)));
                    }
                    if resp.double_clicked() {
                        double = Some(d.id);
                    }
                    let id = d.id;
                    let complete = d.status == Status::Complete;
                    let in_queue = d.in_queue;
                    resp.context_menu(|ui| {
                        if complete {
                            for item in &["Open", "Open folder", "Delete"] {
                                if ui.button(*item).clicked() {
                                    context = Some((id, item));
                                    ui.close_menu();
                                }
                            }
                        } else {
                            for item in &["Resume", "Stop", "Show progress"] {
                                if ui.button(*item).clicked() {
                                    context = Some((id, item));
                                    ui.close_menu();
                                }
                            }
                            let queue_label = if in_queue { "Remove from queue" } else { "Add to queue" };
                            if ui.button(queue_label).clicked() {
                                context = Some((id, queue_label));
                                ui.close_menu();
                            }
                            if ui.button("Delete").clicked() {
                                context = Some((id, "Delete"));
                                ui.close_menu();
                            }
                        }
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
            _ => {}
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
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
        egui::Window::new("Enter new address to download").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Address:");
                let r = ui.add(egui::TextEdit::singleline(url).desired_width(420.0).hint_text("https://"));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    ok = true;
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ok |= ui.add_enabled(net::looks_like_url(url), egui::Button::new("    OK    ")).clicked();
                cancel = ui.button("Cancel").clicked();
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
        egui::Window::new("Download File Info").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            egui::Grid::new("fi").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("URL");
                ui.add(egui::TextEdit::singleline(&mut fi.url.clone()).desired_width(430.0).interactive(false));
                ui.end_row();
                ui.label("Category");
                let before = fi.category;
                egui::ComboBox::from_id_salt("fi_cat").selected_text(fi.category.name()).width(200.0).show_ui(ui, |ui| {
                    for c in Category::ALL.into_iter().chain([Category::General]) {
                        ui.selectable_value(&mut fi.category, c, c.name());
                    }
                });
                if before != fi.category {
                    fi.save_dir = self.settings.folder_for(fi.category).display().to_string();
                }
                ui.end_row();
                ui.label("Save To");
                ui.add(egui::TextEdit::singleline(&mut fi.save_dir).desired_width(430.0));
                ui.end_row();
                ui.label("File name");
                ui.add(egui::TextEdit::singleline(&mut fi.file_name).desired_width(430.0));
                ui.end_row();
                ui.label("Description");
                ui.add(egui::TextEdit::singleline(&mut fi.description).desired_width(430.0));
                ui.end_row();
                ui.label("Size");
                match (&fi.probe, &fi.error, fi.waiting.is_some()) {
                    (_, _, true) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Getting file information...");
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
                        ui.label("Unknown");
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
                if ui.add_enabled(ok, egui::Button::new(RichText::new("Start Download").strong())).clicked() {
                    action = Some(true);
                }
                if ui.add_enabled(ok, egui::Button::new("Download Later")).clicked() {
                    action = Some(false);
                }
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel {
            self.file_info = None;
        }
        if let Some(now) = action {
            let fi = self.file_info.take().unwrap();
            let h = self.new_download(&fi.url, &fi.file_name, fi.category, PathBuf::from(fi.save_dir.trim()), fi.description, fi.probe.as_ref());
            let id = h.d.lock().unwrap().id;
            if now {
                engine::start(&h);
                self.progress.push(id);
            } else {
                h.d.lock().unwrap().in_queue = true;
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
            egui::Window::new(format!("{pct}{}", d.file_name)).id(egui::Id::new(("progress", id))).open(&mut open).collapsible(true).resizable(false).default_width(480.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let limiter = self.progress_tab_limiter.contains(&id);
                    if ui.selectable_label(!limiter, "Download status").clicked() {
                        self.progress_tab_limiter.remove(&id);
                    }
                    if ui.selectable_label(limiter, "Speed Limiter").clicked() {
                        self.progress_tab_limiter.insert(id);
                    }
                });
                ui.separator();
                if self.progress_tab_limiter.contains(&id) {
                    let mut on = d.speed_limit_kbps.is_some();
                    let mut kbps = d.speed_limit_kbps.unwrap_or(500);
                    ui.checkbox(&mut on, "Use Speed Limiter");
                    ui.add_enabled_ui(on, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Maximum download speed");
                            ui.add(egui::DragValue::new(&mut kbps).range(1..=1_000_000).speed(10));
                            ui.label("KBytes/sec");
                        });
                    });
                    ui.label(RichText::new("The setting is kept for this file when it is stopped and resumed.").small().weak());
                    h.d.lock().unwrap().speed_limit_kbps = on.then_some(kbps);
                } else {
                    ui.label(RichText::new(&d.url).small().color(Color32::from_rgb(20, 70, 160)));
                    ui.add_space(4.0);
                    egui::Grid::new(("pg", id)).num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                        ui.label("Status");
                        ui.label(if d.message.is_empty() { status_text(&d) } else { d.message.clone() });
                        ui.end_row();
                        ui.label("File size");
                        ui.label(d.size.map(widgets::fmt_size).unwrap_or_else(|| "Unknown".into()));
                        ui.end_row();
                        ui.label("Downloaded");
                        ui.label(match d.progress() {
                            Some(p) => format!("{}  ( {:.2} % )", widgets::fmt_size(d.downloaded()), p * 100.0),
                            None => widgets::fmt_size(d.downloaded()),
                        });
                        ui.end_row();
                        ui.label("Transfer rate");
                        ui.label(widgets::fmt_speed(d.speed));
                        ui.end_row();
                        ui.label("Time left");
                        ui.label(d.time_left_secs().map(widgets::fmt_time_left).unwrap_or_default());
                        ui.end_row();
                        ui.label("Resume capability");
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
                            cancel = ui.button("Cancel").clicked();
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
                                ui.strong("N.");
                                ui.strong("Downloaded");
                                ui.strong("Info");
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
            egui::Window::new("Download complete").id(egui::Id::new(("done", id))).collapsible(false).resizable(false).show(ctx, |ui| {
                egui::Grid::new(("dg", id)).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    ui.label("Address");
                    ui.label(RichText::new(&d.url).small());
                    ui.end_row();
                    ui.label("Downloaded");
                    ui.label(format!("{} ({} Bytes)", widgets::fmt_size(d.downloaded()), d.downloaded()));
                    ui.end_row();
                    ui.label("The file saved as");
                    ui.label(d.final_path().display().to_string());
                    ui.end_row();
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Open").clicked() {
                        open_in_shell(&d.final_path(), false);
                        close = true;
                    }
                    if ui.button("Open folder").clicked() {
                        open_in_shell(&d.final_path(), true);
                        close = true;
                    }
                    close |= ui.button("Close").clicked();
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
        egui::Window::new("Options").collapsible(false).resizable(false).default_width(520.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(tab, OptionsTab::General, "General");
                ui.selectable_value(tab, OptionsTab::Connection, "Connection");
                ui.selectable_value(tab, OptionsTab::SaveTo, "Save To");
            });
            ui.separator();
            match tab {
                OptionsTab::General => {
                    ui.checkbox(&mut s.watch_clipboard, "Automatically start downloading of URLs placed to clipboard");
                    ui.label(RichText::new("Copied links to known file types open the Download File Info dialog.").small().weak());
                }
                OptionsTab::Connection => {
                    ui.horizontal(|ui| {
                        ui.label("Default max. conn. number");
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
                        ui.label("Files downloaded at the same time in the queue");
                        ui.add(egui::DragValue::new(&mut s.max_at_once).range(1..=20));
                    });
                }
                OptionsTab::SaveTo => {
                    let mut base = s.base_dir.display().to_string();
                    ui.horizontal(|ui| {
                        ui.label("Main download folder");
                        if ui.add(egui::TextEdit::singleline(&mut base).desired_width(330.0)).changed() {
                            s.base_dir = PathBuf::from(base.trim());
                        }
                    });
                    ui.add_space(4.0);
                    egui::Grid::new("cats").striped(true).num_columns(3).show(ui, |ui| {
                        ui.strong("Category");
                        ui.strong("Folder");
                        ui.strong("File types");
                        ui.end_row();
                        for c in Category::ALL {
                            ui.label(c.name());
                            ui.label(s.folder_for(c).display().to_string());
                            ui.label(RichText::new(c.extensions()).small());
                            ui.end_row();
                        }
                    });
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("    OK    ").clicked() {
                    result = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    result = Some(false);
                }
            });
        });
        match result {
            Some(true) => {
                self.settings = self.options.take().unwrap().0;
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
        egui::Window::new("Scheduler").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new("Main download queue").strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.start_at_enabled, "Start download at");
                ui.add(egui::TextEdit::singleline(&mut s.start_at).desired_width(60.0));
                if !valid_time(&s.start_at) {
                    ui.colored_label(Color32::RED, "use HH:MM");
                }
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.stop_at_enabled, "Stop download at");
                ui.add(egui::TextEdit::singleline(&mut s.stop_at).desired_width(60.0));
                if !valid_time(&s.stop_at) {
                    ui.colored_label(Color32::RED, "use HH:MM");
                }
            });
            ui.horizontal(|ui| {
                ui.label("Files downloaded at the same time");
                ui.add(egui::DragValue::new(&mut s.max_at_once).range(1..=20));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                start = ui.button("Start now").clicked();
                stop = ui.button("Stop").clicked();
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
        egui::Window::new("Grabber").open(&mut open).default_width(640.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Start page");
                ui.add(egui::TextEdit::singleline(&mut g.page).desired_width(420.0).hint_text("https://"));
                if ui.add_enabled(g.waiting.is_none() && net::looks_like_url(&g.page), egui::Button::new("Find files")).clicked() {
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
                ui.label("File types");
                ui.add(egui::TextEdit::singleline(&mut g.types).desired_width(500.0));
            });
            ui.label(RichText::new("Type file extensions separated by space (e.g. avi mpg mpeg)").small().weak());
            ui.separator();
            if g.waiting.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Exploring the page...");
                });
            }
            if let Some(e) = &g.error {
                ui.colored_label(Color32::from_rgb(190, 40, 30), e);
            }
            if !g.links.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(format!("{} files found", g.links.len()));
                    if ui.small_button("Select all").clicked() {
                        g.links.iter_mut().for_each(|l| l.0 = true);
                    }
                    if ui.small_button("Select none").clicked() {
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
                    if ui.button("Download selected now").clicked() {
                        add = Some(true);
                    }
                    if ui.button("Add selected to queue").clicked() {
                        add = Some(false);
                    }
                });
            }
        });
        if let Some(now) = add {
            let g = self.grabber.as_ref().unwrap();
            let page = g.page.trim().to_string();
            let chosen: Vec<String> = g.links.iter().filter(|l| l.0).map(|l| l.1.clone()).collect();
            for link in chosen {
                let name = net::file_name_from_url(&link);
                let c = Category::for_file(&name);
                let dir = self.settings.folder_for(c);
                let h = self.new_download(&link, &name, c, dir, format!("Grabbed from {page}"), None);
                h.d.lock().unwrap().in_queue = true;
            }
            if now {
                self.start_queue();
            }
            self.grabber = None;
            self.view = View::Queue;
        } else if !open {
            self.grabber = None;
        }
    }

    fn small_dialogs(&mut self, ctx: &egui::Context) {
        if let Some((ids, with_files)) = &mut self.confirm_delete {
            let mut answer = None;
            egui::Window::new("Confirm").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(format!("Are you sure you want to delete {} selected download(s) from the list?", ids.len()));
                ui.checkbox(with_files, "Also delete finished files from disk");
                ui.horizontal(|ui| {
                    if ui.button("  Yes  ").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("  No  ").clicked() {
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
            egui::Window::new("Tell a Friend").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label(message);
                if ui.button("Copy message").clicked() {
                    ui.ctx().copy_text(message.to_string());
                }
            });
            self.tell_friend = open;
        }
        if self.about {
            let mut open = true;
            egui::Window::new("About").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.heading("Iran Download Manager");
                ui.label("Version 0.1");
                ui.label("Splits each file into parts and downloads them at the same time, with pause and resume, categories, a queue, a scheduler and a speed limiter.");
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
                ui.label(egui::RichText::new(format!("{} downloads", self.downloads.len())).color(pal.text_dim));
                ui.separator();
                ui.label(egui::RichText::new(format!("{running} active")).color(pal.text_dim));
                if speed > 0.0 {
                    ui.separator();
                    ui.label(egui::RichText::new(widgets::fmt_speed(speed)).color(pal.progress_fill));
                }
                if self.queue_running {
                    ui.separator();
                    ui.label(egui::RichText::new("Queue running").color(pal.progress_fill));
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
