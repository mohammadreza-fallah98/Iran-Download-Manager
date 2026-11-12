//! Interface language. egui has no Arabic shaping or bidirectional layout, so Persian text is
//! shaped here (joined letter forms) and reordered into visual order before it is drawn.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Language {
    #[default]
    English,
    Persian,
}

impl Language {
    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Persian => "فارسی",
        }
    }
}

static PERSIAN: AtomicBool = AtomicBool::new(false);

pub fn set_language(l: Language) {
    PERSIAN.store(l == Language::Persian, Ordering::Relaxed);
}

pub fn is_persian() -> bool {
    PERSIAN.load(Ordering::Relaxed)
}

/// Translates a UI text; unknown texts stay as they are.
pub fn tr(text: &str) -> String {
    if !is_persian() {
        return text.to_string();
    }
    match dictionary().get(text) {
        Some(fa) => to_visual(fa),
        None => text.to_string(),
    }
}

/// Registers a font that has the Arabic letter forms as a fallback for every text style.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("vazirmatn".into(), std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/Vazirmatn-Regular.ttf"))));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("vazirmatn".into());
    }
    ctx.set_fonts(fonts);
}

// ── Letter shaping ───────────────────────────────────────────────────────────

/// (letter, isolated, final, initial, medial); 0 = the form does not exist.
const FORMS: &[(char, u32, u32, u32, u32)] = &[
    ('\u{0621}', 0xFE80, 0, 0, 0),
    ('\u{0622}', 0xFE81, 0xFE82, 0, 0),
    ('\u{0623}', 0xFE83, 0xFE84, 0, 0),
    ('\u{0624}', 0xFE85, 0xFE86, 0, 0),
    ('\u{0625}', 0xFE87, 0xFE88, 0, 0),
    ('\u{0626}', 0xFE89, 0xFE8A, 0xFE8B, 0xFE8C),
    ('\u{0627}', 0xFE8D, 0xFE8E, 0, 0),
    ('\u{0628}', 0xFE8F, 0xFE90, 0xFE91, 0xFE92),
    ('\u{0629}', 0xFE93, 0xFE94, 0, 0),
    ('\u{062A}', 0xFE95, 0xFE96, 0xFE97, 0xFE98),
    ('\u{062B}', 0xFE99, 0xFE9A, 0xFE9B, 0xFE9C),
    ('\u{062C}', 0xFE9D, 0xFE9E, 0xFE9F, 0xFEA0),
    ('\u{062D}', 0xFEA1, 0xFEA2, 0xFEA3, 0xFEA4),
    ('\u{062E}', 0xFEA5, 0xFEA6, 0xFEA7, 0xFEA8),
    ('\u{062F}', 0xFEA9, 0xFEAA, 0, 0),
    ('\u{0630}', 0xFEAB, 0xFEAC, 0, 0),
    ('\u{0631}', 0xFEAD, 0xFEAE, 0, 0),
    ('\u{0632}', 0xFEAF, 0xFEB0, 0, 0),
    ('\u{0633}', 0xFEB1, 0xFEB2, 0xFEB3, 0xFEB4),
    ('\u{0634}', 0xFEB5, 0xFEB6, 0xFEB7, 0xFEB8),
    ('\u{0635}', 0xFEB9, 0xFEBA, 0xFEBB, 0xFEBC),
    ('\u{0636}', 0xFEBD, 0xFEBE, 0xFEBF, 0xFEC0),
    ('\u{0637}', 0xFEC1, 0xFEC2, 0xFEC3, 0xFEC4),
    ('\u{0638}', 0xFEC5, 0xFEC6, 0xFEC7, 0xFEC8),
    ('\u{0639}', 0xFEC9, 0xFECA, 0xFECB, 0xFECC),
    ('\u{063A}', 0xFECD, 0xFECE, 0xFECF, 0xFED0),
    ('\u{0641}', 0xFED1, 0xFED2, 0xFED3, 0xFED4),
    ('\u{0642}', 0xFED5, 0xFED6, 0xFED7, 0xFED8),
    ('\u{0643}', 0xFED9, 0xFEDA, 0xFEDB, 0xFEDC),
    ('\u{0644}', 0xFEDD, 0xFEDE, 0xFEDF, 0xFEE0),
    ('\u{0645}', 0xFEE1, 0xFEE2, 0xFEE3, 0xFEE4),
    ('\u{0646}', 0xFEE5, 0xFEE6, 0xFEE7, 0xFEE8),
    ('\u{0647}', 0xFEE9, 0xFEEA, 0xFEEB, 0xFEEC),
    ('\u{0648}', 0xFEED, 0xFEEE, 0, 0),
    ('\u{064A}', 0xFEF1, 0xFEF2, 0xFEF3, 0xFEF4),
    ('\u{067E}', 0xFB56, 0xFB57, 0xFB58, 0xFB59),
    ('\u{0686}', 0xFB7A, 0xFB7B, 0xFB7C, 0xFB7D),
    ('\u{0698}', 0xFB8A, 0xFB8B, 0, 0),
    ('\u{06A9}', 0xFB8E, 0xFB8F, 0xFB90, 0xFB91),
    ('\u{06AF}', 0xFB92, 0xFB93, 0xFB94, 0xFB95),
    ('\u{06CC}', 0xFBFC, 0xFBFD, 0xFBFE, 0xFBFF),
];

