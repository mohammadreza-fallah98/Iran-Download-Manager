# پلن ویژگی‌ها — هم‌ترازی با IDM + دکمه‌ی دانلود روی ویدیو

> وضعیت: **فقط پلن؛ هنوز هیچ‌چیز پیاده‌سازی نشده.** منبع استخراج: اسکرین‌شات‌های تنظیمات، پنجره‌ی پیشرفت دانلود و منوی راست‌کلیک برنامه‌ی Internet Download Manager.
> سورس برنامه: `WindowsForm/` (Rust + egui). افزونه‌ها: `WindowsForm/extensions/{chrome,firefox}`.
> علامت‌ها: ✅ هست · 🟡 نیمه‌کاره · ❌ نیست. اولویت: **P1** (اول) · **P2** · **P3** (آخر / اختیاری).
> متن UI برنامه همان انگلیسی فعلی می‌ماند (فارسی‌سازی خارج از این پلن است، مگر بگویی).

---

## ۰. وضعیت فعلی (از روی کد)

| بخش | چه داریم |
|---|---|
| Options | ۳ تب: General (clipboard، run on startup) · Connection (تعداد اتصال پیش‌فرض، تعداد هم‌زمان در صف) · Save To (پوشه‌ی اصلی + پوشه‌ی هر دسته) |
| دسته‌بندی | Compressed / Documents / Music / Programs / Video / General با پسوندهای ثابت (`model.rs`) |
| Download | `speed_limit_kbps` در مدل هست؛ `connections`، `in_queue`، `description`، parts چندقطعه‌ای |
| منوی راست‌کلیک ردیف | Open / Open folder / Delete (کامل‌شده) · Resume / Stop / Show progress / Add-Remove queue / Delete (ناقص) |
| پنجره‌ی پیشرفت | جزئیات + جدول اتصال‌ها (در `app.rs`)؛ بدون تب‌های محدودکننده‌ی سرعت و گزینه‌های اتمام |
| Scheduler / Grabber / Queue | ✅ |
| سینی سیستم، تک‌نمونه | ✅ (همین سشن اصلاح شد) |
| افزونه‌ی مرورگر | Chrome/Firefox: منوی راست‌کلیک (link / video / audio) + رهگیری `downloads.onCreated` → `POST localhost:6800/add` |

---

## ۱. دکمه‌ی دانلود روی ویدیو در مرورگر (درخواست ویژه) — **P1**

**هدف:** وقتی در مرورگر ویدیویی پخش می‌شود، روی خود ویدیو یک دکمه‌ی «Download this video» نمایش داده شود؛ با کلیک، دانلود شروع شود.

### رفتار
1. افزونه در هر صفحه `<video>` (و `<audio>` اختیاری) را شناسایی می‌کند (شامل iframeها و ویدیوهای بعد از لود پویا با `MutationObserver`).
2. وقتی ویدیو **در حال پخش** است یا ماوس روی آن می‌رود، یک دکمه‌ی شناور (بالا‌-راست ویدیو، قابل جابه‌جایی) ظاهر می‌شود و بعد از چند ثانیه بی‌حرکتی محو می‌شود.
3. کلیک روی دکمه:
   - اگر فقط یک منبع پیدا شده → مستقیم `POST /add` و دانلود شروع می‌شود (طبق تنظیم: با پنجره‌ی File Info یا بی‌صدا).
   - اگر چند کیفیت/منبع پیدا شده (۳۶۰p/۷۲۰p/…) → یک پنل کوچک لیست کیفیت‌ها (نام، پسوند، حجم) نشان داده می‌شود و کاربر انتخاب می‌کند.
4. برنامه‌ی دسکتاپ فایل را با نام ویدیو (عنوان صفحه/`title`) در دسته‌ی Video ذخیره می‌کند.

### فنی
| مورد | راه‌حل |
|---|---|
| منبع ساده (`video.src` / `<source>` به mp4/webm) | مستقیم URL را بفرست؛ همراه `Referer`، `User-Agent`، `Cookie` صفحه (برای سایت‌هایی که hotlink را می‌بندند) |
| `blob:` / MSE (یوتیوب، آپارات…) | URL قابل‌دانلود نیست؛ با `chrome.webRequest.onResponseStarted/onSendHeaders` در `background.js` درخواست‌های media (Content-Type `video/*`، `audio/*`، `application/vnd.apple.mpegurl`، `application/dash+xml`، پسوند `.mp4 .webm .m3u8 .mpd .ts .m4s`) را به‌ازای هر tab ثبت و به دکمه نسبت بده |
| HLS (`.m3u8`) | فاز بعد: دانلود segmentها + الحاق؛ ابتدا فقط mp4/webm مستقیم |
| DASH (ویدیو و صدا جدا) | فاز بعد: نیاز به mux (ffmpeg اختیاری)؛ در فاز اول اعلام «پشتیبانی نمی‌شود» |
| DRM (Netflix و…) | پشتیبانی نمی‌شود؛ دکمه نمایش داده نشود |
| سایت‌های استثنا | لیست «دکمه را در این سایت‌ها نشان نده» |

