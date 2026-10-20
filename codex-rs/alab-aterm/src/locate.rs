// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Finding the aterm instance that hosts this terminal, as `aterm ctl` does for `@self`
//! (`crates/aterm-ctl/src/lib.rs` `self_instance_sock_in`, `read_token_file_at`).
//!
//! - The session is `$ATERM_PARENT_SESSION_ID` (never `ATERM_SESSION_ID`, a nested
//!   aterm's preminted identity), shaped `s-<hex>`.
//! - The rendezvous dir is `$XDG_RUNTIME_DIR/aterm`, else `$HOME/Library/Application
//!   Support/aterm` (`aterm_uds::control_socket_dir`).
//! - `<dir>/graph/<sid>` holds `sock <abs path>` and `pid <n>` lines, written by the
//!   instance hosting that session.
//! - The token is beside the socket: `aterm-<pid>.sock` pairs with `aterm-<pid>.token`;
//!   any other socket name `<name>` with `<name>.token` (`aterm-sock-<name>.token` when the
//!   name does not end in `.sock`), then the legacy shared `aterm.token`
//!   (`aterm_types::control_socket::token_names_for_sock`). It is read after the connect,
//!   no-follow, a regular file of at most 256 bytes, trimmed, one line.
//!
//! There is no fallback to the `aterm.sock` alias: that names the newest instance, not
//! necessarily ours, and a session this process cannot place is no aterm at all.

use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

/// The environment variable naming the aterm session hosting this terminal.
pub const SELF_SID_ENV: &str = "ATERM_PARENT_SESSION_ID";

const MAX_TOKEN_FILE_BYTES: u64 = 256;

/// Where one aterm session is served: the instance socket and the session id that
/// `@self` stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The instance's control socket.
    pub sock: PathBuf,
    /// The session (`s-<hex>`) this process runs in.
    pub sid: String,
}

impl Endpoint {
    /// The endpoint of the session hosting this terminal, or `None` when this process is
    /// not codex-alab ([`codex_alab_core::active`]), is not inside an aterm session, or
    /// that session has no graph entry. Nothing is connected here.
    pub fn from_env() -> Option<Endpoint> {
        if !codex_alab_core::active() {
            return None;
        }
        let sid = std::env::var(SELF_SID_ENV).ok()?;
        locate_in(&control_socket_dir()?, &sid)
    }

    /// An endpoint named outright (a private instance's `--control-sock`, a test).
    pub fn explicit(sock: impl Into<PathBuf>, sid: impl Into<String>) -> Endpoint {
        Endpoint {
            sock: sock.into(),
            sid: sid.into(),
        }
    }
}

/// aterm's per-user rendezvous directory (`aterm_uds::control_socket_dir`, non-Windows).
pub fn control_socket_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR") {
        return Some(PathBuf::from(xdg).join("aterm"));
    }
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("aterm"),
    )
}

/// Whether `sid` has the server's shape, `s-<hex>`: anything else could walk out of the
/// graph directory.
pub fn valid_sid(sid: &str) -> bool {
    sid.strip_prefix("s-")
        .is_some_and(|hex| !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The endpoint of `sid` from its graph entry in `dir`, or `None`.
pub fn locate_in(dir: &Path, sid: &str) -> Option<Endpoint> {
    if !valid_sid(sid) {
        return None;
    }
    let body = std::fs::read_to_string(dir.join("graph").join(sid)).ok()?;
    let sock = graph_entry_sock(&body)?;
    Some(Endpoint {
        sock: PathBuf::from(sock),
        sid: sid.to_string(),
    })
}

/// The `sock <abs-path>` line of a graph entry (`aterm_types::control_socket::graph_entry_sock`).
pub fn graph_entry_sock(body: &str) -> Option<String> {
    body.lines()
        .find_map(|l| l.strip_prefix("sock "))
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
}

/// The pid in a per-instance name `aterm-<pid>.sock`, digits only.
fn instance_sock_pid(name: &str) -> Option<u32> {
    let pid = name.strip_suffix(".sock")?.strip_prefix("aterm-")?;
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    pid.parse().ok()
}

/// Every token file name a client may read for the socket named `sock_name`, in order
/// (`aterm_types::control_socket::token_names_for_sock`).
pub fn token_names_for_sock(sock_name: &str) -> Vec<String> {
    if let Some(pid) = instance_sock_pid(sock_name) {
        return vec![format!("aterm-{pid}.token")];
    }
    let own = if sock_name.ends_with(".sock") {
        format!("{sock_name}.token")
    } else {
        format!("aterm-sock-{sock_name}.token")
    };
    vec![own, "aterm.token".to_string()]
}

/// The token beside `sock`. A file that exists but is malformed (a symlink, not regular,
/// oversized, empty, several lines) is an error at once, never a reason to try the next
/// name.
pub fn read_token_beside(sock: &Path) -> std::io::Result<String> {
    use std::io::Error;
    use std::io::ErrorKind;
    let (Some(dir), Some(name)) = (sock.parent(), sock.file_name()) else {
        return Err(Error::new(
            ErrorKind::NotFound,
            "socket path names no directory",
        ));
    };
    // A `latest`-style symlink names its instance; its token is the instance's.
    let name = std::fs::read_link(sock)
        .ok()
        .and_then(|t| t.file_name().map(std::ffi::OsStr::to_os_string))
        .unwrap_or_else(|| name.to_os_string());
    let mut miss = None;
    for candidate in token_names_for_sock(&name.to_string_lossy()) {
        match read_token_file(&dir.join(candidate)) {
            Ok(token) => return Ok(token),
            Err(e) if matches!(e.kind(), ErrorKind::InvalidData | ErrorKind::InvalidInput) => {
                return Err(e);
            }
            Err(e) => {
                miss.get_or_insert(e);
            }
        }
    }
    Err(miss.unwrap_or_else(|| Error::new(ErrorKind::NotFound, "no token beside the socket")))
}

fn read_token_file(path: &Path) -> std::io::Result<String> {
    use std::io::Error;
    use std::io::ErrorKind;
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| {
            if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
                Error::new(ErrorKind::InvalidData, "token path is a symlink")
            } else {
                e
            }
        })?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "token path is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_TOKEN_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TOKEN_FILE_BYTES {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "token file exceeds 256 bytes",
        ));
    }
    let raw = String::from_utf8(bytes)
        .map_err(|_| Error::new(ErrorKind::InvalidData, "token file is not UTF-8"))?;
    let token = raw.trim();
    if token.is_empty() || token.contains(['\r', '\n', ' ']) {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "token file is not one token",
        ));
    }
    Ok(token.to_string())
}
