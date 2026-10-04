//! Read-only console-log viewer on its own port (Dubon request).
//!
//! The launcher already pipes `wgp.py` stdout/stderr into the in-app console
//! via the `launch-log` event + `LOG_HISTORY` ring buffer (`base.rs`). Remote
//! browsers on `--listen` never see that stream. This module exposes it as a
//! tiny HTTP page (e.g. `:7862` = logs while `:7860` = UI) with no new crates:
//! plain `std::net::TcpListener` on a background thread.
//!
//! Opt-in only: `log_server_start` binds `127.0.0.1` by default, `0.0.0.0`
//! when `lan=true`. Stop via `log_server_stop`; status via `log_server_status`.
//! Port default: `serverPort+2`. The thread dies with the process; generation
//! counter stops stale accept loops on restart.

use std::io::{Read, Write};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex, OnceLock,
};

struct LogServerState {
    running: bool,
    port: u64,
    lan: bool,
}

static STATE: OnceLock<Mutex<LogServerState>> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn state() -> &'static Mutex<LogServerState> {
    STATE.get_or_init(|| {
        Mutex::new(LogServerState {
            running: false,
            port: 0,
            lan: false,
        })
    })
}

fn load_ports() -> (u64, Option<u64>, Option<u64>) {
    let cfg = crate::base::load_config_value();
    let server_port = cfg
        .get("serverPort")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(7860);
    let deepy_port = cfg.get("deepyPort").and_then(serde_json::Value::as_u64);
    let log_port = cfg.get("logPort").and_then(serde_json::Value::as_u64);
    (server_port, deepy_port, log_port)
}

fn persist(port: u64, lan: bool, enabled: bool) {
    let path = crate::base::get_config_file();
    let mut cfg = crate::base::load_config_value();
    if let Some(obj) = cfg.as_object_mut() {
        obj.insert("logPort".into(), serde_json::json!(port));
        obj.insert("logLan".into(), serde_json::json!(lan));
        obj.insert("logServerEnabled".into(), serde_json::json!(enabled));
        let _ = crate::base::atomic_write(
            &path,
            &serde_json::to_string_pretty(&cfg).unwrap_or_default(),
        );
    }
}

fn lan_ip() -> Option<String> {
    local_ip_address::local_ip()
        .ok()
        .map(|ip| ip.to_string())
        .filter(|s| !s.starts_with("127."))
}

fn log_lines() -> Vec<String> {
    crate::base::LOG_HISTORY
        .get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default()
}

