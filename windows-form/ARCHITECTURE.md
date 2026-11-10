# معماری فنی — Iran Download Manager (ویندوز)

این پروژه سرور ندارد؛ تمام منطق داخل یک اپلیکیشن دسکتاپ (Rust + egui) است. افزونه‌های مرورگر از طریق یک HTTP server محلی با برنامه حرف می‌زنند.

## Stack

| بخش | انتخاب |
|---|---|
| زبان | Rust (edition 2021) |
| UI | `eframe` / `egui` 0.31 (چند پنجره‌ی واقعی با viewport) + `egui_extras` |
| شبکه | `ureq` 2 (همگام، چند thread) |
| سرور محلی | `tiny_http` روی `127.0.0.1:6800` |
| ذخیره‌سازی | `serde_json` → `%APPDATA%\IranDownloadManager\downloads.json` (نوشتن اتمی با `.tmp` + rename) |
| Win32 | FFI مستقیم (`user32`, `shell32`, `crypt32`, `winmm`, `kernel32`) بدون crate اضافه |
| تست | `cargo test` (واحد + چند تست یکپارچه با سرور محلی) — ۵۲ تست؛ `ttf-parser` فقط برای تست پوشش فونت (dev-dependency) |

## ساختار `src/`

| فایل | مسئولیت |
|---|---|
| `main.rs` | نقطه‌ی ورود، تک‌نمونه‌بودن، ساخت پنجره‌ی اصلی، شروع سرور محلی |
| `app.rs` | همه‌ی UI (منو، جدول، سایدبار، دیالوگ‌ها، Options)، حلقه‌ی `tick` (صف، زمان‌بند، سهمیه، کلیپ‌بورد، ذخیره، درخواست‌های افزونه) |
| `model.rs` | `Download`، `Part`، `Settings` و زیرساختارهایش، دسته‌بندی، تطبیق دامنه/پسوند، `temp_path`، `move_file_any` |
| `engine.rs` | دانلود چندقطعه‌ای HTTP: probe، `plan_parts`، worker، resume، throttle (هر دانلود + سراسری)، `finish` |
| `hls.rs` | دانلود HLS: parse playlist، چهارتا‌چهارتا segment، AES-128-CBC داخلی، الحاق |
| `dash.rs` | دانلود DASH: parse MPD، دانلود بهترین ویدیو + صدا، ادغام fMP4 بدون ffmpeg |
| `ftp.rs` | دانلود FTP (حالت passive، ادامه با `REST`، یک اتصال) |
| `i18n.rs` | زبان رابط: `tr()`، دیکشنری فارسی، شکل‌دهی حروف و ترتیب بصری راست‌به‌چپ |
| `net.rs` | کلاینت مشترک `ureq` (پروکسی HTTP/SOCKS5/PAC، User-Agent)، `get()` با Authorization/Referer/Cookie، probe، grabber، ابزار URL |
| `server.rs` | `POST /add`، `GET /config`، زمان آخرین ارتباط افزونه |
| `tray.rs` | آیکن سینی، نمایش/جلو آوردن پنجره (Win32)، `raise_window` |
| `secret.rs` | رمزنگاری رمزها با Windows DPAPI |
| `sound.rs` | پخش صدا (`PlaySoundW` / `MessageBeep`) |
| `widgets.rs` | تم، رنگ‌ها، آیکون‌ها، نوار قطعه‌ها |

## جریان‌های اصلی

**دانلود معمولی:** `Download` در `app.rs` ساخته می‌شود → `engine::start` thread می‌سازد → `prepare` (probe + `plan_parts` + ساخت فایل `.part`) → چند `worker` قطعه‌ها را با `Range` می‌گیرند → `finish` فایل را به نام نهایی منتقل می‌کند (`move_file_any`، پشتیبانی بین درایوها) و تاریخ سرور را اگر خواسته شده اعمال می‌کند.

**انتخاب موتور:** `engine::start` از روی آدرس تصمیم می‌گیرد: `ftp://` → `ftp::run`، `.m3u8` → `hls::run`، `.mpd` → `dash::run`، غیر آن → `engine::run`. پیشرفت HLS/DASH در `Download.hls` (`HlsState`) و بر حسب segment نگه‌داری می‌شود؛ DASH از resume پشتیبانی نمی‌کند.

