use std::io::Read as _;
use crate::model::{host_matches, host_of, Credential, ProxyConfig, ProxyKind, ProxyMode};
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) IranDownloadManager/0.1";
const MAX_PAGE_BYTES: u64 = 8 * 1024 * 1024;

static AGENT: RwLock<Option<ureq::Agent>> = RwLock::new(None);
static CREDENTIALS: RwLock<Vec<Credential>> = RwLock::new(Vec::new());
static EXTRAS: Mutex<Option<HashMap<String, Extra>>> = Mutex::new(None);

/// Headers a browser sent along with a link (kept per URL, cookies only in memory).
#[derive(Clone, Default, Debug)]
pub struct Extra {
    pub referer: String,
    pub cookies: String,
    pub user_agent: String,
}

pub fn set_extra(url: &str, extra: Extra) {
    EXTRAS.lock().unwrap().get_or_insert_with(HashMap::new).insert(url.trim().to_string(), extra);
}

pub fn get_extra(url: &str) -> Extra {
    EXTRAS.lock().unwrap().as_ref().and_then(|m| m.get(url.trim()).cloned()).unwrap_or_default()
}

pub fn configure_credentials(list: &[Credential]) {
    *CREDENTIALS.write().unwrap() = list.to_vec();
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Basic `Authorization` header value when a saved login matches this URL (by prefix or by host).
pub fn auth_header(url: &str) -> Option<String> {
    let host = host_of(url);
    let list = CREDENTIALS.read().unwrap();
    let c = list.iter().find(|c| {
        let p = c.pattern.trim();
        !p.is_empty() && !c.user.is_empty() && (url.starts_with(p) || host_matches(&host_of(p), &host))
    })?;
    Some(format!("Basic {}", base64(format!("{}:{}", c.user, c.pass).as_bytes())))
}

/// A GET request with the saved login (if any) already attached.
pub fn get(url: &str) -> ureq::Request {
    let mut req = agent_for(url).get(url);
    if let Some(a) = auth_header(url) {
        req = req.set("Authorization", &a);
    }
    let extra = get_extra(url);
    if !extra.referer.is_empty() {
        req = req.set("Referer", &extra.referer);
    }
    if !extra.cookies.is_empty() {
        req = req.set("Cookie", &extra.cookies);
    }
    if !extra.user_agent.is_empty() {
        req = req.set("User-Agent", &extra.user_agent);
    }
    req
}

static USER_AGENT_OVERRIDE: RwLock<String> = RwLock::new(String::new());

/// An empty text restores the built-in User-Agent. Call `configure_proxy` afterwards to apply it.
pub fn set_user_agent(ua: &str) {
    *USER_AGENT_OVERRIDE.write().unwrap() = ua.trim().to_string();
}

fn build_agent(proxy: Option<ureq::Proxy>) -> ureq::Agent {
    let ua = USER_AGENT_OVERRIDE.read().unwrap().clone();
    let ua = if ua.is_empty() { USER_AGENT.to_string() } else { ua };
    let mut b = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(20)).timeout_read(Duration::from_secs(30)).redirects(10).user_agent(&ua);
    if let Some(p) = proxy {
        b = b.proxy(p);
    }
    b.build()
}

/// Shared HTTP client; cheap to clone.
pub fn agent() -> ureq::Agent {
    if let Some(a) = AGENT.read().unwrap().as_ref() {
        return a.clone();
    }
    let a = build_agent(None);
    *AGENT.write().unwrap() = Some(a.clone());
    a
}

/// Rebuilds the shared client so new requests use the chosen proxy.
pub fn configure_proxy(cfg: &ProxyConfig) {
    let proxy = proxy_address(cfg).and_then(|addr| ureq::Proxy::new(addr).ok());
    *AGENT.write().unwrap() = Some(build_agent(proxy));
    *PAC_URL.write().unwrap() = if cfg.mode == ProxyMode::Pac && !cfg.pac_url.trim().is_empty() { Some(cfg.pac_url.trim().to_string()) } else { None };
    *PAC_AGENTS.lock().unwrap() = None;
    *PAC_CACHE.lock().unwrap() = None;
}