fn viewer_html(port: u64) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Wan2GP console :{port}</title>
<style>body{{background:#0b0e14;color:#d6deeb;font-family:monospace;margin:0}}
header{{padding:10px 14px;border-bottom:1px solid #263043;display:flex;gap:12px;align-items:center;flex-wrap:wrap}}
#log{{white-space:pre-wrap;padding:12px 14px;font-size:13px;line-height:1.5}}
button{{background:#1b2434;color:#d6deeb;border:1px solid #334155;border-radius:6px;padding:4px 10px}}</style>
</head><body>
<header><b>Wan2GP console</b><span id="st">live</span>
<button onclick="load()">refresh</button>
<label><input type="checkbox" id="auto" checked> auto (2s)</label></header>
<div id="log">loading…</div>
<script>async function load(){{try{{const r=await fetch('/logs.txt?n=600');document.getElementById('log').textContent=await r.text();window.scrollTo(0,document.body.scrollHeight);}}catch(e){{document.getElementById('st').textContent='offline: '+e;}}}}
load();setInterval(()=>{{if(document.getElementById('auto').checked)load();}},2000);</script>
</body></html>"#
    )
}

/// How many trailing log lines `/logs.txt?n=N` asked for. Clamped to 50–2000
/// so one request cannot ask the viewer to render the whole ring buffer.
fn parse_log_tail(path: &str) -> usize {
    path.split("n=")
        .nth(1)
        .and_then(|s| s.split('&').next().unwrap_or("").parse().ok())
        .unwrap_or(600)
        .clamp(50, 2000)
}

/// Last `want` lines, oldest first. Slicing from the end keeps the ring
/// buffer's insertion order, which is what the viewer renders.
fn tail_text(lines: Vec<String>, want: usize) -> String {
    let start = lines.len().saturating_sub(want);
    lines[start..].join("\n")
}

fn serve_one(mut stream: std::net::TcpStream, port: u64) {
    let mut buf = [0u8; 4096];
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    let n = stream.read(&mut buf).unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]);
    let line = req.lines().next().unwrap_or("");
    let path = line.split_whitespace().nth(1).unwrap_or("/");

    let (status, ctype, body) = if path == "/" || path == "/index.html" {
        ("200 OK", "text/html; charset=utf-8", viewer_html(port))
    } else if path.starts_with("/logs.json") {
        let lines = log_lines();
        let tail: Vec<String> = lines.into_iter().rev().take(600).rev().collect();
        (
            "200 OK",
            "application/json",
            serde_json::json!({"ok": true, "port": port, "lines": tail}).to_string(),
        )
    } else {
        // /logs.txt?n=600 — plain text tail, polled by the viewer page.
        let want = parse_log_tail(path);
        let lines = log_lines();
        ("200 OK", "text/plain; charset=utf-8", tail_text(lines, want))
    };
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

fn spawn_listener(bind: String, port: u64, gen: u64) {
    std::thread::spawn(move || {
        let listener = match std::net::TcpListener::bind(&bind) {
            Ok(l) => l,
            Err(_) => {
                if let Ok(mut s) = state().lock() {
                    if s.port == port {
                        s.running = false;
                    }
                }
                return;
            }
        };
        listener.set_nonblocking(true).ok();
        loop {
            if GENERATION.load(Ordering::SeqCst) != gen {
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => serve_one(stream, port),
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        }
    });
}

/// Start the log viewer. `port=None` → saved `logPort` → `serverPort+2`.
#[tauri::command]
pub async fn log_server_start(
    port: Option<u64>,
    lan: Option<bool>,
) -> Result<serde_json::Value, String> {
    let (server_port, deepy_port, saved_log) = load_ports();
    let lan = lan.unwrap_or(false);
    let mut p = port.or(saved_log).unwrap_or(server_port + 2);
    if p == 0 || p > 65535 {
        return Err("log port must be 1–65535".into());
    }
    if p == server_port {
        p = server_port + 2;
    }
    if Some(p) == deepy_port {
        p += 1;
    }
    // Already running on the same bind → just report status.
    if let Ok(s) = state().lock() {
        if s.running && s.port == p && s.lan == lan {
            drop(s);
            return Ok(log_server_status_inner());
        }
    }
    // Stop any previous listener, then start the new one.
    GENERATION.fetch_add(1, Ordering::SeqCst);
    std::thread::sleep(std::time::Duration::from_millis(150));
    let bind = if lan {
        format!("0.0.0.0:{p}")
    } else {
        format!("127.0.0.1:{p}")
    };
    // Fail fast if the port is taken.
    if std::net::TcpListener::bind(&bind).is_err() {
        return Err(format!("port {p} is already in use — pick another log port"));
    }
    let gen = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    spawn_listener(bind, p, gen);
    if let Ok(mut s) = state().lock() {
        s.running = true;
        s.port = p;
        s.lan = lan;
    }
    persist(p, lan, true);
    // Brief settle so the first open doesn't 404 on a cold bind.
    std::thread::sleep(std::time::Duration::from_millis(150));
    Ok(log_server_status_inner())
}

#[tauri::command]
pub async fn log_server_stop() -> Result<serde_json::Value, String> {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut s) = state().lock() {
        s.running = false;
    }
    persist(
        state().lock().map(|s| s.port).unwrap_or(0),
        state().lock().map(|s| s.lan).unwrap_or(false),
        false,
    );
    Ok(serde_json::json!({"ok": true, "running": false}))
}

fn log_server_status_inner() -> serde_json::Value {
    let (running, port, lan) = state()
        .lock()
        .map(|s| (s.running, s.port, s.lan))
        .unwrap_or((false, 0, false));
    let same_pc = if port > 0 {
        Some(format!("http://localhost:{port}"))
    } else {
        None
    };
    let phone_str = if lan {
        lan_ip().map(|ip| format!("http://{ip}:{port}"))
    } else {
        None
    };
    let phone_unavailable = if lan { phone_str.is_none() } else { true };
    serde_json::json!({
        "ok": true, "running": running, "port": port, "lan": lan,
        "samePc": same_pc.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null),
        "phone": phone_str.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null),
        "phoneUnavailable": phone_unavailable,
    })
}

#[tauri::command]
pub async fn log_server_status() -> Result<serde_json::Value, String> {
    Ok(log_server_status_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_count_parses_the_n_parameter() {
        assert_eq!(parse_log_tail("/logs.txt?n=600"), 600);
        assert_eq!(parse_log_tail("/logs.txt?n=100"), 100);
    }

    #[test]
    fn tail_count_ignores_extra_query_params() {
        assert_eq!(parse_log_tail("/logs.txt?n=250&x=1"), 250);
    }

    #[test]
    fn tail_count_defaults_and_clamps() {
        // No n=, garbage, empty → the viewer's own default.
        assert_eq!(parse_log_tail("/logs.txt"), 600);
        assert_eq!(parse_log_tail("/logs.txt?n=abc"), 600);
        assert_eq!(parse_log_tail("/logs.txt?n="), 600);
        // Clamped both ways so one request cannot render the whole buffer.
        assert_eq!(parse_log_tail("/logs.txt?n=1"), 50);
        assert_eq!(parse_log_tail("/logs.txt?n=99999"), 2000);
    }

    #[test]
    fn tail_text_keeps_the_newest_lines_in_insertion_order() {
        let lines: Vec<String> = (1..=10).map(|i| i.to_string()).collect();
        assert_eq!(tail_text(lines.clone(), 3), "8\n9\n10");
        // Asking for more than exist returns everything.
        assert_eq!(
            tail_text(lines, 100),
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10"
        );
    }

    #[test]
    fn tail_text_handles_an_empty_buffer() {
        assert_eq!(tail_text(Vec::new(), 600), "");
    }

    #[test]
    fn viewer_page_names_its_port() {
        assert!(viewer_html(7862).contains("Wan2GP console :7862"));
    }
}
