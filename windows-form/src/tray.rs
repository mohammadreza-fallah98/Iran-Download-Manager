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

#[cfg(not(windows))]
pub fn start() -> TrayHandle {
    TrayHandle {}
}

// ── Windows implementation ───────────────────────────────────────────────────

#[cfg(windows)]
mod win {
    use super::*;
    use std::sync::OnceLock;

    // --- Constants ---
    const WM_TRAY_CALLBACK: u32 = 0x0401; // WM_USER + 1
    const WM_LBUTTONUP: u32     = 0x0202;
    const WM_LBUTTONDBLCLK: u32 = 0x0203;
    const WM_RBUTTONUP: u32     = 0x0205;
    const WM_COMMAND: u32       = 0x0111;
    const NIF_MESSAGE: u32      = 0x0001;
    const NIF_ICON: u32         = 0x0002;
    const NIF_TIP: u32          = 0x0004;
    const NIM_ADD: u32          = 0x0000_0000;
    const NIM_DELETE: u32       = 0x0000_0002;
    const IDM_SHOW: usize       = 100;
    const IDM_EXIT: usize       = 101;
    const MF_STRING: u32        = 0x0000_0000;
    const MF_SEPARATOR: u32     = 0x0000_0800;
    const TPM_RIGHTBUTTON: u32  = 0x0002;
    const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
    const WS_POPUP: u32         = 0x8000_0000;
    const IDI_APPLICATION: *const u16 = 32512usize as *const u16;

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
        fn TrackPopupMenu(menu: usize, flags: u32, x: i32, y: i32, reserved: i32,
            hwnd: usize, rect: *const std::ffi::c_void) -> i32;
        fn DestroyMenu(menu: usize) -> i32;
        fn GetCursorPos(pt: *mut POINT) -> i32;
    }

    #[link(name = "shell32")]
    extern "system" {
        fn Shell_NotifyIconW(msg: u32, data: *mut NOTIFYICONDATAW) -> i32;
    }

    // --- Global shared state ---
    struct TrayGlobal {
        exit_requested: Arc<AtomicBool>,
        show_requested: Arc<AtomicBool>,
    }

    static TRAY_GLOBAL: OnceLock<TrayGlobal> = OnceLock::new();

    // --- Window procedure ---
    unsafe extern "system" fn wnd_proc(hwnd: usize, msg: u32, wp: usize, lp: isize) -> isize {
        if msg == WM_TRAY_CALLBACK {
            let event = (lp & 0xFFFF) as u32;
            if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
                if let Some(g) = TRAY_GLOBAL.get() {
                    g.show_requested.store(true, Ordering::Relaxed);
                }
            } else if event == WM_RBUTTONUP {
                show_context_menu(hwnd);
            }
            return 0;
        }
        if msg == WM_COMMAND {
            let id = wp & 0xFFFF;
            match id {
                IDM_SHOW => {
                    if let Some(g) = TRAY_GLOBAL.get() {
                        g.show_requested.store(true, Ordering::Relaxed);
                    }
                }
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

            let icon = LoadIconW(0, IDI_APPLICATION);
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
            nid.h_icon             = icon;
            nid.sz_tip             = tip;
            Shell_NotifyIconW(NIM_ADD, &mut nid);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            Shell_NotifyIconW(NIM_DELETE, &mut nid);
        }
    }

    pub fn start_impl() -> TrayHandle {
        let exit_requested = Arc::new(AtomicBool::new(false));
        let show_requested = Arc::new(AtomicBool::new(false));
        let _ = TRAY_GLOBAL.set(TrayGlobal {
            exit_requested: exit_requested.clone(),
            show_requested: show_requested.clone(),
        });
        std::thread::spawn(tray_thread);
        TrayHandle { exit_requested, show_requested }
    }
}

#[cfg(windows)]
pub fn start() -> TrayHandle {
    win::start_impl()
}
