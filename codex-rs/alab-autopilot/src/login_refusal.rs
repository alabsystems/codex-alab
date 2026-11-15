// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! CODEX-ALAB'S OWN REFUSAL OF A LOGIN, KEPT AS A VETO (the lost-login move,
//! `alab/design/accounts-lost-login.md`). A session whose login is lost moves to another
//! signed-in account only when the token simply ended (`refresh_token_expired`, settled on
//! aterm's record). When codex's own refresh of the token was refused for any other reason
//! (reused, invalidated, `invalid_grant`, an unknown code), that answer says the move must not
//! happen, and it must still say so after a restart: aterm may present the same token again
//! and the issuer may answer it differently, and codex sends nothing once aterm's refresh
//! journal names the login, so the new process would never hear the first answer again.
//!
//! So that answer is kept here, one file per refused refresh token under
//! `<CODEX_HOME>/alab/login-refusals/`, named by a domain-separated SHA-256 of the token
//! ([`Refusals::name`]); the file holds the reason word only. No token byte is kept, written
//! or said. A file is only ever a veto: it is written once, never for `expired`, never
//! changed, and taken away only by the 30-day prune each write runs ([`KEPT`]). Every session
//! that shares the `CODEX_HOME` reads the same folder; since a file names one token and only
//! vetoes, they need no lock. aterm never reads it.

use std::io;
use std::io::Read as _;
use std::io::Write as _;
use std::os::unix::fs::DirBuilderExt as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use codex_protocol::auth::RefreshTokenFailedReason;
use sha2::Digest as _;
use sha2::Sha256;

/// The digest's domain: a refresh token's name here is never the hash another protocol uses.
const DOMAIN: &[u8] = b"alab/login-refusal/v1\0";

/// How long a veto stands: a write removes the ones older than this.
pub(crate) const KEPT: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// The most a veto file is read: its word and a newline.
const MOST: u64 = 64;

/// The vetoes under one `CODEX_HOME`.
pub(crate) struct Refusals {
    dir: PathBuf,
}

/// The word a refusal is kept as; `None` for `expired`, which is never kept.
fn word(reason: RefreshTokenFailedReason) -> Option<&'static str> {
    match reason {
        RefreshTokenFailedReason::Expired => None,
        RefreshTokenFailedReason::Exhausted => Some("exhausted"),
        RefreshTokenFailedReason::Revoked => Some("revoked"),
        RefreshTokenFailedReason::Other => Some("other"),
    }
}

/// The reason a kept word names. A file is a veto whatever it says, so a word this reader
/// does not know (or a file it cannot read) is `Other`, never `Expired`.
fn of_word(word: &str) -> RefreshTokenFailedReason {
    match word.trim() {
        "exhausted" => RefreshTokenFailedReason::Exhausted,
        "revoked" => RefreshTokenFailedReason::Revoked,
        _ => RefreshTokenFailedReason::Other,
    }
}

/// Whether `name` is a veto's (64 lowercase hex digits) or a write's temporary file.
fn ours(name: &str) -> bool {
    (name.len() == 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        || (name.starts_with('.') && name.ends_with(".tmp"))
}

impl Refusals {
    /// The vetoes kept under `codex_home` (`<CODEX_HOME>/alab/login-refusals/`, beside ALab's
    /// session records).
    pub(crate) fn under(codex_home: &Path) -> Refusals {
        Refusals {
            dir: codex_home.join("alab").join("login-refusals"),
        }
    }

    /// The folder the vetoes are kept in.
    #[cfg(test)]
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    /// The name `refresh_token`'s veto is kept under: the hex SHA-256 of the domain and the
    /// token. It says nothing of the token.
    pub(crate) fn name(refresh_token: &str) -> String {
        let mut hash = Sha256::new();
        hash.update(DOMAIN);
        hash.update(refresh_token.as_bytes());
        format!("{:x}", hash.finalize())
    }

    /// The refusal kept for `refresh_token`, if one is: a file that cannot be read is a veto
    /// too, as it stands there. An empty token has none.
    pub(crate) fn refused(&self, refresh_token: &str) -> Option<RefreshTokenFailedReason> {
        if refresh_token.is_empty() {
            return None;
        }
        let path = self.dir.join(Self::name(refresh_token));
        match std::fs::File::open(&path) {
            Ok(file) => {
                let mut text = String::new();
                match file.take(MOST).read_to_string(&mut text) {
                    Ok(_) => Some(of_word(&text)),
                    Err(_) => Some(RefreshTokenFailedReason::Other),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(_) => Some(RefreshTokenFailedReason::Other),
        }
    }

    /// KEEP CODEX'S OWN REFUSAL OF `refresh_token` FOR `reason`: whether this call wrote its
    /// veto. `expired` is never kept (it is the one ordinary end, and aterm's record decides
    /// it), nor is an empty token. The first refusal kept stands: a second write for the same
    /// token leaves it as it is (create-new: the file is linked into place only where none
    /// is). Each write first removes the vetoes older than [`KEPT`].
    pub(crate) fn keep(
        &self,
        refresh_token: &str,
        reason: RefreshTokenFailedReason,
    ) -> io::Result<bool> {
        self.keep_at(refresh_token, reason, SystemTime::now())
    }

    fn keep_at(
        &self,
        refresh_token: &str,
        reason: RefreshTokenFailedReason,
        now: SystemTime,
    ) -> io::Result<bool> {
        let Some(word) = word(reason) else {
            return Ok(false);
        };
        if refresh_token.is_empty() {
            return Ok(false);
        }
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        self.prune(now);
        let name = Self::name(refresh_token);
        let target = self.dir.join(&name);
        if std::fs::symlink_metadata(&target).is_ok() {
            return Ok(false);
        }
        let nanos = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let temporary = self
            .dir
            .join(format!(".{name}.{}.{nanos}.tmp", std::process::id()));
        let written = link_new(&temporary, &target, word);
        let _ = std::fs::remove_file(&temporary);
        if written.as_ref().is_ok_and(|written| *written)
            && let Ok(dir) = std::fs::File::open(&self.dir)
        {
            let _ = dir.sync_all();
        }
        written
    }

    /// Remove the vetoes (and a write's leftovers) last written before `now` less [`KEPT`].
    /// A file whose time cannot be read is left: it can only veto.
    fn prune(&self, now: SystemTime) {
        let Some(before) = now.checked_sub(KEPT) else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name.to_str().is_some_and(ours) {
                continue;
            }
            let old = entry
                .metadata()
                .ok()
                .filter(std::fs::Metadata::is_file)
                .and_then(|metadata| metadata.modified().ok())
                .is_some_and(|modified| modified < before);
            if old {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Write `word` to the new file `temporary`, then link it into place at `target` only where
/// nothing stands there: whether it was linked.
fn link_new(temporary: &Path, target: &Path, word: &str) -> io::Result<bool> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(temporary)?;
    file.write_all(word.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    match std::fs::hard_link(temporary, target) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "login_refusal_tests.rs"]
mod tests;