### تغییرات لازم
- `extensions/*/content.js`: تشخیص ویدیو، رندر دکمه (Shadow DOM تا CSS سایت به آن آسیب نزند).
- `extensions/*/background.js`: ثبت media درخواست‌ها به‌ازای tab، API داخلی `getMediaForTab`.
- `extensions/*/manifest.json`: مجوز `webRequest`، `tabs`، `cookies` (فقط در صورت نیاز).
- `extensions/*/popup.html/js` + صفحه‌ی options افزونه: روشن/خاموش، حداقل حجم ویدیو، لیست سایت‌های استثنا، موقعیت دکمه.
- `src/server.rs`: فیلدهای جدید در `/add`: `referer`، `cookies`، `userAgent`، `title`، `startNow`، `headers`.
- `src/engine.rs` + `net.rs`: ارسال همان هدرها در همه‌ی درخواست‌های (probe و قطعه‌ها) آن دانلود؛ `Download` فیلد `referer`، `cookies`، `user_agent` می‌گیرد (با `#[serde(default)]`).
- تست: unit برای parse هدرها در `/add`؛ دستی در Chrome/Edge/Firefox روی یک mp4 مستقیم.

### معیار پذیرش
ویدیوی mp4 مستقیم روی یک سایت باز شود ← دکمه دیده شود ← کلیک ← دانلود در برنامه شروع شود و فایل در `Video` ذخیره شود؛ روی سایت با hotlink-protection هم (به‌خاطر Referer) کار کند.

### ۱.۱ تصمیم‌های گرفته‌شده برای بخش ۱
- **HLS و DASH مستقیم در همین فاز پیاده شوند** (نه «پشتیبانی نمی‌شود»). یعنی: خواندن playlist `.m3u8` (master و media)، دانلود segmentها با چند اتصال، الحاق به فایل نهایی؛ برای DASH (صدا و تصویر جدا) mux نهایی.
- **mux/الحاق — بدون ffmpeg (تصمیم جدید، حجم تقریباً صفر):** IDM هم ffmpeg همراه ندارد و خودش الحاق می‌کند. ما هم داخل برنامه (Rust) انجام می‌دهیم:

  | سناریو | روش بدون ffmpeg | حجم اضافه |
  |---|---|---|
  | HLS با segment از نوع `.ts` (رایج‌ترین) | segmentها به ترتیب پشت هم نوشته می‌شوند (الحاق باینری TS معتبر است)؛ خروجی `.ts` یا با remux داخلی به `.mp4` | ۰ برای `.ts`؛ remuxer ≈ چند‌صد KB |
  | HLS با segment از نوع fMP4 (`.m4s` + init) | `init + segmentها` پشت هم = یک fragmented MP4 قابل‌پخش | ۰ |
  | DASH، ویدیو و صدا **جدا** | ۱) اگر سایت نسخه‌ی ترکیبی (progressive) هم دارد همان را انتخاب کن؛ ۲) وگرنه **muxer داخلی fMP4** (ادغام دو track در یک فایل: یکی‌کردن `moov`ها و ترتیب‌دادن `moof/mdat`)؛ ۳) در بدترین حالت دو فایل جدا ذخیره و به کاربر اعلام شود | ≈ چند‌صد KB کد |
  | کاربری که ffmpeg دارد | اگر `ffmpeg` در PATH بود از آن استفاده شود (اختیاری، بدون حجم) | ۰ |

  - **ffmpeg همراه برنامه نمی‌شود** و اجباری هم نیست.
  - ریسک: muxer داخلی DASH کار فنی دقیقی است (ساختار box های MP4)؛ ابتدا HLS (ساده) و DASH ترکیبی، بعد muxer. قبل از شروع یک spike کوتاه می‌زنم و کتابخانه‌های Rust موجود (مثل `mp4`، `mpeg2ts-reader`) را از نظر لایسنس/حجم می‌سنجم؛ **هنوز تأیید نشده که کدام کتابخانه مناسب است.**
- رمزگذاری AES-128 در HLS (کلید ساده در playlist) پشتیبانی شود؛ DRM (Widevine/FairPlay) خیر.
- مدل `Download` یک نوع جدید می‌گیرد: `kind: Direct | Hls | Dash` با `segments` و `audio_url`؛ منطق resume بر اساس شماره‌ی segment.

---

## ۱.۵ باگ‌ها و بهبودهای افزونه/پنجره که کاربر گزارش داد — **P0 (اول از همه)**