const ZWNJ: char = '\u{200C}';

fn form_of(c: char) -> Option<&'static (char, u32, u32, u32, u32)> {
    FORMS.iter().find(|f| f.0 == c)
}

fn pick(code: u32) -> char {
    char::from_u32(code).unwrap_or('?')
}

/// Replaces letters with their joined forms (still in logical order).
fn shape(text: &str) -> Vec<char> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;
    // can the previous letter connect to the one after it?
    let mut prev_joins = false;
    while i < chars.len() {
        let c = chars[i];
        if c == ZWNJ {
            prev_joins = false;
            i += 1;
            continue;
        }
        let Some(f) = form_of(c) else {
            prev_joins = false;
            out.push(c);
            i += 1;
            continue;
        };
        // lam + alef make one ligature
        if c == '\u{0644}' && chars.get(i + 1) == Some(&'\u{0627}') {
            out.push(pick(if prev_joins { 0xFEFC } else { 0xFEFB }));
            prev_joins = false;
            i += 2;
            continue;
        }
        let next_is_letter = chars.get(i + 1).and_then(|n| form_of(*n)).is_some_and(|nf| nf.2 != 0);
        let join_prev = prev_joins && f.2 != 0;
        let join_next = f.3 != 0 && next_is_letter;
        let code = match (join_prev, join_next) {
            (true, true) => f.4,
            (true, false) => f.2,
            (false, true) => f.3,
            (false, false) => f.1,
        };
        out.push(pick(code));
        prev_joins = join_next;
        i += 1;
    }
    out
}

// ── Visual ordering ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Dir {
    Rtl,
    Ltr,
    Neutral,
}

fn dir_of(c: char) -> Dir {
    let n = c as u32;
    if (0x0600..=0x06FF).contains(&n) || (0xFB50..=0xFDFF).contains(&n) || (0xFE70..=0xFEFF).contains(&n) {
        Dir::Rtl
    } else if c.is_alphanumeric() {
        Dir::Ltr
    } else {
        Dir::Neutral
    }
}

fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '<' => '>',
        '>' => '<',
        other => other,
    }
}

/// Shapes the letters and puts the text in the order it is drawn left to right.
pub fn to_visual(logical: &str) -> String {
    let chars = shape(logical);
    let strong: Vec<Dir> = chars.iter().map(|c| dir_of(*c)).collect();
    // neutrals between two letters of the same direction follow them, otherwise the paragraph (right to left)
    let mut dirs = strong.clone();
    for i in 0..chars.len() {
        if strong[i] != Dir::Neutral {
            continue;
        }
        let before = strong[..i].iter().rev().find(|d| **d != Dir::Neutral).copied();
        let after = strong[i + 1..].iter().find(|d| **d != Dir::Neutral).copied();
        dirs[i] = match (before, after) {
            (Some(a), Some(b)) if a == b => a,
            _ => Dir::Rtl,
        };
    }
    let mut runs: Vec<(Dir, Vec<char>)> = Vec::new();
    for (c, d) in chars.iter().zip(dirs.iter()) {
        match runs.last_mut() {
            Some((rd, run)) if rd == d => run.push(*c),
            _ => runs.push((*d, vec![*c])),
        }
    }
    let mut out = String::new();
    for (d, run) in runs.into_iter().rev() {
        if d == Dir::Rtl {
            out.extend(run.into_iter().rev().map(mirror));
        } else {
            out.extend(run);
        }
    }
    out
}

