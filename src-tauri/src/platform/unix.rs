#![allow(dead_code)]

//! Pure parsers for the Unix tools the Linux/macOS adapters shell out to.
//!
//! Keeping the parsing here (no OS calls) means every one of these runs in the
//! Windows test suite too — the port/process logic cannot rot away unexercised
//! just because the CI runner for that OS is unavailable.
//!
//! Tools: `lsof` (LISTEN sockets and PIDs), `ss` (iproute2, Linux fallback for
//! hosts without lsof), `pgrep` (command-line scan).

use crate::platform::{Listener, PortOwner, ProcessInfo};

/// `lsof -ti tcp:PORT` output: one PID per line. Anything unparsable (a
/// warning line, an empty line) is skipped rather than aborting the scan.
pub(crate) fn parse_lsof_pids(stdout: &str) -> Vec<u32> {
    stdout
        .lines()
        .filter_map(|l| l.trim().parse::<u32>().ok())
        .filter(|p| *p != 0)
        .collect()
}

/// Trailing `:PORT` of a socket address. Handles `0.0.0.0:7860`, `*:7860`,
/// `[::]:7860`, `[::1]:7860`, and refuses non-numeric ports.
pub(crate) fn port_of_addr(addr: &str) -> Option<u64> {
    let port = addr.rsplit(':').next()?.trim();
    if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    port.parse().ok()
}

/// `ss -ltnp` rows → listeners. Real rows look like:
///
/// ```text
/// State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process
/// LISTEN 0      4096   0.0.0.0:7860       0.0.0.0:*         users:(("python3.12",pid=4242,fd=7))
/// LISTEN 0      4096   [::]:7860            [::]:*            users:(("python3.12",pid=4242,fd=8))
/// LISTEN 0      4096   127.0.0.1:631        0.0.0.0:*
/// ```
///
/// A row without a `users:` section belongs to another user (ss hides it), so
/// it yields no listener: guessing a PID there would risk killing a stranger.
pub(crate) fn parse_ss_listeners(stdout: &str) -> Vec<Listener> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("LISTEN") {
            continue;
        }
        // Recv-Q, Send-Q, Local Address:Port, Peer Address:Port, Process.
        let local = match fields.nth(2) {
            Some(a) => a,
            None => continue,
        };
        let rest: Vec<&str> = fields.collect();
        let users = match rest.iter().find(|f| f.starts_with("users:(")) {
            Some(u) => *u,
            None => continue,
        };
        let Some(pid) = pid_from_users(users) else {
            continue;
        };
        let Some(port) = port_of_addr(local) else {
            continue;
        };
        out.push(Listener {
            pid,
            name: name_from_users(users),
            port,
        });
    }
    out
}

/// `users:(("python3.12",pid=4242,fd=7),("python3.12",pid=4242,fd=8))` → 4242.
fn pid_from_users(users: &str) -> Option<u32> {
    let start = users.find("pid=")? + 4;
    let digits: String = users[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok().filter(|p| *p != 0)
}

/// First quoted program name inside the `users:(...)` list, quotes stripped.
fn name_from_users(users: &str) -> String {
    let Some(open) = users.find("((\"") else {
        return String::new();
    };
    users[open + 3..]
        .split('"')
        .next()
        .unwrap_or_default()
        .to_string()
}

/// `lsof -nP -iTCP -sTCP:LISTEN -F pcn` output, records of pid/command/name:
///
/// ```text
/// p4242
/// cpython3.12
/// n*:7860
/// n127.0.0.1:7861
/// p900
/// cnode
/// n[::1]:5173
/// ```
///
/// The field form is used instead of the column form because command names may
/// contain spaces, which shifts every column in the default output.
pub(crate) fn parse_lsof_field_listeners(stdout: &str) -> Vec<Listener> {
    let mut out = Vec::new();
    let mut pid: Option<u32> = None;
    let mut name = String::new();
    for line in stdout.lines() {
        let line = line.trim();
        let Some((tag, value)) = line.split_at_checked(1) else {
            continue;
        };
        match tag {
            "p" => {
                pid = value.parse::<u32>().ok().filter(|p| *p != 0);
                name.clear();
            }
            "c" => name = value.to_string(),
            "n" => {
                let (Some(pid), Some(port)) = (pid, port_of_addr(strip_listen(value))) else {
                    continue;
                };
                out.push(Listener {
                    pid,
                    name: name.clone(),
                    port,
                });
            }
            _ => {}
        }
    }
    out
}

/// `lsof` may decorate the name with a state suffix (`*:7860 (LISTEN)`).
fn strip_listen(name: &str) -> &str {
    match name.find(" (") {
        Some(i) => &name[..i],
        None => name,
    }
}

/// `pgrep -af MARKER` output: `PID COMMAND LINE...`.
pub(crate) fn parse_pgrep(stdout: &str) -> Vec<ProcessInfo> {
    stdout
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let (pid, cmd) = line.split_once(char::is_whitespace)?;
            let pid = pid.parse::<u32>().ok().filter(|p| *p != 0)?;
            let cmd = cmd.trim();
            if cmd.is_empty() {
                return None;
            }
            Some(ProcessInfo {
                pid,
                // pgrep gives no executable path; is_ours_process falls back to
                // the command line, which carries the interpreter under the repo.
                exe: String::new(),
                cmd: cmd.to_string(),
            })
        })
        .collect()
}