### ۱.۵.۱ نشان IDM روی لینک‌ها: نشانگر ماوس (cursor) دست نمی‌شود
- **مشاهده:** روی لینک‌های دانلود در Firefox نشان نارنجی «IDM ↓» می‌آید ولی با رفتن ماوس روی آن، نشانگر به شکل دست نمی‌شود و کاربر نمی‌فهمد کلیک‌شدنی است.
- **وضعیت کد:** `content.js` برای `<span>` نشان `cursor:pointer` را inline دارد، پس علت قطعی نیست (احتمال: CSS سایت با `!important`، `pointer-events`، یا اینکه نسخه‌ی نصب‌شده‌ی افزونه قدیمی/موقت است). **باید در Firefox روی همان سایت بررسی (inspect) شود؛ حدس نمی‌زنم.**
- **راه‌حل:**
  1. نشان را به‌جای `span` یک `<button>`/`<a role="button" tabindex="0">` داخل **Shadow DOM** بسازیم تا CSS سایت روی آن اثر نگذارد.
  2. `cursor: pointer !important; pointer-events: auto !important; user-select: none`.
  3. حالت hover (روشن‌تر شدن/بزرگ‌تر شدن) + tooltip واضح «Download with Iran Download Manager» + حالت focus با کیبورد.
  4. نشان وقتی لینک داخل `<a>` تو در تو یا کنار آیکن‌های سایت است نباید لینک سایت را فعال کند (`stopPropagation`، `preventDefault`).
- **معیار پذیرش:** روی نشان، دست نمایش داده شود و کلیک فقط دانلود را شروع کند، نه ناوبری لینک.

### ۱.۵.۲ برنامه در سینی (کنار ساعت) است و کلیک روی لینک دانلود، برنامه را جلو نمی‌آورد
- **ریشه‌ی مشکل (همان ریشه‌ی باگ تری که قبلاً درست شد):** درخواست افزونه در `server_rx` می‌ماند و فقط داخل `update()` خوانده می‌شود؛ وقتی پنجره مخفی است `update()` اجرا نمی‌شود، پس نه دیالوگ باز می‌شود نه پنجره ظاهر.
- **راه‌حل:**
  1. هنگام دریافت `/add` در ترد سرور (`server.rs`)، مثل `tray::request_show()` پنجره را مستقیم با Win32 نشان بدهد (`ShowWindow` + `SetForegroundWindow`) و `ctx.request_repaint()` بزند (ترد سرور `egui::Context` بگیرد).
  2. چون ویندوز اجازه‌ی foreground گرفتن به پروسه‌ی پس‌زمینه را نمی‌دهد: ترفند استاندارد `AttachThreadInput` یا `SetWindowPos(HWND_TOPMOST)` سپس بازگشت به `HWND_NOTOPMOST`، و `FlashWindowEx` به‌عنوان پشتیبان.
  3. **در هر حالتی** کار کند: مخفی در سینی · minimize شده · پشت پنجره‌های دیگر · روی دسکتاپ مجازی دیگر.
  4. گزینه در Options: «When a download is sent from browser: bring window to front» (پیش‌فرض روشن).
- **معیار پذیرش:** برنامه در سینی؛ روی لینک دانلود در Firefox/Chrome کلیک ← در کمتر از ۱ ثانیه دیالوگ دانلود جلوی همه‌ی پنجره‌ها دیده شود.

### ۱.۵.۳ دیالوگ «Download File Info» داخل پنجره‌ی اصلی باز می‌شود و جابه‌جا نمی‌شود
- **مشاهده:** فرم وسط برنامه می‌آید، قابل جابه‌جایی نیست و بیرون از برنامه هم نمی‌آید (تمام دیالوگ‌ها `egui::Window` داخل پنجره‌ی اصلی‌اند).
- **راه‌حل:** دیالوگ‌ها به **پنجره‌ی سیستم‌عامل مستقل** تبدیل شوند (egui viewport: `show_viewport_immediate` / `ViewportBuilder` با عنوان، اندازه‌ی ثابت، `always_on_top`، قابل جابه‌جایی، آیکن برنامه، بدون تأثیر بر پنجره‌ی اصلی).
  - شامل: **Download File Info**، **Add URL**، **Download complete**، **پنجره‌ی پیشرفت هر دانلود**، و (اختیاری) Options / Scheduler / Grabber.
  - دیالوگ File Info که از افزونه می‌آید باید **بدون نمایش پنجره‌ی اصلی** هم بتواند باز شود (برنامه در سینی بماند، فقط دیالوگ بیاید) — شبیه رفتار IDM.