// ── Translations ─────────────────────────────────────────────────────────────

fn dictionary() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| TEXTS.iter().copied().collect())
}

const TEXTS: &[(&str, &str)] = &[
    // buttons and common words
    ("    OK    ", "    تأیید    "),
    ("OK", "تأیید"),
    ("Cancel", "انصراف"),
    ("  Yes  ", "  بله  "),
    ("  No  ", "  خیر  "),
    ("Close", "بستن"),
    ("Delete", "حذف"),
    ("Remove", "حذف از لیست"),
    ("New", "جدید"),
    ("New...", "جدید..."),
    ("Rename", "تغییر نام"),
    ("Open", "باز کردن"),
    ("Open folder", "باز کردن پوشه"),
    ("Play", "پخش"),
    ("Resume", "ادامه"),
    ("Stop", "توقف"),
    ("Stop All", "توقف همه"),
    ("Stop all", "توقف همه"),
    ("Properties", "ویژگی‌ها"),
    ("Info", "اطلاعات"),
    ("Confirm", "تأیید"),
    ("Unknown", "نامشخص"),
    // main window
    ("Tasks", "وظایف"),
    ("File", "فایل"),
    ("Downloads", "دانلودها"),
    ("View", "نمایش"),
    ("Help", "راهنما"),
    ("Registration", "ثبت‌نام"),
    ("Add URL", "افزودن آدرس"),
    ("Options", "تنظیمات"),
    ("Options...", "تنظیمات..."),
    ("Scheduler", "زمان‌بندی"),
    ("Scheduler...", "زمان‌بندی..."),
    ("Start Queue", "شروع صف"),
    ("Stop Queue", "توقف صف"),
    ("Start queue", "شروع صف"),
    ("Stop queue", "توقف صف"),
    ("Grabber", "گرابر"),
    ("Tell a Friend", "معرفی به دوستان"),
    ("Delete all completed", "حذف همه‌ی کامل‌شده‌ها"),
    ("Add new download...", "افزودن دانلود جدید..."),
    ("Add batch download from clipboard", "افزودن گروهی از کلیپ‌بورد"),
    ("Export list...", "خروجی گرفتن از لیست..."),
    ("Import list...", "وارد کردن لیست..."),
    ("Run site grabber", "اجرای گرابر سایت"),
    ("Exit", "خروج"),
    ("Download now", "دانلود کن"),
    ("Download selected now", "دانلود انتخاب‌شده‌ها"),
    ("Stop download", "توقف دانلود"),
    ("All downloads", "همه‌ی دانلودها"),
    ("Categories", "دسته‌بندی‌ها"),
    ("Grabber projects", "پروژه‌های گرابر"),
    ("Queues", "صف‌ها"),
    ("Main download queue", "صف اصلی دانلود"),
    ("Queue running", "صف در حال اجرا"),
    ("Create a queue", "ساخت صف"),
    ("Queue name", "نام صف"),
    ("Delete this queue; its files go back to the main queue", "حذف این صف؛ فایل‌هایش به صف اصلی برمی‌گردند"),
    ("File Name", "نام فایل"),
    ("Size", "حجم"),
    ("Status", "وضعیت"),
    ("Time left", "زمان باقی‌مانده"),
    ("Transfer rate", "سرعت"),
    ("Last Try Date", "آخرین تلاش"),
    ("Queue", "صف"),
    ("Add to queue", "افزودن به صف"),
    ("file(s) were added to a queue. Choose the queue:", "فایل به صف اضافه شد. صف را انتخاب کنید:"),
    ("Keep in the main queue", "در صف اصلی بماند"),
    ("Ask which queue to use after \"Download Later\"", "بعد از «دانلود بعداً» انتخاب صف پرسیده شود"),
    ("Ask which queue to use when adding a group of files", "هنگام افزودن گروهی فایل‌ها انتخاب صف پرسیده شود"),
    ("Antivirus", "آنتی‌ویروس"),
    ("Scan finished files with a virus scanner", "بررسی فایل‌های تمام‌شده با آنتی‌ویروس"),
    ("Program to run ({path} = the finished file):", "برنامه‌ی اجرایی ({path} = فایل تمام‌شده):"),
    ("Use Windows Defender", "استفاده از Windows Defender"),
    ("Scan only files with these extensions (empty = every file):", "فقط فایل‌هایی با این پسوندها بررسی شوند (خالی = همه‌ی فایل‌ها):"),
    ("The scanner runs in the background after each download finishes; its result is not shown here.", "آنتی‌ویروس بعد از پایان هر دانلود در پس‌زمینه اجرا می‌شود و نتیجه‌اش اینجا نشان داده نمی‌شود."),
    ("Disconnect from the internet", "قطع اتصال اینترنت"),
    ("Check for updates...", "بررسی به‌روزرسانی..."),
    ("Checking for updates...", "در حال بررسی به‌روزرسانی..."),
    ("A newer version is available:", "نسخه‌ی جدیدتری موجود است:"),
    ("you have", "نسخه‌ی شما"),
    ("You have the latest version", "نسخه‌ی شما آخرین نسخه است"),
    ("Could not check for updates", "بررسی به‌روزرسانی ممکن نشد"),
    ("Checking the new address...", "در حال بررسی آدرس جدید..."),
    ("The download no longer exists.", "این دانلود دیگر وجود ندارد."),
    ("The new address could not be checked", "بررسی آدرس جدید ممکن نشد"),
    ("The new address points to a file of a different size", "آدرس جدید به فایلی با حجم متفاوت اشاره می‌کند"),
    ("The address was not changed.", "آدرس تغییر نکرد."),
    ("The new server does not support resume, so the parts already downloaded could not be kept. The address was not changed.", "سرور جدید ادامه‌ی دانلود را پشتیبانی نمی‌کند و بخش‌های دانلودشده قابل حفظ نیستند. آدرس تغییر نکرد."),
    ("The address was updated. The download can be resumed.", "آدرس به‌روز شد. می‌توانید دانلود را ادامه دهید."),
    ("Light", "روشن"),
    ("Dark", "تیره"),
    ("All Downloads", "همه‌ی دانلودها"),
    ("Unfinished", "ناتمام"),
    ("Finished", "تمام‌شده"),
    ("Description", "توضیحات"),
    ("Complete", "کامل"),
    ("Error", "خطا"),
    ("Stopped", "متوقف"),
    ("Queued", "در صف"),
    ("Connecting...", "در حال اتصال..."),
    ("Receiving data...", "در حال دریافت..."),
    ("Compressed", "فشرده"),
    ("Documents", "اسناد"),
    ("Music", "موسیقی"),
    ("Programs", "برنامه‌ها"),
    ("Video", "ویدیو"),
    ("General", "عمومی"),
    ("downloads", "دانلود"),
    ("active", "فعال"),
    ("Total size", "مجموع حجم"),
    // context menu
    ("Open with...", "باز کردن با..."),
    ("Move/Rename...    (Ctrl+M)", "جابه‌جایی/تغییر نام...    (Ctrl+M)"),
    ("Redownload", "دانلود دوباره"),
    ("Show progress", "نمایش پیشرفت"),
    ("Refresh download address...", "بازیابی آدرس دانلود..."),
    ("Add to queue", "افزودن به صف"),
    ("Remove from queue", "حذف از صف"),
    ("Move to queue", "انتقال به صف"),
    ("Copy URL", "کپی آدرس"),
    ("Move / Rename", "جابه‌جایی / تغییر نام"),
    ("Refresh download address", "بازیابی آدرس دانلود"),
    ("Paste the new address of this file. Already downloaded parts are kept.", "آدرس جدید این فایل را بچسبانید. بخش‌های دانلودشده حفظ می‌شوند."),
    ("Rename queue", "تغییر نام صف"),
    ("New queue", "صف جدید"),
    // dialogs
    ("Download File Info", "اطلاعات فایل دانلود"),
    ("Enter new address to download", "آدرس جدید برای دانلود را وارد کنید"),
    ("Address:", "آدرس:"),
    ("Address", "آدرس"),
    ("URL", "آدرس"),
    ("Category", "دسته"),
    ("Save To", "ذخیره در"),
    ("File name", "نام فایل"),
    ("Start Download", "شروع دانلود"),
    ("Download Later", "دانلود بعداً"),
    ("Getting file information...", "در حال دریافت اطلاعات فایل..."),
    ("Download complete", "دانلود کامل شد"),
    ("Downloads finished", "دانلودها تمام شد"),
    ("Download limit reached", "به سقف دانلود رسیدید"),
    ("Downloaded", "دانلود شده"),
    ("File size", "حجم فایل"),
    ("Resume capability", "قابلیت ادامه"),
    ("Download status", "وضعیت دانلود"),
    ("Speed Limiter", "محدودکننده‌ی سرعت"),
    ("When done", "پس از اتمام"),
    ("Use Speed Limiter", "استفاده از محدودکننده‌ی سرعت"),
    ("Maximum download speed", "حداکثر سرعت دانلود"),
    ("KBytes/sec", "کیلوبایت بر ثانیه"),
    ("The setting is kept for this file when it is stopped and resumed.", "این تنظیم برای این فایل بعد از توقف و ادامه حفظ می‌شود."),
    ("After all downloads are finished:", "بعد از اتمام همه‌ی دانلودها:"),
    ("Do nothing", "هیچ کاری نکن"),
    ("Exit the program", "بستن برنامه"),
    ("Turn off / restart the computer:", "خاموش / راه‌اندازی مجدد کامپیوتر:"),
    ("Shut down", "خاموش کردن"),
    ("Restart", "راه‌اندازی مجدد"),
    ("Sleep", "خواب"),
    ("Hibernate", "هایبرنیت"),
    ("Force close running applications", "بستن اجباری برنامه‌های در حال اجرا"),
    ("A 30 second countdown with a Cancel button is shown before the computer is turned off.", "قبل از خاموش شدن، شمارش معکوس ۳۰ ثانیه‌ای با دکمه‌ی انصراف نشان داده می‌شود."),
    ("Show details >>", "نمایش جزئیات >>"),
    ("<< Hide details", "<< پنهان کردن جزئیات"),
    ("Pause", "توقف موقت"),
    ("N.", "ردیف"),
    ("Select all", "انتخاب همه"),
    ("Select none", "لغو انتخاب"),
    ("Add selected to queue", "افزودن انتخاب‌شده‌ها به صف"),
    ("Find files", "یافتن فایل‌ها"),
    ("Exploring the page...", "در حال بررسی صفحه..."),
    ("Start page", "صفحه‌ی شروع"),
    ("File types", "نوع فایل‌ها"),
    ("Start now", "همین حالا شروع کن"),
    ("Type file extensions separated by space (e.g. avi mpg mpeg)", "پسوند فایل‌ها را با فاصله بنویسید (مثل avi mpg mpeg)"),
    ("Start download at", "شروع دانلود در ساعت"),
    ("Stop download at", "توقف دانلود در ساعت"),
    ("use HH:MM", "قالب HH:MM"),
    ("Files downloaded at the same time", "تعداد دانلود هم‌زمان"),
    ("Copy message", "کپی پیام"),
    ("About", "درباره"),
    ("Version 0.1", "نسخه ۰٫۱"),
    ("Iran Download Manager", "مدیر دانلود ایران"),
    ("This rebuild is free; nothing to register.", "این نسخه رایگان است؛ نیازی به ثبت‌نام نیست."),
    ("Splits each file into parts and downloads them at the same time, with pause and resume, categories, a queue, a scheduler and a speed limiter.", "هر فایل را به چند بخش تقسیم می‌کند و هم‌زمان دانلود می‌کند؛ با توقف و ادامه، دسته‌بندی، صف، زمان‌بندی و محدودکننده‌ی سرعت."),
    ("The file saved as", "فایل ذخیره شد با نام"),
    // options: tabs
    ("General", "عمومی"),
    ("Connection", "اتصال"),
    ("Save To", "ذخیره در"),
    ("File Types", "نوع فایل‌ها"),
    ("Proxy", "پروکسی"),
    ("Sounds", "صداها"),
    ("Password Sites", "سایت‌های رمزدار"),
    // options: general
    ("Automatically start downloading of URLs placed to clipboard", "شروع خودکار دانلود آدرس‌هایی که در کلیپ‌بورد قرار می‌گیرند"),
    ("Copied links to known file types open the Download File Info dialog.", "لینک‌های کپی‌شده‌ی فایل‌های شناخته‌شده، پنجره‌ی اطلاعات فایل را باز می‌کنند."),
    ("Run Iran Download Manager when Windows starts", "اجرای برنامه با روشن شدن ویندوز"),
    ("Browser integration", "اتصال به مرورگر"),
    ("Take over downloads of the file types listed in the File Types tab", "گرفتن دانلود فایل‌های لیست «نوع فایل‌ها» از مرورگر"),
    ("Show the \"Download this video\" button on videos", "نمایش دکمه‌ی «دانلود این ویدیو» روی ویدیوها"),
    ("Hold this key while clicking a link to let the browser download it:", "برای دانلود با خود مرورگر، این کلید را هنگام کلیک نگه دارید:"),
    ("Install it from the Chrome/Edge or Firefox package in the Release\\Extensions folder, then reload the browser tab.", "افزونه را از بسته‌ی Chrome/Edge یا Firefox در پوشه‌ی Release\\Extensions نصب کنید و سپس صفحه را دوباره بارگذاری کنید."),
    ("Language", "زبان"),
    // options: connection
    ("Default max. conn. number", "حداکثر اتصال پیش‌فرض"),
    ("Files downloaded at the same time in the queue", "تعداد فایل هم‌زمان در صف"),
    ("Use many connections with care: some servers limit or block clients that open too many.", "با احتیاط از اتصال زیاد استفاده کنید: بعضی سرورها کاربرانی که اتصال زیاد باز می‌کنند را محدود یا مسدود می‌کنند."),
    ("Speed limit for all downloads together (0 = no limit)", "سقف سرعت کل دانلودها (۰ = بدون محدودیت)"),
    ("Limit downloads", "محدود کردن دانلودها"),
    ("Stop after more than", "توقف بعد از بیش از"),
    ("MB in each", "مگابایت در هر"),
    ("hour(s)", "ساعت"),
    ("Exceptions (max. connections for specific servers)", "استثناها (حداکثر اتصال برای سرورهای خاص)"),
    ("server or *.domain.com", "سرور یا *.domain.com"),
    // options: downloads
    ("Show the \"Download File Info\" window when a download starts", "نمایش پنجره‌ی «اطلاعات فایل» هنگام شروع دانلود"),
    ("Do not start the download, only add the file to the queue", "دانلود شروع نشود، فقط فایل به صف اضافه شود"),
    ("Show the \"Download complete\" window", "نمایش پنجره‌ی «دانلود کامل شد»"),
    ("These settings do not apply to the scheduler.", "این تنظیمات روی زمان‌بندی اثر ندارند."),
    ("If the address is already in the list:", "اگر آدرس از قبل در لیست باشد:"),
    ("Show a warning in the info window", "هشدار در پنجره‌ی اطلاعات نشان بده"),
    ("Download again with a new name", "با نام جدید دوباره دانلود کن"),
    ("Do not add it, select the existing one", "اضافه نکن، موجود را انتخاب کن"),
    ("User-Agent for downloads (empty = default):", "User-Agent دانلودها (خالی = پیش‌فرض):"),
    ("Run a program on every finished file, e.g. a virus scanner ({path} = the file; empty = none):", "اجرای یک برنامه روی هر فایل تمام‌شده، مثلاً آنتی‌ویروس ({path} = فایل؛ خالی = هیچ):"),
    // options: save to
    ("Main download folder", "پوشه‌ی اصلی دانلود"),
    ("Folder", "پوشه"),
    ("Temporary folder for unfinished files", "پوشه‌ی موقت برای فایل‌های ناتمام"),
    ("empty = next to the final file", "خالی = کنار فایل نهایی"),
    ("Give the file the date reported by the server", "تاریخ فایل مطابق تاریخ سرور باشد"),
    ("Remember the last folder chosen for each category", "آخرین پوشه‌ی هر دسته به خاطر سپرده شود"),
    ("Browse folder", "انتخاب پوشه"),
    // options: file types
    ("Start downloading automatically files with these extensions (separated by spaces):", "دانلود خودکار فایل‌هایی با این پسوندها (با فاصله جدا کنید):"),
    ("Do not start downloading automatically from these sites (separated by spaces, * allowed):", "دانلود خودکار از این سایت‌ها انجام نشود (با فاصله جدا کنید، * مجاز است):"),
    ("Default settings", "تنظیمات پیش‌فرض"),
    ("Restore the default site list", "بازگرداندن لیست پیش‌فرض سایت‌ها"),
    // options: proxy
    ("Do not use a proxy", "بدون پروکسی"),
    ("Use the Windows proxy settings", "استفاده از تنظیمات پروکسی ویندوز"),
    ("Use an automatic configuration script (PAC)", "استفاده از اسکریپت پیکربندی خودکار (PAC)"),
    ("Manual proxy settings", "تنظیم دستی پروکسی"),
    ("Script address", "آدرس اسکریپت"),
    ("Type", "نوع"),
    ("Port", "پورت"),
    ("User name", "نام کاربری"),
    ("Password", "رمز عبور"),
    ("Get from Windows", "گرفتن از ویندوز"),
    ("The password is kept only while the program runs; it is not saved to disk.", "رمز فقط تا بسته شدن برنامه نگه داشته می‌شود و روی دیسک ذخیره نمی‌شود."),
    // options: sounds / passwords
    ("Play a sound when:", "پخش صدا هنگام:"),
    ("empty = system beep, or a .wav file", "خالی = صدای سیستم، یا یک فایل .wav"),
    ("Sites that need a user name and password to download:", "سایت‌هایی که برای دانلود نام کاربری و رمز می‌خواهند:"),
    ("Site / path", "سایت / مسیر"),
    ("Passwords are encrypted with your Windows account.", "رمزها با حساب ویندوز شما رمزنگاری می‌شوند."),
    ("Sounds", "صداها"),
    // options: save-to extras
    ("Folder", "پوشه"),
    ("A download is complete", "یک دانلود کامل شد"),
    ("A download failed", "یک دانلود ناموفق بود"),
    ("The scheduler started", "زمان‌بند شروع شد"),
    ("The scheduler stopped", "زمان‌بند متوقف شد"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_joins_letters_by_position() {
        // ب ا ب: initial form, final alef ... "باب"
        let s: String = shape("باب").into_iter().collect();
        assert_eq!(s, "\u{FE91}\u{FE8E}\u{FE8F}");
        // a lone letter keeps its isolated form
        assert_eq!(shape("ب").into_iter().collect::<String>(), "\u{FE8F}");
    }

    #[test]
    fn shape_uses_lam_alef_ligature_and_breaks_at_zwnj() {
        assert_eq!(shape("لا").into_iter().collect::<String>(), "\u{FEFB}");
        assert_eq!(shape("بلا").into_iter().collect::<String>(), "\u{FE91}\u{FEFC}");
        // with a ZWNJ the second letter starts a new word form
        assert_eq!(shape("ب\u{200C}ب").into_iter().collect::<String>(), "\u{FE8F}\u{FE8F}");
    }

    #[test]
    fn to_visual_reverses_persian_and_keeps_latin_and_digits_in_order() {
        // two Persian letters: the visual (left to right) order is reversed
        let v: Vec<char> = to_visual("با").chars().collect();
        assert_eq!(v, vec!['\u{FE8E}', '\u{FE91}']);
        // Latin words stay readable and move to the other side
        let v = to_visual("Proxy پروکسی");
        assert!(v.ends_with("Proxy"));
        assert!(v.starts_with(|c: char| (c as u32) >= 0xFB50));
        // brackets are mirrored in right-to-left text
        assert!(to_visual("(ب)").starts_with('('));
    }

    #[test]
    fn tr_returns_english_unless_persian_is_selected() {
        set_language(Language::English);
        assert_eq!(tr("Cancel"), "Cancel");
        set_language(Language::Persian);
        assert_ne!(tr("Cancel"), "Cancel");
        assert_eq!(tr("Some unknown text"), "Some unknown text");
        set_language(Language::English);
    }

    #[test]
    fn every_translation_glyph_exists_in_the_bundled_font() {
        let data = include_bytes!("../assets/Vazirmatn-Regular.ttf");
        let face = ttf_parser::Face::parse(data, 0).unwrap();
        let mut missing = std::collections::BTreeSet::new();
        for (_, fa) in TEXTS {
            for c in to_visual(fa).chars() {
                if !c.is_control() && face.glyph_index(c).is_none() {
                    missing.insert(c);
                }
            }
        }
        assert!(missing.is_empty(), "glyphs missing in the font: {missing:?}");
    }

    #[test]
    fn dictionary_has_no_empty_or_duplicated_conflicting_entries() {
        let mut seen: HashMap<&str, &str> = HashMap::new();
        for (en, fa) in TEXTS {
            assert!(!fa.trim().is_empty());
            if let Some(prev) = seen.insert(en, fa) {
                assert_eq!(prev, *fa, "conflicting translations for {en}");
            }
        }
    }
}
