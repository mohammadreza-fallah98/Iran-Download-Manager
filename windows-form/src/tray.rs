use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[cfg(windows)]
use std::sync::atomic::Ordering;

pub struct TrayHandle {
    #[cfg(windows)]
    pub exit_requested: Arc<AtomicBool>,
    #[cfg(windows)]
    pub show_requested: Arc<AtomicBool>,
}

/// Returns false if another instance is already running.
pub fn ensure_single_instance() -> bool {
    #[cfg(windows)]
    { return win::check_single_instance(); }
    #[cfg(not(windows))]
    true
}

/// Brings a top-level window of this app (by title) in front once; it does not stay on top.
pub fn raise_window(title: &str) {
    #[cfg(windows)]
    unsafe { win::raise_by_title(title); }
    #[cfg(not(windows))]
    let _ = title;
}

/// Sends the main window to the tray (kept alive off screen so dialogs can still be shown).
pub fn hide_main() {
    #[cfg(windows)]
    unsafe { win::hide_main(); }
}

/// Wakes the UI loop (it sleeps while the window is hidden) without showing the main window.
pub fn wake() {
    #[cfg(windows)]
    win::wake_ui();
}

#[cfg(not(windows))]
pub fn start(_ctx: egui::Context) -> TrayHandle {
    TrayHandle {}
}

// ── Windows implementation ───────────────────────────────────────────────────

