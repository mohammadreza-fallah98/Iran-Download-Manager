use crate::model::SoundSetting;

#[cfg(windows)]
#[link(name = "winmm")]
extern "system" {
    fn PlaySoundW(name: *const u16, module: usize, flags: u32) -> i32;
}

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn MessageBeep(kind: u32) -> i32;
}

/// Plays the chosen sound without blocking; an empty path gives the system beep.
pub fn play(s: &SoundSetting) {
    if !s.enabled {
        return;
    }
    play_path(&s.path);
}

pub fn play_path(path: &str) {
    #[cfg(windows)]
    unsafe {
        const SND_ASYNC: u32 = 0x0001;
        const SND_FILENAME: u32 = 0x0002_0000;
        const MB_ICONASTERISK: u32 = 0x40;
        let path = path.trim();
        if path.is_empty() || !std::path::Path::new(path).is_file() {
            MessageBeep(MB_ICONASTERISK);
            return;
        }
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        PlaySoundW(wide.as_ptr(), 0, SND_FILENAME | SND_ASYNC);
    }
    #[cfg(not(windows))]
    let _ = path;
}