static PAC_URL: RwLock<Option<String>> = RwLock::new(None);
static PAC_AGENTS: Mutex<Option<HashMap<String, ureq::Agent>>> = Mutex::new(None);
static PAC_CACHE: Mutex<Option<HashMap<String, (std::time::Instant, Option<String>)>>> = Mutex::new(None);
const PAC_CACHE_SECS: u64 = 300;

/// The client to use for this URL: the shared one, or one that goes through the proxy the PAC script picks.
fn agent_for(url: &str) -> ureq::Agent {
    let Some(pac) = PAC_URL.read().unwrap().clone() else { return agent() };
    let host = host_of(url);
    let cached = PAC_CACHE.lock().unwrap().get_or_insert_with(HashMap::new).get(&host).filter(|(t, _)| t.elapsed().as_secs() < PAC_CACHE_SECS).map(|(_, p)| p.clone());
    let proxy = cached.unwrap_or_else(|| {
        let found = pac_proxy_for(&pac, url).map(|(h, p)| format!("{h}:{p}"));
        PAC_CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(host, (std::time::Instant::now(), found.clone()));
        found
    });
    let Some(addr) = proxy else { return build_agent(None) };
    let mut agents = PAC_AGENTS.lock().unwrap();
    agents.get_or_insert_with(HashMap::new).entry(addr.clone()).or_insert_with(|| build_agent(ureq::Proxy::new(&addr).ok())).clone()
}

/// Asks Windows (WinHTTP) to run the PAC script for this URL; `None` means connect directly.
fn pac_proxy_for(pac_url: &str, url: &str) -> Option<(String, String)> {
    #[cfg(windows)]
    unsafe {
        #[repr(C)]
        struct AutoProxyOptions {
            flags: u32,
            auto_detect_flags: u32,
            auto_config_url: *const u16,
            reserved: *mut std::ffi::c_void,
            reserved2: u32,
            auto_logon: i32,
        }
        #[repr(C)]
        struct ProxyInfo {
            access_type: u32,
            proxy: *mut u16,
            bypass: *mut u16,
        }
        #[link(name = "winhttp")]
        extern "system" {
            fn WinHttpOpen(agent: *const u16, access: u32, proxy: *const u16, bypass: *const u16, flags: u32) -> *mut std::ffi::c_void;
            fn WinHttpGetProxyForUrl(session: *mut std::ffi::c_void, url: *const u16, opts: *mut AutoProxyOptions, info: *mut ProxyInfo) -> i32;
            fn WinHttpCloseHandle(h: *mut std::ffi::c_void) -> i32;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GlobalFree(mem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        }
        const WINHTTP_ACCESS_TYPE_NO_PROXY: u32 = 1;
        const WINHTTP_AUTOPROXY_CONFIG_URL: u32 = 0x2;
        let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
        let session = WinHttpOpen(wide("IranDownloadManager").as_ptr(), WINHTTP_ACCESS_TYPE_NO_PROXY, std::ptr::null(), std::ptr::null(), 0);
        if session.is_null() {
            return None;
        }
        let pac = wide(pac_url);
        let target = wide(url);
        let mut opts = AutoProxyOptions { flags: WINHTTP_AUTOPROXY_CONFIG_URL, auto_detect_flags: 0, auto_config_url: pac.as_ptr(), reserved: std::ptr::null_mut(), reserved2: 0, auto_logon: 1 };
        let mut info = ProxyInfo { access_type: 0, proxy: std::ptr::null_mut(), bypass: std::ptr::null_mut() };
        let ok = WinHttpGetProxyForUrl(session, target.as_ptr(), &mut opts, &mut info) != 0;
        WinHttpCloseHandle(session);
        if !ok {
            return None;
        }
        let mut text = String::new();
        if !info.proxy.is_null() {
            let mut len = 0;
            while *info.proxy.add(len) != 0 {
                len += 1;
            }
            text = String::from_utf16_lossy(std::slice::from_raw_parts(info.proxy, len));
            GlobalFree(info.proxy as *mut _);
        }
        if !info.bypass.is_null() {
            GlobalFree(info.bypass as *mut _);
        }
        return parse_proxy_server(&text);
    }
    #[cfg(not(windows))]
    {
        let _ = (pac_url, url);
        None
    }
}

fn proxy_address(cfg: &ProxyConfig) -> Option<String> {
    match cfg.mode {
        ProxyMode::None | ProxyMode::Pac => None,
        ProxyMode::System => system_proxy().map(|(h, p)| format!("{h}:{p}")),
        ProxyMode::Manual => {
            let host = cfg.host.trim();
            if host.is_empty() {
                return None;
            }
            let port = if cfg.port.trim().is_empty() { "8080" } else { cfg.port.trim() };
            let auth = if cfg.user.is_empty() { String::new() } else { format!("{}:{}@", cfg.user, cfg.pass) };
            let scheme = if cfg.kind == ProxyKind::Socks5 { "socks5://" } else { "" };
            Some(format!("{scheme}{auth}{host}:{port}"))
        }
    }
}

/// The proxy configured in Windows Internet settings, as (host, port).
pub fn system_proxy() -> Option<(String, String)> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x08000000;
        let query = |name: &str| -> Option<String> {
            let out = std::process::Command::new("reg")
                .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings", "/v", name])
                .creation_flags(NO_WINDOW)
                .output()
                .ok()?;
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.lines().find(|l| l.contains(name)).and_then(|l| l.split_whitespace().last().map(str::to_string))
        };
        if query("ProxyEnable")? != "0x1" {
            return None;
        }
        return parse_proxy_server(&query("ProxyServer")?);
    }
    #[cfg(not(windows))]
    None
}

