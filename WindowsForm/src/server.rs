/// Lightweight HTTP server on localhost:6800.
/// Browser extensions POST JSON to /add and the app picks it up via a channel.
use serde::Deserialize;
use std::sync::mpsc::Sender;

pub const PORT: u16 = 6800;

static CONFIG_JSON: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());
static LAST_SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Settings the browser extensions read with `GET /config`.
pub fn publish_config(s: &crate::model::Settings) {
    let json = serde_json::json!({
        "auto_extensions": s.auto_extensions,
        "excluded_sites": s.excluded_sites,
        "block_key": s.block_key.name(),
        "intercept": s.ext_intercept,
        "video_button": s.ext_video_button,
    });
    *CONFIG_JSON.write().unwrap() = json.to_string();
}

/// Seconds since a browser extension last talked to the app, if it ever did.
pub fn seconds_since_extension_seen() -> Option<u64> {
    let t = LAST_SEEN.load(std::sync::atomic::Ordering::Relaxed);
    if t == 0 {
        return None;
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    Some(now.saturating_sub(t))
}

fn mark_seen() {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    LAST_SEEN.store(now, std::sync::atomic::Ordering::Relaxed);
}

#[derive(Deserialize, Debug, Clone)]
pub struct AddRequest {
    pub url: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub referrer: String,
    #[serde(default)]
    pub cookies: String,
    #[serde(default)]
    pub user_agent: String,
}

pub fn spawn(tx: Sender<AddRequest>) {
    std::thread::spawn(move || run(tx));
}

fn run(tx: Sender<AddRequest>) {
    let addr = format!("127.0.0.1:{PORT}");
    let server = match tiny_http::Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[server] failed to bind {addr}: {e}");
            return;
        }
    };
    for mut req in server.incoming_requests() {
        // Allow browser extensions from any origin
        let origin = req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("Origin")).map(|h| h.value.to_string()).unwrap_or_default();

        if req.method() == &tiny_http::Method::Options {
            let _ = req.respond(cors_response(&origin, tiny_http::Response::empty(204)));
            continue;
        }

        if req.url() == "/config" && req.method() == &tiny_http::Method::Get {
            mark_seen();
            let body = CONFIG_JSON.read().unwrap().clone();
            let body = if body.is_empty() { "{}".to_string() } else { body };
            let _ = req.respond(cors_response(&origin, tiny_http::Response::from_string(body).with_header(json_header())));
            continue;
        }

        if req.url() != "/add" || req.method() != &tiny_http::Method::Post {
            let _ = req.respond(cors_response(&origin, tiny_http::Response::from_string("Not found").with_status_code(404)));
            continue;
        }

        let mut body = String::new();
        if req.as_reader().read_to_string(&mut body).is_err() {
            let _ = req.respond(cors_response(&origin, tiny_http::Response::from_string("Bad request").with_status_code(400)));
            continue;
        }

        match serde_json::from_str::<AddRequest>(&body) {
            Ok(r) if !r.url.is_empty() => {
                mark_seen();
                crate::net::set_extra(&r.url, crate::net::Extra { referer: r.referrer.clone(), cookies: r.cookies.clone(), user_agent: r.user_agent.clone() });
                let _ = tx.send(r);
                // update() does not run while the window is hidden in the tray, so show it from here
                crate::tray::wake();
                let _ = req.respond(cors_response(&origin, tiny_http::Response::from_string(r#"{"ok":true}"#).with_header(json_header())));
            }
            _ => {
                let _ = req.respond(cors_response(&origin, tiny_http::Response::from_string(r#"{"ok":false}"#).with_status_code(400).with_header(json_header())));
            }
        }
    }
}

fn json_header() -> tiny_http::Header {
    tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap()
}

fn cors_response<R: std::io::Read>(origin: &str, resp: tiny_http::Response<R>) -> tiny_http::Response<R> {
    let allow_origin = if origin.is_empty() { "*".to_string() } else { origin.to_string() };
    resp.with_header(tiny_http::Header::from_bytes("Access-Control-Allow-Origin", allow_origin.as_str()).unwrap())
        .with_header(tiny_http::Header::from_bytes("Access-Control-Allow-Methods", "GET, POST, OPTIONS").unwrap())
        .with_header(tiny_http::Header::from_bytes("Access-Control-Allow-Headers", "Content-Type").unwrap())
}
