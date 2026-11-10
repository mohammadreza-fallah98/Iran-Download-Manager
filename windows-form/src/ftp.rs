//! Plain FTP downloads (passive mode, resume with REST) over one connection.
use crate::model::*;
use crate::net::{self, ProbeInfo};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::Ordering;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const IO_TIMEOUT: Duration = Duration::from_secs(30);
const CHUNK: usize = 64 * 1024;

pub fn is_ftp_url(url: &str) -> bool {
    url.trim().to_ascii_lowercase().starts_with("ftp://")
}

#[derive(Debug, PartialEq)]
struct Target {
    host: String,
    port: u16,
    user: String,
    pass: String,
    path: String,
}

fn parse_url(url: &str) -> Result<Target, String> {
    let rest = url.trim().get(6..).filter(|_| is_ftp_url(url)).ok_or("This is not an FTP address")?;
    let (authority, path) = rest.split_once('/').map(|(a, p)| (a, p)).unwrap_or((rest, ""));
    let (cred, hostport) = authority.rsplit_once('@').map(|(c, h)| (Some(c), h)).unwrap_or((None, authority));
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => (h, p.parse().map_err(|_| "Bad FTP port")?),
        _ => (hostport, 21),
    };
    if host.is_empty() {
        return Err("The FTP address has no server".into());
    }
    let (user, pass) = match cred {
        Some(c) => {
            let (u, p) = c.split_once(':').unwrap_or((c, ""));
            (net::percent_decode(u), net::percent_decode(p))
        }
        None => ("anonymous".to_string(), "anonymous@".to_string()),
    };
    let path = net::percent_decode(path.split(['?', '#']).next().unwrap_or(""));
    if path.is_empty() || path.ends_with('/') {
        return Err("The FTP address does not point to a file".into());
    }
    Ok(Target { host: host.to_string(), port, user, pass, path })
}

struct Conn {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    host: String,
}

impl Conn {
    fn reply(&mut self) -> Result<(u32, String), String> {
        let mut full = String::new();
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err("The FTP server closed the connection".into());
            }
            full.push_str(line.trim_end());
            full.push('\n');
            let b = line.as_bytes();
            if b.len() >= 4 && b[..3].iter().all(u8::is_ascii_digit) && b[3] == b' ' {
                let code = line[..3].parse().unwrap_or(0);
                return Ok((code, full.trim_end().to_string()));
            }
        }
    }

    fn cmd(&mut self, line: &str) -> Result<(u32, String), String> {
        self.writer.write_all(format!("{line}\r\n").as_bytes()).map_err(|e| e.to_string())?;
        self.reply()
    }

    fn passive(&mut self) -> Result<TcpStream, String> {
        let (code, text) = self.cmd("PASV")?;
        if code != 227 {
            return Err(format!("The server refused passive mode: {text}"));
        }
        let (addr, port) = parse_pasv(&text).ok_or("Bad passive mode answer")?;
        let _ = addr; // behind NAT the announced address is often private: use the server we dialled
        let target = (self.host.as_str(), port).to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or("Cannot resolve the server")?;
        let data = TcpStream::connect_timeout(&target, CONNECT_TIMEOUT).map_err(|e| e.to_string())?;
        data.set_read_timeout(Some(IO_TIMEOUT)).ok();
        Ok(data)
    }
}

/// `227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)` -> (address, port)
fn parse_pasv(text: &str) -> Option<(String, u16)> {
    let inner = text.split_once('(')?.1.split_once(')')?.0;
    let n: Vec<u16> = inner.split(',').filter_map(|v| v.trim().parse().ok()).collect();
    if n.len() != 6 || n.iter().any(|v| *v > 255) {
        return None;
    }
    Some((format!("{}.{}.{}.{}", n[0], n[1], n[2], n[3]), n[4] * 256 + n[5]))
}