**درخواست از مرورگر:** افزونه به `POST /add` می‌فرستد → ترد سرور `net::set_extra` (Referer/Cookie/UA) را ثبت می‌کند، پیام را در `mpsc` می‌گذارد و `tray::bring_to_front` پنجره را (حتی از سینی) جلو می‌آورد (چون `update()` روی پنجره‌ی مخفی اجرا نمی‌شود) → `tick` درخواست را می‌خواند و دیالوگ «Download File Info» (viewport مستقل، وسط مانیتور) را باز می‌کند.

**تنظیمات مشترک بین threadها:** `apply_runtime_settings` (در `app.rs`) هنگام شروع و بعد از OK در Options اجرا می‌شود: User-Agent و پروکسی (`net`)، لیست ورودها، سقف سرعت سراسری (`engine::GLOBAL_LIMIT_KBPS`)، تاریخ سرور (`model::USE_SERVER_DATE`) و JSON تنظیمات افزونه (`server::publish_config`).

**صف‌ها:** `Settings.queues` فهرست صف‌هاست (id صفر = صف اصلی)؛ هر دانلود `queue_id` دارد. `App.queues_running` مجموعه‌ی صف‌های در حال اجراست؛ `tick` برای هر صف تا `max_at_once` دانلود شروع می‌کند. زمان‌بند همه‌ی صف‌ها را با هم شروع/متوقف می‌کند.

**سهمیه و «When done»:** `tick` مجموع بایت‌های دانلودشده را در بازه‌ی چرخان می‌شمارد و در رسیدن به سقف همه را متوقف می‌کند. اکشن بعد از اتمام (`AfterAll`: خروج، خاموش، ری‌استارت، Sleep، Hibernate) با شمارش معکوس ۳۰ ثانیه‌ای و دکمه‌ی Cancel اجرا می‌شود.

## پروکسی، FTP و زبان

- **پروکسی:** `net::configure_proxy` کلاینت مشترک را می‌سازد؛ حالت‌ها: بدون پروکسی، تنظیمات ویندوز (رجیستری)، دستی (HTTP یا SOCKS5 با ویژگی `socks-proxy` در `ureq`) و **PAC**. برای PAC خود ویندوز اسکریپت را اجرا می‌کند (`WinHttpGetProxyForUrl`، بدون مفسر JS در برنامه)؛ نتیجه هر هاست ۵ دقیقه کش می‌شود و برای هر پروکسی یک کلاینت جدا ساخته می‌شود (`net::agent_for`).
- **FTP:** `ftp.rs` یک کلاینت ساده با `std::net` است (USER/PASS، TYPE I، SIZE، PASV، REST، RETR). پروکسی روی FTP اعمال نمی‌شود. پیشرفت با همان مدل `Part` نشان داده می‌شود و `engine::finish` فایل را نهایی می‌کند.
- **زبان:** `Settings.language`. egui شکل‌دهی عربی/فارسی و bidi ندارد، پس `i18n::tr` متن فارسی را با جدول اشکال حروف (Arabic Presentation Forms) می‌چسباند و به ترتیب بصری می‌برد؛ ارقام و کلمات لاتین سر جای خود می‌مانند و پرانتزها آینه می‌شوند. فونت Vazirmatn (OFL) در `assets/` به‌عنوان فونت جایگزین ثبت می‌شود. تمام متن‌های UI در `app.rs` با `tr("...")` رد می‌شوند و متنی که در دیکشنری نباشد انگلیسی می‌ماند. یک تست بررسی می‌کند همه‌ی گلیف‌های ترجمه‌ها در فونت هست. اسکرین‌شات فارسی: `IDM_LANG=fa iran-download-manager.exe --screenshot out.png`.

## قرارداد HTTP محلی (`localhost:6800`)

| مسیر | متد | توضیح |
|---|---|---|
| `/add` | POST | بدنه‌ی JSON: `url` (اجباری)، `file_name`، `description`، `referrer`، `cookies`، `user_agent`. پاسخ `{"ok":true}` |
| `/config` | GET | تنظیمات افزونه: `auto_extensions`، `excluded_sites`، `block_key`، `intercept`، `video_button` |