#[cfg(windows)]
mod win {
    use super::*;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU32, AtomicUsize};

    // --- Constants ---
    const WM_TRAY_CALLBACK: u32  = 0x0401;
    const WM_LBUTTONUP: u32      = 0x0202;
    const WM_LBUTTONDBLCLK: u32  = 0x0203;
    const WM_RBUTTONUP: u32      = 0x0205;
    const WM_COMMAND: u32        = 0x0111;
    const NIF_MESSAGE: u32       = 0x0001;
    const NIF_ICON: u32          = 0x0002;
    const NIF_TIP: u32           = 0x0004;
    const NIM_ADD: u32           = 0x0000_0000;
    const NIM_DELETE: u32        = 0x0000_0002;
    const IDM_SHOW: usize        = 100;
    const IDM_EXIT: usize        = 101;
    const MF_STRING: u32         = 0x0000_0000;
    const MF_SEPARATOR: u32      = 0x0000_0800;
    const TPM_RIGHTBUTTON: u32   = 0x0002;
    const WS_EX_TOOLWINDOW: u32  = 0x0000_0080;
    const WS_POPUP: u32          = 0x8000_0000;
    const IDI_APPLICATION: *const u16 = 32512usize as *const u16;
    const ERROR_ALREADY_EXISTS: u32 = 183;
    const DIB_RGB_COLORS: u32    = 0;
    const SW_SHOW: i32           = 5;
    const SW_RESTORE: i32        = 9;
    const HWND_TOPMOST: isize    = -1;
    const HWND_NOTOPMOST: isize  = -2;
    const SW_HIDE: i32           = 0;
    const SW_SHOWNA: i32         = 8;
    const GWL_EXSTYLE: i32       = -20;
    const WS_EX_APPWINDOW: u32   = 0x0004_0000;
    const SWP_NOZORDER: u32      = 0x0004;
    const SWP_NOACTIVATE: u32    = 0x0010;
    const OFFSCREEN: i32         = -32000;
    static MAIN_HIDDEN: AtomicBool = AtomicBool::new(false);
    static SAVED_X: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
    static SAVED_Y: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

    #[repr(C)]
    struct RECT { left: i32, top: i32, right: i32, bottom: i32 }
    const SWP_NOSIZE: u32        = 0x0001;
    const SWP_NOMOVE: u32        = 0x0002;

    // --- Structs ---
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct POINT { x: i32, y: i32 }

    #[repr(C)]
    struct MSG {
        hwnd: usize, message: u32, w_param: usize, l_param: isize,
        time: u32, pt: POINT, _private: u32,
    }

    #[repr(C)]
    struct WNDCLASSEXW {
        cb_size: u32, style: u32,
        lpfn_wnd_proc: unsafe extern "system" fn(usize, u32, usize, isize) -> isize,
        cb_cls_extra: i32, cb_wnd_extra: i32,
        h_instance: usize, h_icon: usize, h_cursor: usize,
        hbr_background: usize, lpsz_menu_name: *const u16,
        lpsz_class_name: *const u16, h_icon_sm: usize,
    }

    // Full NOTIFYICONDATAW for 64-bit Windows (976 bytes)
    #[repr(C)]
    struct NOTIFYICONDATAW {
        cb_size: u32,  _pad0: u32,
        h_wnd: usize,
        u_id: u32, u_flags: u32, u_callback_message: u32,
        _pad1: u32,
        h_icon: usize,
        sz_tip: [u16; 128],
        _rest: [u8; 680],
    }

    impl NOTIFYICONDATAW {
        fn zeroed() -> Self { unsafe { std::mem::zeroed() } }
    }

    #[repr(C)]
    struct BITMAPINFOHEADER {
        bi_size: u32, bi_width: i32, bi_height: i32,
        bi_planes: u16, bi_bit_count: u16,
        bi_compression: u32, bi_size_image: u32,
        bi_x_pels_per_meter: i32, bi_y_pels_per_meter: i32,
        bi_clr_used: u32, bi_clr_important: u32,
    }

    #[repr(C)]
    struct BITMAPINFO {
        bmi_header: BITMAPINFOHEADER,
        bmi_colors: [u32; 1],
    }

    #[repr(C)]
    struct ICONINFO {
        f_icon: i32,
        x_hotspot: u32, y_hotspot: u32,
        hbm_mask: usize,
        hbm_color: usize,
    }

    // --- FFI ---
    #[link(name = "user32")]
    extern "system" {
        fn GetModuleHandleW(lp: *const u16) -> usize;
        fn RegisterClassExW(p: *const WNDCLASSEXW) -> u16;
        fn CreateWindowExW(ex: u32, cls: *const u16, name: *const u16, style: u32,
            x: i32, y: i32, w: i32, h: i32, parent: usize, menu: usize,
            inst: usize, param: *mut std::ffi::c_void) -> usize;
        fn DefWindowProcW(hwnd: usize, msg: u32, wp: usize, lp: isize) -> isize;
        fn GetMessageW(msg: *mut MSG, hwnd: usize, min: u32, max: u32) -> i32;
        fn TranslateMessage(msg: *const MSG) -> i32;
        fn DispatchMessageW(msg: *const MSG) -> isize;
        fn LoadIconW(inst: usize, name: *const u16) -> usize;
        fn CreatePopupMenu() -> usize;
        fn AppendMenuW(menu: usize, flags: u32, id: usize, text: *const u16) -> i32;
        fn SetForegroundWindow(hwnd: usize) -> i32;
        fn FindWindowW(cls: *const u16, name: *const u16) -> usize;
        fn ShowWindow(hwnd: usize, cmd: i32) -> i32;
        fn GetWindowRect(hwnd: usize, rect: *mut RECT) -> i32;
        fn GetWindowLongPtrW(hwnd: usize, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: usize, index: i32, value: isize) -> isize;
        fn GetForegroundWindow() -> usize;
        fn GetWindowThreadProcessId(hwnd: usize, pid: *mut u32) -> u32;
        fn AttachThreadInput(id_attach: u32, id_attach_to: u32, attach: i32) -> i32;
        fn SetWindowPos(hwnd: usize, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        fn BringWindowToTop(hwnd: usize) -> i32;
        fn RegisterWindowMessageW(s: *const u16) -> u32;
        fn TrackPopupMenu(menu: usize, flags: u32, x: i32, y: i32, reserved: i32,
            hwnd: usize, rect: *const std::ffi::c_void) -> i32;
        fn DestroyMenu(menu: usize) -> i32;
        fn GetCursorPos(pt: *mut POINT) -> i32;
        fn GetDC(hwnd: usize) -> usize;
        fn ReleaseDC(hwnd: usize, hdc: usize) -> i32;
        fn CreateCompatibleDC(hdc: usize) -> usize;
        fn DeleteDC(hdc: usize) -> i32;
        fn CreateBitmap(nwidth: i32, nheight: i32, nplanes: u32, nbit_count: u32, lp_bits: *const u8) -> usize;
        fn CreateIconIndirect(piconinfo: *mut ICONINFO) -> usize;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateDIBSection(hdc: usize, pbmi: *const BITMAPINFO, usage: u32,
            ppvbits: *mut *mut u8, hsection: usize, offset: u32) -> usize;
        fn DeleteObject(ho: usize) -> i32;
    }

    #[link(name = "shell32")]
    extern "system" {
        fn Shell_NotifyIconW(msg: u32, data: *mut NOTIFYICONDATAW) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateMutexW(attrs: *const std::ffi::c_void, initial_owner: i32, name: *const u16) -> usize;
        fn GetLastError() -> u32;
        fn GetCurrentThreadId() -> u32;
    }

    // --- Global shared state ---
    struct TrayGlobal {
        exit_requested: Arc<AtomicBool>,
        show_requested: Arc<AtomicBool>,
        ctx: egui::Context,
    }

    static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
    static TRAY_ICON: AtomicUsize = AtomicUsize::new(0);

    // Hidden eframe windows get no update() calls, so restore the native window
    // directly from the tray thread instead of waiting for the flag to be polled.
    pub(super) fn wake_ui() {
        if let Some(g) = TRAY_GLOBAL.get() {
            g.ctx.request_repaint();
        }
    }

    pub(super) unsafe fn request_show() {
        if let Some(g) = TRAY_GLOBAL.get() {
            g.show_requested.store(true, Ordering::Relaxed);
            let title = w("Iran Download Manager");
            let main = FindWindowW(std::ptr::null(), title.as_ptr());
            if main != 0 {
                unhide_main(main);
                ShowWindow(main, SW_SHOW);
                ShowWindow(main, SW_RESTORE);
                raise_hwnd(main);
            }
            g.ctx.request_repaint();
        }
    }

    /// Windows blocks foreground steals from background processes; a topmost
    /// flip + attaching to the foreground thread's input queue works around it.
    unsafe fn raise_hwnd(hwnd: usize) {
        let fg = GetForegroundWindow();
        let fg_thread = if fg != 0 { GetWindowThreadProcessId(fg, std::ptr::null_mut()) } else { 0 };
        let me = GetCurrentThreadId();
        if fg_thread != 0 && fg_thread != me { AttachThreadInput(me, fg_thread, 1); }
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        SetForegroundWindow(hwnd);
        BringWindowToTop(hwnd);
        if fg_thread != 0 && fg_thread != me { AttachThreadInput(me, fg_thread, 0); }
    }

    /// "Hides" the main window in the tray by moving it far off screen and taking it out of the
    /// taskbar. A truly hidden window would stop the UI loop, and then no dialog could be shown.
    pub(super) unsafe fn hide_main() {
        let title = w("Iran Download Manager");
        let main = FindWindowW(std::ptr::null(), title.as_ptr());
        if main == 0 || MAIN_HIDDEN.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(main, &mut r);
        SAVED_X.store(r.left, Ordering::SeqCst);
        SAVED_Y.store(r.top, Ordering::SeqCst);
        ShowWindow(main, SW_HIDE);
        let ex = GetWindowLongPtrW(main, GWL_EXSTYLE);
        SetWindowLongPtrW(main, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize));
        SetWindowPos(main, 0, OFFSCREEN, OFFSCREEN, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        ShowWindow(main, SW_SHOWNA);
    }

    /// Undoes `hide_main`: back on screen and back in the taskbar.
    unsafe fn unhide_main(main: usize) {
        if !MAIN_HIDDEN.swap(false, Ordering::SeqCst) {
            return;
        }
        let ex = GetWindowLongPtrW(main, GWL_EXSTYLE);
        ShowWindow(main, SW_HIDE);
        SetWindowLongPtrW(main, GWL_EXSTYLE, (ex & !(WS_EX_TOOLWINDOW as isize)) | WS_EX_APPWINDOW as isize);
        SetWindowPos(main, 0, SAVED_X.load(Ordering::SeqCst), SAVED_Y.load(Ordering::SeqCst), 0, 0, SWP_NOSIZE | SWP_NOZORDER);
    }

    /// Raises the window with this exact title (once, without keeping it on top).
    pub(super) unsafe fn raise_by_title(title: &str) {
        let t = w(title);
        let hwnd = FindWindowW(std::ptr::null(), t.as_ptr());
        if hwnd != 0 {
            ShowWindow(hwnd, SW_SHOW);
            raise_hwnd(hwnd);
        }
    }

    unsafe fn add_icon(hwnd: usize) -> bool {
        let tip_str = "Iran Download Manager";
        let mut tip = [0u16; 128];
        for (i, c) in tip_str.encode_utf16().enumerate().take(127) {
            tip[i] = c;
        }
        let mut nid = NOTIFYICONDATAW::zeroed();
        nid.cb_size            = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.h_wnd              = hwnd;
        nid.u_id               = 1;
        nid.u_flags            = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.u_callback_message = WM_TRAY_CALLBACK;
        nid.h_icon             = TRAY_ICON.load(Ordering::Relaxed);
        nid.sz_tip             = tip;
        Shell_NotifyIconW(NIM_ADD, &mut nid) != 0
    }

    static TRAY_GLOBAL: OnceLock<TrayGlobal> = OnceLock::new();

    // --- Window procedure ---
    unsafe extern "system" fn wnd_proc(hwnd: usize, msg: u32, wp: usize, lp: isize) -> isize {
        if msg == WM_TRAY_CALLBACK {
            let event = (lp & 0xFFFF) as u32;
            if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
                request_show();
            } else if event == WM_RBUTTONUP {
                show_context_menu(hwnd);
            }
            return 0;
        }
        if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) {
            add_icon(hwnd);
            return 0;
        }
        if msg == WM_COMMAND {
            let id = wp & 0xFFFF;
            match id {
                IDM_SHOW => request_show(),
                IDM_EXIT => {
                    if let Some(g) = TRAY_GLOBAL.get() {
                        g.exit_requested.store(true, Ordering::Relaxed);
                    }
                }
                _ => {}
            }
            return 0;
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe fn show_context_menu(hwnd: usize) {
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING, IDM_SHOW, w("Show Iran Download Manager").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, IDM_EXIT, w("Exit").as_ptr());
        let mut pt = POINT::default();
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, std::ptr::null());
        DestroyMenu(menu);
    }

    // Build a 32x32 HICON from the embedded icon.png
    unsafe fn create_hicon() -> usize {
        let bytes = include_bytes!("../assets/icon.png");
        if let Ok(img) = image::load_from_memory(bytes) {
            let img = img.resize_exact(32, 32, image::imageops::FilterType::Lanczos3).to_rgba8();
            let rgba = img.into_raw();

            let bmi = BITMAPINFO {
                bmi_header: BITMAPINFOHEADER {
                    bi_size: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    bi_width: 32,
                    bi_height: -32, // top-down
                    bi_planes: 1,
                    bi_bit_count: 32,
                    bi_compression: 0,
                    bi_size_image: 0,
                    bi_x_pels_per_meter: 0, bi_y_pels_per_meter: 0,
                    bi_clr_used: 0, bi_clr_important: 0,
                },
                bmi_colors: [0],
            };

            let hdc_screen = GetDC(0);
            let hdc = CreateCompatibleDC(hdc_screen);
            ReleaseDC(0, hdc_screen);

            let mut bits: *mut u8 = std::ptr::null_mut();
            let hbm_color = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, 0, 0);
            DeleteDC(hdc);

            if hbm_color != 0 && !bits.is_null() {
                let dst = std::slice::from_raw_parts_mut(bits, 32 * 32 * 4);
                for i in 0..(32 * 32usize) {
                    let r = rgba[i * 4] as u32;
                    let g = rgba[i * 4 + 1] as u32;
                    let b = rgba[i * 4 + 2] as u32;
                    let a = rgba[i * 4 + 3] as u32;
                    // Pre-multiplied alpha, BGRA order
                    dst[i * 4]     = ((b * a) / 255) as u8;
                    dst[i * 4 + 1] = ((g * a) / 255) as u8;
                    dst[i * 4 + 2] = ((r * a) / 255) as u8;
                    dst[i * 4 + 3] = a as u8;
                }
            }

            // 1bpp mask — all zeros lets alpha channel do transparency
            let mask_bytes = vec![0u8; (32 * 32) / 8];
            let hbm_mask = CreateBitmap(32, 32, 1, 1, mask_bytes.as_ptr());

            let mut iconinfo = ICONINFO {
                f_icon: 1,
                x_hotspot: 0, y_hotspot: 0,
                hbm_mask,
                hbm_color,
            };
            let hicon = CreateIconIndirect(&mut iconinfo);
            DeleteObject(hbm_color);
            DeleteObject(hbm_mask);

            if hicon != 0 { return hicon; }
        }
        LoadIconW(0, IDI_APPLICATION)
    }

    fn tray_thread() {
        unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());
            let cls_name = w("IDM_TRAY_WND");
            let wc = WNDCLASSEXW {
                cb_size: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfn_wnd_proc: wnd_proc,
                cb_cls_extra: 0, cb_wnd_extra: 0,
                h_instance: hinstance,
                h_icon: 0, h_cursor: 0, hbr_background: 0,
                lpsz_menu_name: std::ptr::null(),
                lpsz_class_name: cls_name.as_ptr(),
                h_icon_sm: 0,
            };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW, cls_name.as_ptr(), std::ptr::null(),
                WS_POPUP, 0, 0, 1, 1,
                0, 0, hinstance, std::ptr::null_mut(),
            );

            TRAY_ICON.store(create_hicon(), Ordering::Relaxed);
            TASKBAR_CREATED.store(RegisterWindowMessageW(w("TaskbarCreated").as_ptr()), Ordering::Relaxed);
            // Explorer may not be ready at login: retry for a while
            for _ in 0..60 {
                if add_icon(hwnd) { break; }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let mut nid = NOTIFYICONDATAW::zeroed();
            nid.cb_size = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.h_wnd = hwnd;
            nid.u_id = 1;
            Shell_NotifyIconW(NIM_DELETE, &mut nid);
        }
    }

    pub fn check_single_instance() -> bool {
        unsafe {
            let name = w("Global\\IranDownloadManager_SingleInstance");
            let mutex = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if mutex == 0 { return true; } // Cannot determine, allow run
            GetLastError() != ERROR_ALREADY_EXISTS
        }
    }

    pub fn start_impl(ctx: egui::Context) -> TrayHandle {
        let exit_requested = Arc::new(AtomicBool::new(false));
        let show_requested = Arc::new(AtomicBool::new(false));
        let _ = TRAY_GLOBAL.set(TrayGlobal {
            exit_requested: exit_requested.clone(),
            show_requested: show_requested.clone(),
            ctx,
        });
        std::thread::spawn(tray_thread);

        // When the window is hidden, update() doesn't run, so watch exit_requested here
        let exit_mon = exit_requested.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(200));
                if exit_mon.load(Ordering::Relaxed) {
                    std::process::exit(0);
                }
            }
        });

        TrayHandle { exit_requested, show_requested }
    }
}

#[cfg(windows)]
pub fn start(ctx: egui::Context) -> TrayHandle {
    win::start_impl(ctx)
}