- **ریسک فنی مهم (باید اول spike شود، ۱–۲ ساعت):** وقتی پنجره‌ی اصلی مخفی است، `eframe` برای viewportهای فرزند هم `update()` را اجرا نمی‌کند. دو راه:
  - (الف) به‌جای «مخفی کردن» پنجره‌ی اصلی، آن را **minimize + از taskbar حذف** کنیم (`WS_EX_TOOLWINDOW`) تا حلقه‌ی رندر زنده بماند؛
  - (ب) پنجره‌ی اصلی را با `Visible(false)` نگه داریم و برای دیالوگ‌ها یک پروسه/ترد مستقل با event loop خودش بسازیم (پیچیده‌تر).
  - پیشنهاد: ابتدا spike (الف)؛ اگر شکست خورد (ب).
- **معیار پذیرش:** دیالوگ File Info یک پنجره‌ی مستقل است، با ماوس جابه‌جا و بیرون پنجره‌ی اصلی برده می‌شود، همیشه روی بقیه است، و وقتی برنامه در سینی است هم باز می‌شود.

### ۱.۵.۴ خلاصه‌ی فایل‌های درگیر
| مورد | فایل‌ها |
|---|---|
| ۱.۵.۱ | `extensions/firefox/content.js`، `extensions/chrome/content.js` |
| ۱.۵.۲ | `src/server.rs`، `src/tray.rs` (استفاده‌ی مشترک از تابع نمایش پنجره)، `src/app.rs` (تنظیم جدید) |
| ۱.۵.۳ | `src/app.rs` (تبدیل `file_info_dialog`، `add_url_dialog`، `progress_windows` و… به viewport)، احتمالاً `src/main.rs` |

---

## ۲. Options — تب‌ها (طبق IDM)

IDM این تب‌ها را دارد: عمومی · ارتباط با شبکه · دانلودها · ذخیره در · نوع فایل · سایت‌های پسوورد‌دار · VPN/مودم · صداها · Socks/پروکسی.
برنامه‌ی ما: General · Connection · Save To → **باید به ۹ تب برسد** (VPN/Dial-up را در P3 می‌گذاریم یا حذف).

### ۲.۱ تب General (عمومی) — **P1/P2**

| گزینه‌ی IDM | وضعیت | کار |
|---|---|---|
| Advanced browser integration is enabled + دکمه‌ی Restart | ❌ | نمایش وضعیت اتصال افزونه (ping به `localhost:6800`) + راهنمای نصب افزونه؛ دکمه‌ی «Check connection» |
| شروع IDM همراه بالا آمدن ویندوز | ✅ | `run_on_startup` |
| لینک‌های حافظه (clipboard) را خودکار دانلود کن | ✅ | `watch_clipboard` |
| لیست مرورگرها (Chrome، Edge، Firefox، Safari، IE، Edge Legacy …) با تیک «دانلود از این مرورگرها» | ❌ | جدول مرورگرها با وضعیت نصب افزونه و تیک فعال/غیرفعال؛ دکمه‌ی «Add browser…» برای مرورگر دلخواه (مسیر exe) |
| «کلیدها…» — کلید جلوگیری از دانلود توسط IDM | ❌ | انتخاب کلید (Alt / Ctrl / Shift) که با نگه‌داشتنش هنگام کلیک، افزونه دانلود را رهگیری نکند؛ ذخیره در افزونه (`chrome.storage`) و Settings |
| ویرایش موارد منوی راست‌کلیک مرورگر | ❌ | صفحه‌ی options افزونه: تیک «Download with IDM»، «Download all links»، «Download video/audio»، «Add to queue» |
| ویرایش تنظیمات پنل دانلود در مرورگرها | ❌ | همان تنظیمات دکمه‌ی روی ویدیو (بخش ۱): فعال، حداقل حجم، موقعیت، سایت‌های استثنا |

### ۲.۲ تب Connection (ارتباط با شبکه) — **P2**

| گزینه | وضعیت | کار |
|---|---|---|
| حداکثر تعداد اتصال به سرور در هنگام دانلود (کشویی، پیش‌فرض ۸) | ✅ | موجود (`connections`) |
| لیست «استثناها» (سرور ↔ تعداد اتصال) + New / Edit / Delete | ❌ | `Settings.server_connection_overrides: Vec<{host, connections}>`؛ `engine` هنگام شروع، میزبان URL را با لیست تطبیق دهد (پشتیبانی از wildcard `*.example.com`) |
| محدودسازی دانلودها: «بیش از X مگابایت در هر Y ساعت، دانلود کن / قبل از توقف هر دانلود اخطار بده» | ❌ | `Settings.quota { enabled, mb, hours, warn_before_stop }`؛ شمارنده‌ی بایت‌های دانلود‌شده در بازه‌ی چرخان؛ رسیدن به سقف ← توقف صف + اعلان |
| تعداد فایل هم‌زمان در صف | ✅ | `max_at_once` |