/// Runs `prog args…` and returns stdout. A missing binary and a non-zero exit
/// are both errors — the caller decides whether they are fatal or a fallback
/// trigger.
pub(crate) fn run(prog: &str, args: &[&str]) -> Result<String, String> {
    let o = crate::base::silent_command(prog)
        .args(args)
        .output()
        .map_err(|e| format!("{prog}: {e}"))?;
    if !o.status.success() {
        return Err(format!(
            "{prog} {} exited {}",
            args.join(" "),
            o.status.code().unwrap_or(-1)
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).to_string())
}

/// LISTEN sockets from `ss -ltnp` (iproute2). Only sockets owned by this user
/// carry a `users:` section; the rest cannot be attributed and are dropped.
pub(crate) fn tcp_listeners_via_ss() -> Result<Vec<Listener>, String> {
    run("ss", &["-ltnp"]).map(|out| parse_ss_listeners(&out))
}

/// LISTEN sockets from `lsof -nP -iTCP -sTCP:LISTEN` (macOS and the Linux
/// fallback when iproute2 is absent). `-Fpcn` gives a machine-readable record
/// stream instead of the column layout.
pub(crate) fn tcp_listeners_via_lsof() -> Result<Vec<Listener>, String> {
    run("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpcn"])
        .map(|out| parse_lsof_field_listeners(&out))
}

/// Every LISTEN socket on the host: `ss` first on Linux (no extra package),
/// `lsof` otherwise. Both tools missing is a real error — the stop sweep must
/// say it cannot see anything instead of silently sparing orphans.
pub(crate) fn tcp_listeners(prefer_ss: bool) -> Result<Vec<Listener>, String> {
    let mut first_err = String::new();
    if prefer_ss {
        match tcp_listeners_via_ss() {
            Ok(v) => return Ok(v),
            Err(e) => first_err = e,
        }
    }
    match tcp_listeners_via_lsof() {
        Ok(v) => Ok(v),
        Err(e) if first_err.is_empty() => Err(e),
        Err(e) => Err(format!("{first_err}; {e}")),
    }
}

/// PIDs listening on one port, via `lsof -ti`.
pub(crate) fn port_pids(port: u64) -> Result<Vec<u32>, String> {
    let spec = format!("tcp:{port}");
    run("lsof", &["-ti", &spec]).map(|out| parse_lsof_pids(&out))
}

/// Processes whose command line contains `marker`, via `pgrep -af`.
/// `pgrep` exits 1 when nothing matches — that is an empty result, not a
/// failure (an error here would log noise after every clean stop).
pub(crate) fn scan_processes(marker: &str) -> Result<Vec<ProcessInfo>, String> {
    let o = crate::base::silent_command("pgrep")
        .args(["-af", marker])
        .output()
        .map_err(|e| format!("pgrep: {e}"))?;
    match o.status.code() {
        Some(0) => Ok(parse_pgrep(&String::from_utf8_lossy(&o.stdout))),
        Some(1) => Ok(Vec::new()),
        code => {
            let err: String = String::from_utf8_lossy(&o.stderr)
                .chars()
                .take(200)
                .collect();
            Err(format!(
                "process scan failed (exit {}): {err}",
                code.unwrap_or(-1)
            ))
        }
    }
}

