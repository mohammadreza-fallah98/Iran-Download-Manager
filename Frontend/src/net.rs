use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) IranDownloadManager/0.1";
const MAX_PAGE_BYTES: u64 = 8 * 1024 * 1024;

pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            .timeout_read(Duration::from_secs(30))
            .redirects(10)
            .user_agent(USER_AGENT)
            .build()
    })
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
}

/// Asks for the whole file with an open range: a 206 answer means parts can be fetched separately.
pub fn probe(url: &str) -> Result<ProbeInfo, String> {
    let resp = agent().get(url).set("Range", "bytes=0-").call().map_err(describe_error)?;
    let resumable = resp.status() == 206;
    let size = if resumable {
        resp.header("Content-Range").and_then(|v| v.rsplit('/').next()).and_then(|v| v.trim().parse().ok())
    } else {
        resp.header("Content-Length").and_then(|v| v.trim().parse().ok())
    };
    let from_header = resp.header("Content-Disposition").and_then(file_name_from_disposition);
    let file_name = from_header.unwrap_or_else(|| file_name_from_url(resp.get_url()));
    Ok(ProbeInfo { size, resumable, file_name: sanitize_file_name(&file_name) })
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
    (t.starts_with("http://") || t.starts_with("https://")) && !t.contains(char::is_whitespace) && t.len() > 10
}

/// Downloads one page and lists every linked file (href/src), resolved to full addresses.
pub fn grab_links(page_url: &str) -> Result<Vec<String>, String> {
    let resp = agent().get(page_url).call().map_err(describe_error)?;
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