### ۲.۳ تب Downloads (دانلودها / تنظیمات پیش‌فرض دانلود) — **P1/P2**

| گزینه | وضعیت | کار |
|---|---|---|
| تنظیمات پنجره‌ی دانلود فایل (دکمه‌ی ویرایش…) | ❌ | زیرپنجره‌ی همین تب (پایین) |
| نشان دادن پنجره‌ی «شروع دانلود» (File Info) | 🟡 | الان همیشه نشان داده می‌شود → `Settings.show_start_dialog` |
| ↳ «دانلود شروع نشود، فقط فایل‌ها به صف دانلود اضافه شود» | ❌ | `Settings.add_to_queue_only` (فقط وقتی تیک بالا فعال است) |
| نشان دادن پنجره‌ی اتمام دانلود | 🟡 | الان پنجره‌ی Complete هست → `Settings.show_complete_dialog` |
| (توجه: این تنظیمات به بخش زمان‌بندی مربوط نمی‌شود) | — | scheduler همیشه بی‌صدا |
| بلافاصله بعد از نمایش پنجره‌ی «شروع دانلود»، دانلود شروع شود | ❌ | `Settings.start_immediately` |
| نمایش پنل افزودن به صف بعد از انتخاب «Download Later» | ❌ | `Settings.show_queue_panel_on_later` (نیازمند «صف‌های چندگانه» — بخش ۳.۳) |
| نمایش پنل افزودن به صف هنگام افزودن دانلود گروهی (Grabber) | ❌ | `Settings.show_queue_panel_on_batch` |
| در هنگام ادامه‌ی دانلود، از تغییر تاریخ چشم‌پوشی کن | ❌ | `Settings.ignore_date_change_on_resume` (در `engine` هنگام resume `Last-Modified` را با ذخیره‌شده تطبیق نده) |
| اگر لینک تکراری اضافه شد: «با نمایش پنجره‌ای، گزینه را سؤال کن» + حالت‌های دیگر | ❌ | `Settings.duplicate_action: Ask / Overwrite / AutoRename / Skip`؛ تشخیص تکراری بر اساس URL و مسیر نهایی |
| User-Agent برای دانلودهای دستی (فیلد متنی؛ پیش‌فرض Mozilla/5.0 …) | ❌ | `Settings.user_agent`؛ در `net.rs` برای درخواست‌هایی که از افزونه UA نمی‌گیرند |
| تنظیمات مقابله با ویروس (ویرایش…) | ❌ | `Settings.antivirus { enabled, command, args }`؛ بعد از اتمام، دستور (مثلاً `MpCmdRun.exe -Scan -ScanType 3 -File "{path}"`) اجرا شود؛ P3 |

### ۲.۴ تب Save To (ذخیره در) — **P1/P2**

| گزینه | وضعیت | کار |
|---|---|---|
| گروه‌ها: New / Edit + کشویی گروه (عمومی، …) | 🟡 | الان ۵ دسته‌ی ثابت → گروه‌ها قابل‌تعریف توسط کاربر: `Settings.groups: Vec<Group{name, dir, extensions}>` (Category ثابت به لیست داده‌محور تبدیل شود؛ نیازمند مهاجرت `Category` enum → string) |
| فایل‌های دسته‌بندی‌نشده به گروه «عمومی» بروند | ✅ | `Category::General` |
| مسیر پیش‌فرض ذخیره‌ی فایل‌ها (گروه عمومی) + Browse | ✅ | `base_dir` |
| آخرین مسیر ذخیره در گروه عمومی را یادت بماند | ❌ | `Settings.remember_last_dir` |
| ثبت تاریخ فایل بر اساس تاریخ سرور (`Last-Modified`) | ❌ | `Settings.use_server_date`؛ پس از اتمام `SetFileTime` / `filetime` |
| مسیر ذخیره‌ی موقت فایل‌ها هنگام دانلود (Temp dir) + Browse + توضیح | 🟡 | الان `.part` کنار فایل نهایی → `Settings.temp_dir: Option<PathBuf>`؛ `Download::temp_path` از آن استفاده کند؛ در پایان move/rename بین دیسک‌ها (copy+delete) |
| فیلد پوشه‌ی هر دسته (عرض بیشتر) | ✅ | همین سشن |

### ۲.۵ تب File Types (نوع فایل) — **P1**

