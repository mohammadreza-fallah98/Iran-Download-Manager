# Architecture — Iran Download Manager (Desktop)

## Stack و نسخه‌ها

| کتابخانه | نسخه | نقش |
|---|---|---|
| Rust | 2021 edition | زبان اصلی |
| eframe | 0.31 | پنجره، event loop |
| egui | 0.31 | رندر UI فوری (immediate mode) |
| egui_extras | 0.31 | TableBuilder |
| ureq | 2 | HTTP client سینکرون |
| serde / serde_json | 1 | سریال‌سازی state |
| tiny_http | 0.12 | HTTP server محلی (پورت 6800) |
| arboard | 3 | کلیپ‌بورد |
| chrono | 0.4 | زمان‌بندی scheduler |
| image | 0.25 | ذخیره screenshot |

## ساختار پوشه

```
Frontend/
├── src/
│   ├── main.rs      entry point، راه‌اندازی server و eframe
│   ├── app.rs       ساختار App، event loop، UI کامل
│   ├── model.rs     مدل‌های داده (Download, Settings, Theme, …)
│   ├── engine.rs    منطق دانلود چندتکه‌ای در thread جداگانه
│   ├── net.rs       HTTP utilities، probe، grab links
│   ├── server.rs    HTTP server روی localhost:6800 برای اکستنشن
│   └── widgets.rs   آیکون‌ها، palette، تم دارک/لایت، custom widgets
├── Cargo.toml
└── Cargo.lock
```

## مدل داده

```rust
Download { id, url, file_name, save_dir, category, size, resumable,
           parts: Vec<Part>, status, connections, speed, … }
Part     { start, end, pos, owner }   // بازه بایتی
Settings { base_dir, connections, max_at_once, theme, … }
Theme    { Dark | Light }
```

## معماری UI (egui immediate mode)

هر فریم `App::update` فراخوانی می‌شود:
1. `tick` — به‌روزرسانی سرعت، scheduler، clipboard، server_rx
2. Panel‌ها: TopBottom (menu, toolbar, status) + SidePanel (categories) + CentralPanel (list)
3. `dialogs` — پنجره‌های modal (add, options, grabber, …)

## ارتباط UI با thread دانلود

```
UI thread                     Download threads
   App::engine::start(h)  -->  thread::spawn → engine::run(h)
   h.d.lock() (Mutex)     <->  h.d.lock() (Mutex)
   h.stop (AtomicBool)    <--  بررسی در هر chunk
```

## HTTP Server (اکستنشن مرورگر)

- پورت: `127.0.0.1:6800`
- Endpoint: `POST /add` با body JSON `{ url, file_name, referrer, description }`
- پاسخ: `{ "ok": true/false }`
- CORS: تمام origin‌ها مجاز (فقط localhost)
- channel: `mpsc::channel<AddRequest>` بین server thread و UI thread

## تم‌بندی

`widgets::palette(theme)` یک `Palette` برمی‌گرداند (۱۶ رنگ).
`widgets::make_visuals(theme)` → `egui::Visuals` و در `App::new` و toggle set می‌شود.
`settings.theme` در `downloads.json` ذخیره می‌شود.

## ذخیره state

`%APPDATA%\IranDownloadManager\downloads.json` — `SavedState { settings, downloads, next_id }`
هر ۳ ثانیه atomic write (tmp → rename).

## اکستنشن‌ها

| مرورگر | مسیر | Manifest |
|---|---|---|
| Chrome | `../extensions/chrome/` | V3 |
| Firefox | `../extensions/firefox/` | V2 |

هر دو اکستنشن به `POST http://127.0.0.1:6800/add` می‌فرستند.

## ستون‌های لیست دانلود

| ستون | توضیح |
|---|---|
| File Name | نام فایل + آیکون دسته‌بندی |
| Size | سایز کل |
| Status | وضعیت (Stopped / Connecting / Downloading / Complete / Error) |
| Time left | زمان باقی‌مانده |
| Transfer rate | سرعت لحظه‌ای |
| Last Try Date | تاریخ و ساعت آخرین تلاش |
| Queue | ✓ سبز اگر فایل در صف است |
| Description | توضیحات / آدرس منبع |

## Context Menu

**فایل ناتمام:** Resume · Stop · Show progress · Add to queue / Remove from queue · Delete  
**فایل کامل:** Open · Open folder · Delete

## استقرار

```
run-frontend.bat      →   اجرای dev (cargo run)
deploy-windows.bat    →   Release/EXE/IranDownloadManager-windows-<date>-v<ver>.exe
```