/// Absolute path of `tool` on PATH, if any. `which` is present on every
/// supported Unix (it is a shell builtin on some, but the binary ships).
pub(crate) fn which_path(tool: &str) -> Option<String> {
    let out = run("which", &[tool]).ok()?;
    out.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

/// PID owning `port`: `lsof -ti` first (cheap), then a full LISTEN scan. The
/// scan fallback is what makes minimal distros work — Fedora and many Debian
/// installs ship without `lsof`.
pub(crate) fn port_pid(port: u64, prefer_ss: bool) -> Option<u32> {
    if let Ok(pids) = port_pids(port) {
        if let Some(pid) = pids.first() {
            return Some(*pid);
        }
    }
    tcp_listeners(prefer_ss)
        .ok()?
        .into_iter()
        .find(|l| l.port == port)
        .map(|l| l.pid)
}

/// Shared `PortOwner` construction: PID → signature → "is this ours?".
pub(crate) fn port_owner(
    port: u64,
    prefer_ss: bool,
    signature: impl Fn(u32) -> Option<(String, String)>,
) -> Option<PortOwner> {
    let pid = port_pid(port, prefer_ss)?;
    let (exe, cmd) = signature(pid).unwrap_or_default();
    let name = exe.rsplit('/').next().unwrap_or("").to_string();
    let ours = cmd.to_lowercase().contains("wgp.py");
    Some(PortOwner {
        pid,
        name,
        cmd,
        ours,
    })
}

/// SIGKILL by PID. Every Unix kill in this project goes through here so the
/// signal and the error policy cannot drift between adapters.
pub(crate) fn kill_pid(pid: u32) {
    let _ = crate::base::silent_command("kill")
        .arg("-9")
        .arg(pid.to_string())
        .output();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsof_pids_skips_noise() {
        assert_eq!(parse_lsof_pids("4242\n4243\n"), vec![4242, 4243]);
        assert_eq!(parse_lsof_pids(""), Vec::<u32>::new());
        assert_eq!(
            parse_lsof_pids("lsof: WARNING: can't stat()\n77\n"),
            vec![77]
        );
        assert_eq!(parse_lsof_pids("0\n"), Vec::<u32>::new());
    }

    #[test]
    fn port_of_addr_covers_socket_shapes() {
        assert_eq!(port_of_addr("0.0.0.0:7860"), Some(7860));
        assert_eq!(port_of_addr("*:7860"), Some(7860));
        assert_eq!(port_of_addr("[::]:7860"), Some(7860));
        assert_eq!(port_of_addr("[::1]:7861"), Some(7861));
        assert_eq!(port_of_addr("localhost:8080"), Some(8080));
        assert_eq!(port_of_addr("localhost"), None);
        assert_eq!(port_of_addr("0.0.0.0:http"), None);
        assert_eq!(port_of_addr(""), None);
    }

    #[test]
    fn ss_rows_become_listeners() {
        let out = "\
State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process
LISTEN 0      4096   0.0.0.0:7860       0.0.0.0:*         users:((\"python3.12\",pid=4242,fd=7))
LISTEN 0      4096   [::]:7860            [::]:*            users:((\"python3.12\",pid=4242,fd=8))
LISTEN 0      128    127.0.0.1:631        0.0.0.0:*
ESTAB  0      0      10.0.0.2:22          10.0.0.9:51000
";
        assert_eq!(
            parse_ss_listeners(out),
            vec![
                Listener {
                    pid: 4242,
                    name: "python3.12".into(),
                    port: 7860
                },
                Listener {
                    pid: 4242,
                    name: "python3.12".into(),
                    port: 7860
                },
            ]
        );
    }

    /// A LISTEN row without process info is another user's socket (ss strips
    /// the users: section). It must produce nothing — never a guessed PID.
    #[test]
    fn ss_row_without_users_is_skipped() {
        let out = "LISTEN 0 128 127.0.0.1:631 0.0.0.0:*\n";
        assert!(parse_ss_listeners(out).is_empty());
    }

    #[test]
    fn lsof_fields_become_listeners() {
        let out = "\
p4242
cpython3.12
n*:7860
n127.0.0.1:7861
p900
cnode
n[::1]:5173 (LISTEN)
pjunk
cbroken
n*:7862
";
        assert_eq!(
            parse_lsof_field_listeners(out),
            vec![
                Listener {
                    pid: 4242,
                    name: "python3.12".into(),
                    port: 7860
                },
                Listener {
                    pid: 4242,
                    name: "python3.12".into(),
                    port: 7861
                },
                Listener {
                    pid: 900,
                    name: "node".into(),
                    port: 5173
                },
            ]
        );
    }

    #[test]
    fn lsof_record_without_pid_is_ignored() {
        // `c`/`n` before any `p` line (header-ish garbage) must not invent PID 0.
        assert!(parse_lsof_field_listeners("cnode\nn*:7860\n").is_empty());
    }

    #[test]
    fn pgrep_lines_become_process_info() {
        let out = "1234 /usr/bin/python3.12 /home/u/Wan2GP/wgp.py --server\n91 /opt/uv/python3.11 wgp.py\n";
        assert_eq!(
            parse_pgrep(out),
            vec![
                ProcessInfo {
                    pid: 1234,
                    exe: String::new(),
                    cmd: "/usr/bin/python3.12 /home/u/Wan2GP/wgp.py --server".into()
                },
                ProcessInfo {
                    pid: 91,
                    exe: String::new(),
                    cmd: "/opt/uv/python3.11 wgp.py".into()
                },
            ]
        );
        // A line whose first token is not a PID (tool warning, blank line) is
        // skipped rather than turned into a process record.
        assert!(parse_pgrep("pgrep: no matching processes\n\n").is_empty());
    }
}