| گزینه | وضعیت | کار |
|---|---|---|
| دانلود فایل‌هایی با پسوندهای زیر را خودکار شروع کن (textbox) + «تنظیمات اولیه» | 🟡 | پسوندها الان ثابت در `model.rs` → `Settings.auto_download_extensions: String` (قابل‌ویرایش، دکمه‌ی Reset to defaults)؛ به افزونه هم sync شود (`GET /config`) |
| از سایت‌های زیر خودکار دانلود نشود (با فاصله جدا شوند) + «تنظیمات اولیه» | ❌ | `Settings.auto_download_excluded_sites` با wildcard (پیش‌فرض: `*.update.microsoft.com download.windowsupdate.com siteseal.thawte.com ecom.cimetz.com *.voice2page.com karboom.io`) |
| از این آدرس‌ها خودکار دانلودی نگیر — «ویرایش لیست…» | ❌ | لیست مسدود (Blocklist) جدا با دیالوگ edit |
| — | — | اندپوینت `GET /config` در `server.rs` تا افزونه همین لیست‌ها را بخواند و `background.js` بر اساس آن رهگیری کند |

### ۲.۶ تب Password Sites (سایت‌های پسوورددار) — **P2**

| گزینه | کار |
|---|---|
| جدول: سایت/مسیر · نام کاربری · پسوورد (مخفی) + New / Edit / Delete | `Settings.credentials: Vec<{url_pattern, username, password}>`؛ **ذخیره‌ی امن:** رمز با Windows DPAPI (`CryptProtectData`) رمزنگاری شود، نه متن ساده در `data.json` |
| استفاده | `net.rs`: اگر سرور `401` + `WWW-Authenticate: Basic/Digest` داد و الگو مطابق بود، هدر `Authorization` اضافه شود؛ برای FTP هم |

### ۲.۷ تب Proxy / Socks (پروکسی) — **P2**

| گزینه | کار |
|---|---|
| «دریافت از ویندوز» (دکمه) | خواندن `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings` |
| اگر در دانلود، مرورگر خطای proxy/socks داد، از این استفاده کن | `Settings.proxy.fallback_on_error` |
| ○ بدون پروکسی | `ProxyMode::None` |
| ○ استفاده از تنظیمات ویندوز | `ProxyMode::System` |
| ○ اسکریپت تنظیمات خودکار (PAC) + آدرس | `ProxyMode::Pac(url)` (نیاز به مفسر PAC؛ **P3** — یا فقط دانلود و تطبیق ساده) |
| ○ تنظیمات دستی: Proxy، Port، نام کاربری، پسوورد + پروتکل‌ها (http / https / ftp) + «Socks… / بیشتر…» | `Settings.proxy.manual { host, port, user, pass, protocols, socks }`؛ `ureq` پروکسی `http` و `socks5` را پشتیبانی می‌کند (feature `socks-proxy`) |
| استفاده از FTP در حالت PASV | `Settings.ftp_passive` (در صورت پیاده‌سازی FTP — الان فقط HTTP(S) است؛ **P3**) |

### ۲.۸ تب VPN / Dial-up (مودم) — **حذف شد (تأیید کاربر): پیاده‌سازی نمی‌شود**
ارتباط VPN با شماره‌گیری برای مودم‌های قدیمی است. (توضیح قدیمی برای مرجع:) اگر لازم بود: انتخاب اتصال RAS، نام کاربری/رمز (DPAPI)، «رمز را یادت بماند»، «اجرا»، شمارنده‌ی redial (۰ = نامحدود) و فاصله‌ی ۳۰ ثانیه (`rasdial.exe`).

### ۲.۹ تب Sounds (صداها) — **P2**

| گزینه | کار |
|---|---|
| جدول رویداد ↔ فایل صوتی با تیک | رویدادها: «Download complete»، «Download failed (connection error)»، «Scheduler started»، «Scheduler stopped / all downloads done» |
| Browse… / Play | انتخاب `.wav`؛ پخش با `PlaySoundW` (winmm) یا crate `rodio`؛ `Settings.sounds: Vec<{event, enabled, path}>` |

---

## ۳. پنجره‌ی پیشرفت دانلود (Download progress) — **P1**

الان: پنجره‌ی جزئیات + جدول اتصال‌ها. IDM سه تب دارد:

### ۳.۱ تب «Download status» (وضعیت دانلود)
| فیلد | وضعیت |
|---|---|
| عنوان پنجره: `درصد% نام‌فایل` | 🟡 بررسی/اصلاح |
| URL کامل | ✅ |
| Status (در حال دریافت… / متصل شدن / خطا) | ✅ |
| File size · Downloaded (حجم و درصد) · Transfer rate · Time left | ✅ |
| Resume capability (Yes/No — «قابلیت ایست: دارد») | ❌ | نمایش `resumable` |
| دکمه‌ها: Hide/Show details · Pause · Cancel | 🟡 | اطمینان از وجود هر سه |
| نوار پیشرفت کلی + **نوار «نقاط شروع و دانلود به ترتیب اتصال»** (نمودار قطعه‌ها با رنگ هر اتصال) | 🟡 | رسم segmentهای `parts` به صورت نوار رنگی تک‌خطی |
| جدول: N. · Downloaded · Info (در حال دریافت… / ارسال GET / ارتباط قطع شد) | ✅ |