/// Accepts `host:port` or `http=host:port;https=host:port`.
fn parse_proxy_server(value: &str) -> Option<(String, String)> {
    let entry = value
        .split(';')
        .find(|e| e.starts_with("https="))
        .or_else(|| value.split(';').find(|e| e.starts_with("http=")))
        .or_else(|| value.split(';').find(|e| !e.contains('=')))?;
    let addr = entry.split_once('=').map(|(_, a)| a).unwrap_or(entry);
    let (host, port) = addr.rsplit_once(':')?;
    Some((host.to_string(), port.to_string()))
}

pub fn describe_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _) => format!("Server answered {code}"),
        ureq::Error::Transport(t) => format!("Connection failed. {}", t.message().unwrap_or("")),
    }
}

#[derive(Clone, Debug)]
pub struct ProbeInfo {
    pub size: Option<u64>,
    pub resumable: bool,
    pub file_name: String,
    pub last_modified: Option<String>,
}

/// Asks for the whole file with an open range: a 206 answer means parts can be fetched separately.
pub fn probe(url: &str) -> Result<ProbeInfo, String> {
    if crate::ftp::is_ftp_url(url) {
        return crate::ftp::probe(url);
    }
    let resp = get(url).set("Range", "bytes=0-").call().map_err(describe_error)?;
    let resumable = resp.status() == 206;
    let size = if resumable {
        resp.header("Content-Range").and_then(|v| v.rsplit('/').next()).and_then(|v| v.trim().parse().ok())
    } else {
        resp.header("Content-Length").and_then(|v| v.trim().parse().ok())
    };
    let from_header = resp.header("Content-Disposition").and_then(file_name_from_disposition);
    let file_name = from_header.unwrap_or_else(|| file_name_from_url(resp.get_url()));
    Ok(ProbeInfo { size, resumable, file_name: sanitize_file_name(&file_name), last_modified: resp.header("Last-Modified").map(str::to_string) })
}