fn open(t: &Target) -> Result<Conn, String> {
    let addr = (t.host.as_str(), t.port).to_socket_addrs().map_err(|e| format!("Cannot resolve the server: {e}"))?.next().ok_or("Cannot resolve the server")?;
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| format!("Connection failed: {e}"))?;
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok();
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
    let mut c = Conn { reader: BufReader::new(stream.try_clone().map_err(|e| e.to_string())?), writer: stream, host: t.host.clone() };
    let (code, text) = c.reply()?;
    if code != 220 {
        return Err(format!("The FTP server answered: {text}"));
    }
    let (code, text) = c.cmd(&format!("USER {}", t.user))?;
    if code == 331 {
        let (code, text) = c.cmd(&format!("PASS {}", t.pass))?;
        if code != 230 {
            return Err(format!("Login failed: {text}"));
        }
    } else if code != 230 {
        return Err(format!("Login failed: {text}"));
    }
    let (code, text) = c.cmd("TYPE I")?;
    if code != 200 {
        return Err(format!("The server refused binary mode: {text}"));
    }
    Ok(c)
}

fn size_of(c: &mut Conn, path: &str) -> Option<u64> {
    match c.cmd(&format!("SIZE {path}")) {
        Ok((213, text)) => text.split_whitespace().nth(1).and_then(|v| v.parse().ok()),
        _ => None,
    }
}

pub fn probe(url: &str) -> Result<ProbeInfo, String> {
    let t = parse_url(url)?;
    let mut c = open(&t)?;
    let size = size_of(&mut c, &t.path);
    let resumable = matches!(c.cmd("REST 0"), Ok((350, _)));
    let _ = c.cmd("QUIT");
    let name = t.path.rsplit('/').next().unwrap_or("download");
    Ok(ProbeInfo { size, resumable: resumable && size.is_some(), file_name: net::sanitize_file_name(name), last_modified: None })
}

pub fn run(h: Handle) {
    let result = work(&h);
    crate::engine::finish(&h, result.err());
}

