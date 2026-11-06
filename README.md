<div align="center">

<img src="Frontend/assets/icon.png" width="120" alt="Iran Download Manager Logo"/>

# Iran Download Manager

**A free, open-source download manager for Windows — built in Rust**

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows-blue.svg)](https://github.com/mohammadreza-fallah98/Iran-Download-Manager/releases)
[![Language](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![GUI](https://img.shields.io/badge/GUI-egui-purple.svg)](https://github.com/emilk/egui)

</div>

---

## ✨ Features

| Feature | Description |
|---|---|
| ⚡ **Multi-connection download** | Up to 32 parallel connections per file for maximum speed |
| ⏸️ **Pause & Resume** | Resume interrupted downloads with byte-range precision |
| 🗂️ **Download queue** | Automatic queue with configurable concurrent download limit |
| ⏰ **Scheduler** | Start and stop downloads at a specific time of day |
| 🌐 **Browser extensions** | Chrome (MV3) and Firefox (MV2) extensions to capture links |
| 📋 **Clipboard monitor** | Automatically detects download links copied to clipboard |
| 🕷️ **Site Grabber** | Extract all download links from any web page |
| 🚦 **Speed limiter** | Per-download speed cap in KB/s |
| 🌙 **Dark / Light theme** | IDM-style dark neon theme with one-click toggle |
| 📁 **Category folders** | Auto-sorts files into Compressed, Documents, Music, Programs, Video |

---

## 📸 Screenshots

> Dark theme — IDM-style interface

![screenshot](Resource/screenshot.png)

---

## 🚀 Download

Go to [**Releases**](https://github.com/mohammadreza-fallah98/Iran-Download-Manager/releases) and download the latest `.exe`.

No installation required — just run the `.exe`.

---

## 🧩 Browser Extensions

| Browser | File | Install |
|---|---|---|
| **Chrome** | `Release/Extensions/Chrome/iran-download-manager-chrome.zip` | `chrome://extensions` → Developer mode → Load unpacked |
| **Firefox** | `Release/Extensions/Firefox/iran-download-manager-firefox.xpi` | Available on [Firefox Add-ons](https://addons.mozilla.org/) |

---

## 🛠️ Build from Source

**Requirements:** [Rust](https://rustup.rs/) (stable)

```bash
git clone https://github.com/mohammadreza-fallah98/Iran-Download-Manager.git
cd Iran-Download-Manager
run-frontend.bat          # dev run
deploy-windows.bat        # release build → Release/EXE/
```

Or manually:

```bash
cd Frontend
cargo run                 # development
cargo build --release     # release binary → target/release/iran-download-manager.exe
```

---

## 🏗️ Architecture

```
Iran-Download-Manager/
├── Backend/              No server backend (see Backend/ARCHITECTURE.md)
├── Frontend/             Rust + egui desktop app
│   ├── src/
│   │   ├── main.rs       Entry point, window, server startup
│   │   ├── app.rs        UI: toolbar, list, dialogs, theme
│   │   ├── engine.rs     Multi-threaded downloader (split, retry, throttle)
│   │   ├── model.rs      Data models (Download, Settings, Theme, …)
│   │   ├── net.rs        HTTP utilities, probe, grabber
│   │   ├── server.rs     HTTP server on localhost:6800 (for extensions)
│   │   └── widgets.rs    Icons, palette, dark/light theme, custom widgets
│   ├── assets/           App icon
│   └── extensions/
│       ├── chrome/       Manifest V3 Chrome extension
│       └── firefox/      Manifest V2 Firefox extension
├── run-frontend.bat      Dev run
└── deploy-windows.bat    Release build
```

**Key design decisions:**
- Each download runs in its own thread pool; a shared `Arc<Shared>` (Mutex + AtomicBool) coordinates UI ↔ engine
- State is persisted every 3 seconds with an atomic `tmp → rename` write to `%APPDATA%\IranDownloadManager\downloads.json`
- Browser extensions POST to `http://127.0.0.1:6800/add` via a `tiny_http` server in a background thread
- The split/takeover algorithm follows the original IDM: 50-byte takeover threshold, 256 KB minimum split

---

## 📦 Tech Stack

| Library | Version | Purpose |
|---|---|---|
| [eframe](https://github.com/emilk/egui) | 0.31 | Window & event loop |
| [egui](https://github.com/emilk/egui) | 0.31 | Immediate-mode GUI |
| [ureq](https://github.com/algesten/ureq) | 2 | HTTP client |
| [tiny_http](https://github.com/tiny-http/tiny-http) | 0.12 | Local HTTP server |
| [serde_json](https://github.com/serde-rs/json) | 1 | State persistence |
| [chrono](https://github.com/chronotope/chrono) | 0.4 | Scheduler |
| [image](https://github.com/image-rs/image) | 0.25 | Icon loading |

---

## 📄 License

This project is licensed under the **MIT License** — free for personal and commercial use.

---

<div align="center">
Made with ❤️ in Rust &nbsp;|&nbsp; Inspired by IDM
</div>