pub fn file_name_from_disposition(value: &str) -> Option<String> {
    for item in value.split(';').map(str::trim) {
        if let Some(v) = item.strip_prefix("filename*=") {
            let v = v.splitn(3, '\'').last().unwrap_or(v);
            return Some(percent_decode(v.trim_matches('"')));
        }
    }
    value.split(';').map(str::trim).find_map(|i| i.strip_prefix("filename=")).map(|v| v.trim_matches('"').to_string())
}

pub fn file_name_from_url(url: &str) -> String {
    let no_query = url.split(['?', '#']).next().unwrap_or(url);
    let after_scheme = no_query.split_once("://").map(|(_, r)| r).unwrap_or(no_query);
    let last = after_scheme.split_once('/').and_then(|(_, path)| path.rsplit('/').next()).unwrap_or("");
    let name = percent_decode(last);
    if name.is_empty() { "index.html".to_string() } else { name }
}

pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() { "download".to_string() } else { trimmed.to_string() }
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn looks_like_url(text: &str) -> bool {
    let t = text.trim();
    (t.starts_with("http://") || t.starts_with("https://") || t.starts_with("ftp://")) && !t.contains(char::is_whitespace) && t.len() > 10
}

/// Downloads one page and lists every linked file (href/src), resolved to full addresses.
pub fn grab_links(page_url: &str) -> Result<Vec<String>, String> {
    let resp = get(page_url).call().map_err(describe_error)?;
    let base = resp.get_url().to_string();
    let mut body = String::new();
    resp.into_reader().take(MAX_PAGE_BYTES).read_to_string(&mut body).map_err(|e| e.to_string())?;
    let mut links: Vec<String> = Vec::new();
    for attr in ["href=", "src="] {
        let lower = body.to_ascii_lowercase();
        let mut from = 0;
        while let Some(i) = lower[from..].find(attr) {
            let start = from + i + attr.len();
            from = start;
            let rest = &body[start..];
            let value = match rest.chars().next() {
                Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
                _ => rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
            };
            if value.is_empty() || value.starts_with('#') || value.starts_with("javascript:") || value.starts_with("mailto:") {
                continue;
            }
            let full = join_url(&base, &value.replace("&amp;", "&"));
            if !links.contains(&full) {
                links.push(full);
            }
        }
    }
    Ok(links)
}