### ۳.۲ تب «Speed limiter» (محدودکننده‌ی سرعت)
| فیلد | کار |
|---|---|
| سرعت دانلود فعلی (KB/sec) | نمایش زنده |
| ☐ استفاده از محدودکننده‌ی سرعت + «حداکثر سرعت (KBytes/sec)» | UI روی `speed_limit_kbps` موجود در `Download` (مدل آماده است) |
| ☐ تنظیمات را برای این فایل حتی بعد از توقف و ادامه یادت بماند | ذخیره در `Download` (الان هست و persist می‌شود؛ گزینه‌ی «برای این فایل ذخیره کن» اضافه شود) |
| دکمه‌ی «Hide this tab» | جمع‌کردن تب |
| **محدودکننده‌ی سراسری** (جدید، پیشنهاد خودم): سقف کل سرعت در Options > Connection | `Settings.global_speed_limit_kbps` |

### ۳.۳ تب «When done» (گزینه‌های اتمام دانلود)
| فیلد | کار |
|---|---|
| ذخیره در: مسیر | نمایش |
| ☐ نمایش پنجره‌ی اتمام دانلود | per-download override |
| ☐ بعد از اتمام دانلودها، اینترنت را قطع کن | قطع آداپتور/ `rasdial /disconnect`؛ **P3** |
| ☐ بعد از اتمام دانلودها، برنامه را ببند | `Settings.on_complete = ExitApp` |
| ☐ بعد از اتمام دانلودها، کامپیوتر را خاموش کن + کشویی (Shut down / Sleep / Hibernate / Restart / Log off) + «مجبور» (Force) | `Settings.on_complete_power: Option<{action, force}>`؛ `shutdown.exe /s /t 60` با شمارش معکوس و امکان لغو (اجباری برای ایمنی) |

> **صف‌های چندگانه:** IDM چند صف (Main queue، Synchronization queue…) دارد؛ ما فقط یک صف. در P2: `Queue { id, name, schedule, max_at_once }` و `Download.queue_id`. سایدبار «Queues» الان یک ردیف (Main queue) دارد → گسترش.

---

## ۴. منوی راست‌کلیک روی ردیف دانلود — **P1**

| آیتم IDM | وضعیت فعلی | کار |
|---|---|---|
| Open (باز کردن) | ✅ | (فقط کامل‌ها؛ غیرفعال‌ها خاکستری شوند نه حذف) |
| Open with… (باز کردن با…) | ❌ | `OpenWith` دیالوگ شل ویندوز (`rundll32 shell32,OpenAs_RunDLL`) |
| Open folder (باز کردن پوشه) | ✅ | |
| Move/Rename (تغییر مسیر/نام) — **Ctrl+M** | ❌ | دیالوگ: نام جدید + مسیر جدید؛ برای ناتمام: `.part` را جابه‌جا کن؛ برای کامل: فایل را move |
| Redownload (دانلود مجدد) | ❌ | حذف فایل/parts قبلی و شروع از صفر (با تأیید) |
| Resume (ادامه دانلود) / Stop (توقف) | ✅ | وضعیت غیرفعال/فعال درست باشد |
| Refresh download address (بازیابی آدرس دانلود فایل) | ❌ | وقتی لینک منقضی شده: دیالوگ برای URL جدید، یا «Capture from browser» که افزونه آدرس تازه بفرستد؛ ادامه‌ی همان فایل (اعتبارسنجی size/ETag) |
| Delete (حذف) | ✅ | با گزینه‌ی «حذف فایل از دیسک» |
| Move to queue (به صف منتقل شود) ▸ زیرمنو + «از صف انتظار حذف شود» | 🟡 | زیرمنوی لیست صف‌ها + Remove from queue |
| On double-click (با دابل‌کلیک) ▸ زیرمنو | ❌ | `Settings.double_click_action: OpenFile / OpenFolder / ShowProgress`؛ آیتم زیرمنو برای تعیین |
| Properties (خصوصیات) | ❌ | دیالوگ: نام، URL، Referer، مسیر، حجم، تاریخ، توضیح، تعداد اتصال، speed limit، وضعیت resume، تاریخ آخرین تلاش، Checksum (SHA-256 محاسبه‌ی اختیاری) |
| Copy URL / Copy path (پیشنهاد خودم) | ❌ | کپی در clipboard |

> ستون «Queue» در جدول اصلی (ستون «صف») و ستون Last Try Date هست؛ ستون «Description» و «Referer» اختیاری P3.

---

## ۵. منوها و نوار ابزار اصلی (مقایسه‌ی سریع) — **P2/P3**