سرور فقط روی `127.0.0.1` گوش می‌دهد. CORS همه‌ی originها را می‌پذیرد (افزونه‌ها از origin مرورگر می‌آیند)؛ به همین دلیل کوکی‌ها فقط در حافظه نگه داشته می‌شوند و روی دیسک نمی‌روند.

## داده و امنیت

- `downloads.json` شامل تنظیمات + لیست دانلودها است؛ فیلدهای جدید همه `#[serde(default)]` دارند تا فایل قدیمی بدون مشکل بالا بیاید.
- **رمزها:** رمز «Password Sites» با DPAPI (`secret.rs`) رمزنگاری و فقط به‌صورت `pass_enc` ذخیره می‌شود؛ رمز پروکسی اصلاً ذخیره نمی‌شود. کوکی مرورگر فقط در حافظه است.
- Referer و User-Agent هر دانلود در `Download` ذخیره می‌شود تا بعد از ری‌استارت هم ادامه‌ی دانلود کار کند.
- ورود Basic به‌صورت خودکار برای آدرس‌هایی اضافه می‌شود که با پیشوند یا هاست یک ردیف ذخیره‌شده بخوانند.
- فایل‌های موقت `.part` کنار فایل نهایی یا در «Temporary folder» تنظیم‌شده‌اند (نام: `<id>-<نام>.part`).

## افزونه‌ها (`extensions/`)

| فایل | نقش |
|---|---|
| `background.js` | مشترک Chrome و Firefox: منوی راست‌کلیک، رهگیری `downloads.onCreated` بر اساس `/config`، ارسال به `/add` همراه کوکی و UA، کلید «بگذار مرورگر دانلود کند» (`BYPASS`) |
| `media.js` | ثبت پاسخ‌های media هر تب (`webRequest`) برای ویدیوهای `blob:`/HLS/DASH |
| `content.js` | نشان «IDM» کنار لینک‌های دانلود (Shadow DOM) و رهگیری کلیک |
| `video.js` | دکمه‌ی «Download this video» روی ویدیو و پنل انتخاب منبع |
| `manifest.json` | Chrome: MV3 (service worker)، Firefox: MV2 (`persistent: true` برای نگه‌داشتن لیست media) |

## تنظیمات و Options

تب‌ها: General · Connection · Downloads · Save To · File Types · Proxy · Sounds · Password Sites. هر تب یک شاخه‌ی `OptionsTab` در `options_dialog` است و مقادیرش در `Settings` (`model.rs`) ذخیره می‌شود.

## ساخت و انتشار

- اجرای لوکال: `run-frontend.bat` (`cargo run`).
- انتشار: `deploy-windows.bat` — تست‌ها را اجرا می‌کند، exe را می‌سازد و در `Release\EXE` (نام ثابت + نام تاریخ‌دار با نسخه‌ی `Cargo.toml`) می‌گذارد، افزونه‌ها را در `Release\Extensions\{Chrome,Firefox}` کپی و بسته‌ی `zip` (Chrome) و `xpi` (Firefox) را می‌سازد. نسخه‌ها از `Cargo.toml` و `manifest.json`ها خوانده می‌شوند.
- آیکن exe با `build.rs` و `rc.exe` (Windows SDK) تعبیه می‌شود؛ آیکن سینی و پنجره از `assets/icon.png` ساخته می‌شود (کتابخانه‌ی `image` فقط PNG را فعال دارد).

## محدودیت‌های شناخته‌شده

- HLS: فقط پخش غیرزنده؛ AES-128 پشتیبانی می‌شود (SAMPLE-AES و DRM نه)؛ کیفیت همیشه بهترین است. خروجی `.ts` (یا `.mp4` برای fMP4).
- DASH: ادغام داخلی فرض می‌کند fragmentها `base-data-offset` صریح ندارند؛ پخش خروجی روی ویدیوی واقعی هنوز تست نشده.
- ورود به سایت‌ها: فقط Basic. FTP: بدون TLS (FTPS) و بدون پروکسی؛ فقط حالت passive.
- فارسی: چیدمان پنجره‌ها آینه نمی‌شود (فقط متن راست‌به‌چپ می‌شود)؛ متن‌های پویا (پیام‌های خطای موتور، نام فایل‌ها) و برخی جمله‌های ساخته‌شده با `format!` ترجمه نمی‌شوند.
