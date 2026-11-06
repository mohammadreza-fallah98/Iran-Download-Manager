fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let ico_path = std::path::Path::new(&out_dir).join("icon.ico");
    let rc_path  = std::path::Path::new(&out_dir).join("app.rc");
    let res_path = std::path::Path::new(&out_dir).join("app.res");

    // Use the high-quality ICO file directly
    let ico_src = std::path::Path::new("assets/icon.ico");
    std::fs::copy(ico_src, &ico_path).expect("assets/icon.ico not found");

    // RC script (resource ID 1 = app icon Windows picks up automatically)
    let ico_str = ico_path.to_str().unwrap().replace('\\', "/");
    std::fs::write(&rc_path, format!("1 ICON \"{ico_str}\"\n"))
        .expect("Cannot write app.rc");

    // Compile with rc.exe (MSVC SDK); if unavailable try windres (MinGW)
    let compiled = try_rc(&rc_path, &res_path) || try_windres(&rc_path, &res_path);
    if compiled {
        println!("cargo:rustc-link-arg={}", res_path.display());
    }

    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
}

fn find_rc_exe() -> Option<std::path::PathBuf> {
    // Check PATH first
    if std::process::Command::new("rc").arg("/?").output().is_ok() {
        return Some(std::path::PathBuf::from("rc"));
    }
    // Search Windows SDK locations
    let sdk_root = std::path::Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if sdk_root.exists() {
        for arch in &["x64", "x86", "arm64"] {
            // Find the latest SDK version
            if let Ok(entries) = std::fs::read_dir(sdk_root) {
                let mut versions: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                versions.sort_by_key(|e| e.file_name());
                for entry in versions.iter().rev() {
                    let candidate = entry.path().join(arch).join("rc.exe");
                    if candidate.exists() {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    None
}

fn try_rc(rc: &std::path::Path, res: &std::path::Path) -> bool {
    let Some(exe) = find_rc_exe() else { return false };
    std::process::Command::new(exe)
        .args(["/nologo", "/fo", res.to_str().unwrap(), rc.to_str().unwrap()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn try_windres(rc: &std::path::Path, res: &std::path::Path) -> bool {
    std::process::Command::new("windres")
        .args([rc.to_str().unwrap(), "-O", "coff", "-o", res.to_str().unwrap()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