| IDM | ما |
|---|---|
| Tasks: Add URL، Add batch download from clipboard، Run site grabber، Export/Import، Exit | 🟡 فقط Add / Grabber / Exit → اضافه: **Add batch from clipboard**، **Export/Import** لیست دانلودها (JSON) |
| File: … | بررسی منو و اطمینان از Export/Import |
| Downloads: Stop all، Delete all completed، Start/Stop queue، Scheduler، Options | ✅ |
| Registration | 🟡 نمایش فعلی؛ بدون تغییر |
| Tell a Friend | ✅ |
| Grabber | ✅ |
| Check for updates (منوی Help) | ❌ P3 |
| Language (فارسی/انگلیسی) | ❌ P3 — خارج از این پلن |

---

## ۶. تغییرات مدل داده و مهاجرت

- همه‌ی فیلدهای جدید `Settings` و `Download` با `#[serde(default)]` تا `data.json` قدیمی بدون مشکل لود شود (قاعده‌ی فعلی همین است).
- `Category` از enum ثابت به **گروه‌های قابل‌تعریف** می‌رود (۲.۴). مهاجرت: دسته‌های فعلی به ۵ گروه پیش‌فرض تبدیل شوند؛ `Download.category` رشته‌ی نام گروه شود.
- رمزها فقط رمزنگاری‌شده (DPAPI) در دیسک.
- `GET /config` در سرور محلی برای sync تنظیمات با افزونه؛ سرور فقط روی `127.0.0.1` و با توکن مشترک (جلوگیری از سوء‌استفاده‌ی وب‌سایت‌ها از `localhost:6800`).

## ۷. ترتیب پیشنهادی اجرا (فاز‌بندی)

| فاز | محتوا | چرا |
|---|---|---|
| **P0** | بخش ۱.۵ — cursor نشان IDM · جلو آوردن برنامه از سینی · دیالوگ‌ها به‌صورت پنجره‌ی مستقل (با spike اول) | باگ‌های گزارش‌شده؛ پایه‌ی بقیه‌ی فازها |
| **B** | بخش ۴ (منوی راست‌کلیک) + ۳.۱ تا ۳.۳ (پنجره‌ی پیشرفت) | بیشترین دیده‌شدن در استفاده‌ی روزمره |
| **C** | Options: File Types، Downloads، Save To (۲.۳–۲.۵) | تنظیمات اصلی رفتار دانلود |
| **D** | Options: General (مرورگرها/کلیدها)، Connection (استثنا/سهمیه)، Proxy، Sounds | |
| **E** | Password Sites، صف‌های چندگانه، خاموش‌شدن بعد از اتمام | |
| **F** | آنتی‌ویروس، PAC، FTP (VPN/Dial-up حذف شد؛ HLS/DASH در فاز G) | سنگین/اختیاری |
| **G (آخر)** | بخش ۱ — دکمه‌ی دانلود روی ویدیو: mp4/webm مستقیم + Referer/Cookie، سپس **HLS و DASH** + `/config` (به‌درخواست کاربر آخرین فاز) | بزرگ‌ترین و پرریسک‌ترین کار |

هر فاز: تست unit (منطق مدل/تنظیمات) + تست دستی، `cargo build` بدون هشدار، و به‌روزرسانی `Docs/ARCHITECTURE_WindowsForm.md` و `Docs/BUSINESS.md` در همان تسک؛ هر فاز یک commit جدا.

## ۸. سؤال‌های باز (قبل از شروع تصمیم بگیر)

**تصمیم‌شده:** HLS/DASH مستقیم پیاده شود ✔ · VPN/Dial-up حذف ✔

**هنوز باز:**
~~۱. ffmpeg~~ → **تصمیم‌شده:** ffmpeg همراه نمی‌شود؛ الحاق/mux داخل برنامه (بخش ۱.۱).
~~۲. رفتار کلیک روی ویدیو~~ → **تصمیم‌شده (کاربر):** اول دیالوگ File Info (پنجره‌ی مستقل، با انتخاب کیفیت)، سپس Start Download.
3. گروه‌های قابل‌تعریف (۲.۴) تغییر بزرگی در مدل است؛ الان انجام شود یا بعد از بقیه؟ (پیشنهاد: بعد از فاز P0، A، B.)
4. PAC و FTP: حذف شوند؟ (پیشنهاد: بله.)
5. فارسی‌سازی UI و RTL در این پلن نیست؛ اضافه شود؟
6. spike پنجره‌ی مستقل (۱.۵.۳): اگر با viewport روی پنجره‌ی مخفی ممکن نشد، قبول داری پنجره‌ی اصلی به‌جای «مخفی» به‌صورت «minimize و بدون دکمه‌ی taskbar» برود؟
