// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Process identity: copied from upstream's daemon pidfile
//! (`app-server-daemon/src/backend/pid_identity.rs`, `pub(super)` there), made synchronous.
//! A record is live only while its pid exists, is not a zombie, and still has the identity
//! the record carries; a reused pid, or a pid from another boot, does not match. `execve`
//! keeps every field, so a re-exec is the same process.

use codex_alab_core::session::ProcessIdentity;
use std::io;
use std::io::Error;
use std::io::ErrorKind;

/// This process's identity.
pub fn current() -> io::Result<ProcessIdentity> {
    of_pid(std::process::id())
}

/// `pid`'s identity now (`NotFound` once it is gone; a zombie counts as gone).
pub fn of_pid(pid: u32) -> io::Result<ProcessIdentity> {
    let (zombie, identity) = read_process_details(pid)?;
    if zombie {
        return Err(Error::new(ErrorKind::NotFound, "process is a zombie"));
    }
    Ok(identity)
}

/// Whether `pid` is alive and is the process `recorded` names. With no recorded identity,
/// only the pid's existence counts (a reader cannot do better and must not delete it).
pub fn is_live(pid: u32, recorded: Option<&ProcessIdentity>) -> bool {
    match (of_pid(pid), recorded) {
        (Ok(actual), Some(recorded)) => actual == *recorded,
        (Ok(_), None) => true,
        (Err(err), _) => err.kind() == ErrorKind::PermissionDenied,
    }
}

/// When `pid` started, as `ps -o lstart` prints it (local time, C locale).
pub fn proc_start(identity: &ProcessIdentity) -> Option<String> {
    let seconds = match identity {
        ProcessIdentity::MacOs { start_seconds, .. } => *start_seconds,
        ProcessIdentity::Linux { start_ticks, .. } => linux_start_seconds(*start_ticks)?,
    };
    lstart(i64::try_from(seconds).ok()?)
}

/// Native account snapshots use the kernel birth in UTC, with canonical whitespace.
pub fn proc_start_utc(identity: &ProcessIdentity) -> Option<String> {
    let seconds = match identity {
        ProcessIdentity::MacOs { start_seconds, .. } => *start_seconds,
        ProcessIdentity::Linux { start_ticks, .. } => linux_start_seconds(*start_ticks)?,
    };
    let time = chrono::DateTime::from_timestamp(i64::try_from(seconds).ok()?, 0)?;
    Some(
        time.format("%a %b %e %H:%M:%S %Y")
            .to_string()
            .split_ascii_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn lstart(seconds: i64) -> Option<String> {
    let time = libc::time_t::try_from(seconds).ok()?;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: both pointers are valid for the call; localtime_r fills `tm` on success.
    if unsafe { libc::localtime_r(&time, tm.as_mut_ptr()) }.is_null() {
        return None;
    }
    // SAFETY: localtime_r succeeded, so `tm` is initialized.
    let tm = unsafe { tm.assume_init() };
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    Some(format!(
        "{} {} {:>2} {:02}:{:02}:{:02} {}",
        DAYS.get(usize::try_from(tm.tm_wday).ok()?)?,
        MONTHS.get(usize::try_from(tm.tm_mon).ok()?)?,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        i64::from(tm.tm_year) + 1900
    ))
}

#[cfg(test)]
#[test]
fn account_birth_uses_canonical_utc_single_digit_day() {
    let identity = ProcessIdentity::MacOs {
        boot_id: String::new(),
        unique_id: 1,
        start_seconds: 0,
        start_microseconds: 0,
    };
    assert_eq!(
        proc_start_utc(&identity).as_deref(),
        Some("Thu Jan 1 00:00:00 1970")
    );
}

#[cfg(target_os = "macos")]
fn read_process_details(pid: u32) -> io::Result<(bool, ProcessIdentity)> {
    let boot_id = read_boot_id()?;
    let unique_id = read_unique_id(pid)?;
    let pid = libc::pid_t::try_from(pid).map_err(|_| Error::other("pid is out of range"))?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer has the exact size and alignment required by this flavor.
    // arg=1 includes zombies so a reader can tell an exited process from a live one.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            /*arg*/ 1,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if read <= 0 {
        return Err(not_found_or_last_error());
    }
    if read != size {
        return Err(Error::other("incomplete process identity"));
    }
    // SAFETY: proc_pidinfo initialized the entire structure on success.
    let info = unsafe { info.assume_init() };
    if unique_id != read_unique_id(info.pbi_pid)? {
        return Err(Error::other("process changed while reading its identity"));
    }
    Ok((
        info.pbi_status == libc::SZOMB,
        ProcessIdentity::MacOs {
            boot_id,
            unique_id,
            start_seconds: info.pbi_start_tvsec,
            start_microseconds: info.pbi_start_tvusec,
        },
    ))
}

#[cfg(target_os = "macos")]
fn read_unique_id(pid: u32) -> io::Result<u64> {
    // XNU's proc_uniqidentifierinfo ABI (flavor 17), unchanged in size since 2013.
    // Unlike PROC_PIDTBSDINFO, this query does not require matching process ownership.
    #[repr(C)]
    struct UniqueInfo {
        executable_uuid: [u8; 16],
        unique_id: u64,
        parent_unique_id: u64,
        reserved: [u64; 3],
    }
    let pid = libc::pid_t::try_from(pid).map_err(|_| Error::other("pid is out of range"))?;
    let mut info = std::mem::MaybeUninit::<UniqueInfo>::uninit();
    let size = std::mem::size_of::<UniqueInfo>() as libc::c_int;
    // SAFETY: the buffer matches proc_uniqidentifierinfo's size and alignment.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            /*flavor*/ 17,
            /*arg*/ 1,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if read <= 0 {
        return Err(not_found_or_last_error());
    }
    if read != size {
        return Err(Error::other("incomplete unique ID"));
    }
    // SAFETY: proc_pidinfo initialized the entire structure on success.
    Ok(unsafe { info.assume_init() }.unique_id)
}

/// proc_pidinfo reports a missing pid as ESRCH (or as 0 bytes with errno unset).
#[cfg(target_os = "macos")]
fn not_found_or_last_error() -> Error {
    let err = Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::ESRCH) | Some(0) | None => Error::new(ErrorKind::NotFound, "no such process"),
        _ => err,
    }
}