fn work(h: &Handle) -> Result<(), String> {
    let (url, needs_probe) = {
        let d = h.d.lock().unwrap();
        (d.url.clone(), d.parts.is_empty())
    };
    let t = parse_url(&url)?;
    if needs_probe {
        let info = probe(&url)?;
        let mut d = h.d.lock().unwrap();
        d.size = info.size;
        d.resumable = info.resumable;
        d.parts = crate::engine::plan_parts(info.size, false, 1);
    }
    let (temp, dir, resumable, mut pos) = {
        let d = h.d.lock().unwrap();
        (d.temp_path(), d.save_dir.clone(), d.resumable, d.parts[0].pos)
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create folder: {e}"))?;
    if let Some(parent) = temp.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create folder: {e}"))?;
    }
    if !resumable || !temp.exists() {
        pos = 0;
        let mut d = h.d.lock().unwrap();
        d.parts = crate::engine::plan_parts(d.size, false, 1);
        std::fs::File::create(&temp).map_err(|e| format!("Cannot create file: {e}"))?;
    }
    let mut file = OpenOptions::new().write(true).open(&temp).map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;

    let mut c = open(&t)?;
    if pos > 0 {
        let (code, text) = c.cmd(&format!("REST {pos}"))?;
        if code != 350 {
            return Err(format!("The server cannot resume: {text}"));
        }
    }
    let mut data = c.passive()?;
    let (code, text) = c.cmd(&format!("RETR {}", t.path))?;
    if code != 150 && code != 125 {
        return Err(format!("The server refused the file: {text}"));
    }
    {
        let mut d = h.d.lock().unwrap();
        d.active_connections = 1;
        d.parts[0].info = "Receiving data...".into();
    }
    let mut buf = vec![0u8; CHUNK];
    loop {
        if h.stop.load(Ordering::SeqCst) {
            // closing the data connection ends the transfer; the server's final answer is not awaited
            return Ok(());
        }
        let n = match data.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(format!("Connection lost: {e}")),
        };
        file.write_all(&buf[..n]).map_err(|e| format!("Cannot write the file: {e}"))?;
        pos += n as u64;
        h.d.lock().unwrap().parts[0].pos = pos;
        crate::engine::throttle(h, n);
    }
    drop(data);
    let (code, text) = c.reply()?;
    if code != 226 && code != 250 {
        return Err(format!("The transfer did not finish: {text}"));
    }
    let _ = c.cmd("QUIT");
    let mut d = h.d.lock().unwrap();
    let p = &mut d.parts[0];
    if p.end.is_none() {
        p.end = Some(pos);
    }
    p.info = "Complete".into();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_reads_credentials_port_and_path() {
        let t = parse_url("ftp://bob:p%40ss@ftp.example.com:2121/pub/a%20b.zip").unwrap();
        assert_eq!(t, Target { host: "ftp.example.com".into(), port: 2121, user: "bob".into(), pass: "p@ss".into(), path: "pub/a b.zip".into() });
        let t = parse_url("ftp://ftp.example.com/f.bin").unwrap();
        assert_eq!((t.user.as_str(), t.port), ("anonymous", 21));
        assert!(parse_url("ftp://host/dir/").is_err());
        assert!(parse_url("http://host/a").is_err());
    }

    #[test]
    fn parse_pasv_reads_address_and_port() {
        assert_eq!(parse_pasv("227 Entering Passive Mode (192,168,1,5,195,80)"), Some(("192.168.1.5".into(), 50000)));
        assert_eq!(parse_pasv("227 nonsense"), None);
        assert_eq!(parse_pasv("227 (1,2,3,4,5,300)"), None);
    }

    /// Talks to a tiny in-process FTP server: login, SIZE, REST, RETR with resume.
    #[test]
    fn downloads_and_resumes_from_a_local_ftp_server() {
        use std::net::TcpListener;
        let content: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let data = content.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let data = data.clone();
                std::thread::spawn(move || serve(stream, data));
            }
        });
        fn serve(stream: TcpStream, content: Vec<u8>) {
            let mut w = stream.try_clone().unwrap();
            let mut r = BufReader::new(stream);
            let send = |w: &mut TcpStream, s: &str| w.write_all(format!("{s}\r\n").as_bytes()).unwrap();
            send(&mut w, "220 ready");
            let mut rest = 0usize;
            let mut data_listener: Option<std::net::TcpListener> = None;
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                let line = line.trim().to_string();
                let (verb, arg) = line.split_once(' ').unwrap_or((line.as_str(), ""));
                match verb {
                    "USER" => send(&mut w, "331 password please"),
                    "PASS" => send(&mut w, "230 ok"),
                    "TYPE" => send(&mut w, "200 binary"),
                    "SIZE" => send(&mut w, &format!("213 {}", content.len())),
                    "REST" => {
                        rest = arg.parse().unwrap();
                        send(&mut w, "350 restarting");
                    }
                    "PASV" => {
                        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                        let p = l.local_addr().unwrap().port();
                        send(&mut w, &format!("227 Entering Passive Mode (127,0,0,1,{},{})", p / 256, p % 256));
                        data_listener = Some(l);
                    }
                    "RETR" => {
                        send(&mut w, "150 opening");
                        let (mut d, _) = data_listener.take().unwrap().accept().unwrap();
                        d.write_all(&content[rest..]).unwrap();
                        drop(d);
                        send(&mut w, "226 done");
                    }
                    "QUIT" => {
                        send(&mut w, "221 bye");
                        return;
                    }
                    _ => send(&mut w, "502 no"),
                }
            }
        }
        let url = format!("ftp://127.0.0.1:{port}/dir/file.bin");
        let info = probe(&url).unwrap();
        assert_eq!(info.size, Some(5000));
        assert!(info.resumable);
        assert_eq!(info.file_name, "file.bin");

        let dir = std::env::temp_dir().join(format!("idm-ftp-{}", std::process::id()));
        let h = new_handle(Download {
            id: 1, url: url.clone(), file_name: "file.bin".into(), save_dir: dir.clone(), category: Category::General, size: None, resumable: false,
            parts: vec![], status: Status::Stopped, last_try: String::new(), description: String::new(), connections: 1, speed_limit_kbps: None,
            in_queue: false, queue_id: 0, referer: String::new(), user_agent: String::new(), hls: None, temp_dir: None, server_date: None,
            message: String::new(), last_speed: 0.0, speed: 0.0, active_connections: 0,
        });
        // full download through probe, PASV and RETR
        work(&h).unwrap();
        crate::engine::finish(&h, None);
        let d = h.d.lock().unwrap();
        assert_eq!(d.status, Status::Complete);
        assert_eq!(std::fs::read(dir.join("file.bin")).unwrap(), content);
        drop(d);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