pub fn join_url(base: &str, rel: &str) -> String {
    if rel.starts_with("http://") || rel.starts_with("https://") {
        return rel.to_string();
    }
    let (scheme, rest) = base.split_once("://").unwrap_or(("https", base));
    if let Some(r) = rel.strip_prefix("//") {
        return format!("{scheme}://{r}");
    }
    let host = rest.split('/').next().unwrap_or(rest);
    if rel.starts_with('/') {
        return format!("{scheme}://{host}{rel}");
    }
    let path = rest.split(['?', '#']).next().unwrap_or(rest);
    let dir = match path.rfind('/') {
        Some(i) if i >= host.len() => path[..=i].to_string(),
        _ => format!("{host}/"),
    };
    format!("{scheme}://{dir}{rel}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extras_are_stored_per_trimmed_url() {
        set_extra(" http://x.test/a.mp4 ", Extra { referer: "http://x.test/".into(), cookies: "a=1".into(), user_agent: String::new() });
        assert_eq!(get_extra("http://x.test/a.mp4").cookies, "a=1");
        assert_eq!(get_extra("http://x.test/other.mp4").referer, "");
    }

    #[test]
    fn base64_encodes_with_padding() {
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"a"), "YQ==");
    }

    #[test]
    fn auth_header_matches_by_host_and_prefix() {
        configure_credentials(&[Credential { pattern: "http://192.168.10.21".into(), user: "u".into(), pass: "p".into(), pass_enc: String::new() }]);
        assert_eq!(auth_header("http://192.168.10.21/files/a.zip").as_deref(), Some("Basic dTpw"));
        assert_eq!(auth_header("http://other.com/a.zip"), None);
        configure_credentials(&[]);
    }

    #[test]
    fn proxy_address_builds_http_and_socks5_forms() {
        let mut cfg = ProxyConfig { mode: ProxyMode::Manual, host: "10.0.0.1".into(), port: "1080".into(), ..Default::default() };
        assert_eq!(proxy_address(&cfg).as_deref(), Some("10.0.0.1:1080"));
        cfg.kind = ProxyKind::Socks5;
        cfg.user = "u".into();
        cfg.pass = "p".into();
        assert_eq!(proxy_address(&cfg).as_deref(), Some("socks5://u:p@10.0.0.1:1080"));
        assert!(ureq::Proxy::new("socks5://u:p@10.0.0.1:1080").is_ok());
        assert_eq!(proxy_address(&ProxyConfig::default()), None);
    }

    /// Windows evaluates the PAC script; the test serves one from a local socket.
    #[cfg(windows)]
    #[test]
    fn pac_script_is_resolved_through_winhttp() {
        use std::io::Write as _;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                // read the request first; closing with unread data would reset the connection
                let mut req = [0u8; 2048];
                let _ = std::io::Read::read(&mut stream, &mut req);
                let body = "function FindProxyForURL(url, host) { if (shExpMatch(host, \"*.local\")) return \"DIRECT\"; return \"PROXY 127.0.0.1:8888\"; }";
                let reply = format!("HTTP/1.1 200 OK
Content-Type: application/x-ns-proxy-autoconfig
Content-Length: {}
Connection: close

{body}", body.len());
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        let pac = format!("http://127.0.0.1:{port}/proxy.pac");
        assert_eq!(pac_proxy_for(&pac, "http://example.com/a.zip"), Some(("127.0.0.1".into(), "8888".into())));
        assert_eq!(pac_proxy_for(&pac, "http://printer.local/a.zip"), None);
    }

    #[test]
    fn looks_like_url_accepts_ftp() {
        assert!(looks_like_url("ftp://host.example.com/file.zip"));
        assert!(!looks_like_url("file:///C:/a.zip"));
    }

    #[test]
    fn parse_proxy_server_reads_plain_and_protocol_forms() {
        assert_eq!(parse_proxy_server("127.0.0.1:8080"), Some(("127.0.0.1".into(), "8080".into())));
        assert_eq!(parse_proxy_server("http=a:1;https=b:2"), Some(("b".into(), "2".into())));
        assert_eq!(parse_proxy_server("ftp=c:3"), None);
    }

    #[test]
    fn file_name_from_url_strips_query_and_decodes() {
        assert_eq!(file_name_from_url("https://a.com/x/my%20file.zip?x=1"), "my file.zip");
        assert_eq!(file_name_from_url("https://a.com/"), "index.html");
        assert_eq!(file_name_from_url("https://a.com"), "index.html");
    }

    #[test]
    fn file_name_from_disposition_reads_both_forms() {
        assert_eq!(file_name_from_disposition("attachment; filename=\"a b.pdf\"").as_deref(), Some("a b.pdf"));
        assert_eq!(file_name_from_disposition("attachment; filename*=UTF-8''r%C3%A9.zip").as_deref(), Some("ré.zip"));
        assert_eq!(file_name_from_disposition("inline"), None);
    }

    #[test]
    fn join_url_resolves_relative_links() {
        assert_eq!(join_url("https://a.com/d/page.html", "f.zip"), "https://a.com/d/f.zip");
        assert_eq!(join_url("https://a.com/d/page.html", "/f.zip"), "https://a.com/f.zip");
        assert_eq!(join_url("https://a.com", "f.zip"), "https://a.com/f.zip");
        assert_eq!(join_url("https://a.com/d/", "//b.com/f.zip"), "https://b.com/f.zip");
    }

    #[test]
    fn sanitize_file_name_replaces_reserved_characters() {
        assert_eq!(sanitize_file_name("a:b?.zip"), "a_b_.zip");
        assert_eq!(sanitize_file_name("..."), "download");
    }
}