#[cfg(target_os = "macos")]
fn read_boot_id() -> io::Result<String> {
    let mut buffer = [0u8; 37];
    let mut size = buffer.len();
    // SAFETY: the writable buffer has size bytes; the new-value pointer is null.
    let result = unsafe {
        libc::sysctlbyname(
            c"kern.bootsessionuuid".as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            /*newlen*/ 0,
        )
    };
    if result != 0 {
        return Err(Error::last_os_error());
    }
    if size != buffer.len() {
        return Err(Error::other("incomplete macOS boot ID"));
    }
    std::ffi::CStr::from_bytes_with_nul(&buffer)
        .map_err(Error::other)?
        .to_str()
        .map(str::to_owned)
        .map_err(Error::other)
}

#[cfg(target_os = "macos")]
fn linux_start_seconds(_start_ticks: u64) -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn read_process_details(pid: u32) -> io::Result<(bool, ProcessIdentity)> {
    let stat = std::fs::read(format!("/proc/{pid}/stat"))?;
    let (state, start_ticks) = parse_stat(&stat)?;
    Ok((
        state == "Z",
        ProcessIdentity::Linux {
            boot_id: read_boot_id()?,
            start_ticks,
        },
    ))
}

#[cfg(target_os = "linux")]
fn read_boot_id() -> io::Result<String> {
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    let boot_id = boot_id.trim();
    if boot_id.is_empty() {
        return Err(Error::other("Linux boot ID is empty"));
    }
    Ok(boot_id.to_owned())
}

/// Boot time (`btime` in /proc/stat) plus the start ticks at `_SC_CLK_TCK`.
#[cfg(target_os = "linux")]
fn linux_start_seconds(start_ticks: u64) -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let btime: u64 = stat
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    // SAFETY: sysconf has no preconditions.
    let ticks = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
    (ticks > 0).then(|| btime + start_ticks / ticks)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_process_details(_pid: u32) -> io::Result<(bool, ProcessIdentity)> {
    Err(Error::new(
        ErrorKind::Unsupported,
        "process identity is read on macOS and Linux only",
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn linux_start_seconds(_start_ticks: u64) -> Option<u64> {
    None
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn parse_stat(stat: &[u8]) -> io::Result<(String, u64)> {
    let invalid = |what: &str| Error::new(ErrorKind::InvalidData, format!("process stat {what}"));
    // comm (field 2) can contain spaces and closing parentheses. The final ')'
    // terminates it; splitting the whole line on whitespace miscounts fields.
    let end = stat
        .iter()
        .rposition(|byte| *byte == b')')
        .ok_or_else(|| invalid("has no comm"))?;
    let fields = std::str::from_utf8(&stat[end + 1..]).map_err(|_| invalid("is not UTF-8"))?;
    let mut fields = fields.split_whitespace();
    let state = fields.next().ok_or_else(|| invalid("has no state"))?;
    let start_ticks = fields
        .nth(/*n*/ 18)
        .ok_or_else(|| invalid("has no start time"))?
        .parse()
        .map_err(|_| invalid("start time is invalid"))?;
    Ok((state.to_string(), start_ticks))
}
