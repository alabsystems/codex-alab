// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! What codex-alab keeps apart from `codex-old` in the `CODEX_HOME` they share (`ALAB(home)`).
//!
//! `codex-old`, OpenAI's own build, runs beside codex-alab on the same `~/.codex`: one login,
//! one config, one history. Most of that is meant to be shared. A few files are each build's
//! own and get rewritten whenever the other build wrote them last, so each launch of one after
//! the other undid the other's work. Each such name here is upstream's for every binary but
//! codex (the switch is off) and codex-alab's own for codex.

use std::io::Write as _;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// codex-alab's system skills folder under `CODEX_HOME/skills`.
///
/// Upstream lays the skills its binary embeds into `skills/.system` and clears that folder
/// whenever its marker names another set. codex-alab embeds a smaller set (`alab/removed-paths`
/// drops OpenAI's image generation, API docs and GitHub skill installer), so each launch after
/// the other build deleted and re-laid the other's set (about 40 files on 2026-10-23), and one
/// running beside the other could lose a skill it had already listed. The folder is hidden like
/// `.system`, so the user-skills scan of `CODEX_HOME/skills` passes over it (skill discovery
/// skips hidden folders below a root), and every reader (the install, the skill roots, the
/// watcher, the review skill) takes it from `codex_skills::system_cache_root_dir`, where the one
/// hook is.
pub const SYSTEM_SKILLS_DIR: &str = ".alab-system";

/// The system skills folder name: [`SYSTEM_SKILLS_DIR`] for codex, `upstream` otherwise.
pub fn system_skills_dir(upstream: &'static str) -> &'static str {
    own(upstream, SYSTEM_SKILLS_DIR)
}

/// `codex-old`'s system skills folder under `CODEX_HOME/skills` (upstream's
/// `codex_skills::SYSTEM_SKILLS_DIR_NAME`, which that crate keeps private).
pub const UPSTREAM_SYSTEM_SKILLS_DIR: &str = ".system";

/// A skill rule's path as codex matches it against the skills it loaded.
///
/// `[[skills.config]]` rules in the shared `config.toml` turn a skill on or off by its
/// `SKILL.md` path, and both builds write the one under `skills/.system` (see
/// [`shared_skill_rule_path`]). codex loads that skill from `skills/.alab-system`, so for codex a
/// path into `skills/.system/` names the same relative path under `skills/.alab-system/`; every
/// other path, and every path for any binary but codex, is returned as it is.
pub fn own_skill_rule_path(path: &Path) -> PathBuf {
    swap_system_skills_dir(path, UPSTREAM_SYSTEM_SKILLS_DIR, SYSTEM_SKILLS_DIR)
}

/// A skill rule's path as codex writes it into the shared `config.toml`.
///
/// For codex a path under `skills/.alab-system/` is written as the same relative path under
/// `skills/.system/`, the one `codex-old` loads, so a skill turned off in one build stays off in
/// the other ([`own_skill_rule_path`] reads it back). Every other path, and every path for any
/// binary but codex, is returned as it is.
pub fn shared_skill_rule_path(path: &Path) -> PathBuf {
    swap_system_skills_dir(path, SYSTEM_SKILLS_DIR, UPSTREAM_SYSTEM_SKILLS_DIR)
}

/// Whether `path` lies in the other build's system skills folder, the sibling of `own_root`
/// (codex's `skills/.alab-system`) named `skills/.system`. Always false for any binary but codex.
///
/// The app-server watches `CODEX_HOME/skills` for skill edits and passes over its own system
/// folder, which only its install writes. codex never loads `skills/.system` either (skill
/// discovery skips hidden folders), so `codex-old`'s reinstall of its set there is no change to
/// codex's skills and must not clear its skills cache and reload every client's list.
pub fn in_other_system_skills(own_root: &Path, path: &Path) -> bool {
    crate::active()
        && own_root
            .parent()
            .is_some_and(|skills| path.starts_with(skills.join(UPSTREAM_SYSTEM_SKILLS_DIR)))
}

/// `path` with its last `skills/<from>` pair of components renamed `skills/<to>` when the switch
/// is on; `path` unchanged otherwise.
fn swap_system_skills_dir(path: &Path, from: &str, to: &str) -> PathBuf {
    if !crate::active() {
        return path.to_path_buf();
    }
    let components: Vec<Component<'_>> = path.components().collect();
    let Some(at) = components
        .windows(2)
        .rposition(|pair| pair[0].as_os_str() == "skills" && pair[1].as_os_str() == from)
    else {
        return path.to_path_buf();
    };
    let mut swapped = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        if index == at + 1 {
            swapped.push(to);
        } else {
            swapped.push(component.as_os_str());
        }
    }
    swapped
}

/// codex-alab's model catalog cache under `CODEX_HOME`.
///
/// Upstream keeps the catalog it fetches from OpenAI in `models_cache.json` and trusts it only
/// for the client version that wrote it. codex-alab reports its upstream base's next release
/// (`0.165.0` on 2026-10-23) and `codex-old` its own (`0.164.0`), so a launch after the other
/// build always missed the cache, fetched the catalog again and rewrote the file (464 KB). The
/// per-originator caches take their names from this one (`models_cache.alab.<originator>.json`).
pub const MODELS_CACHE_FILE: &str = "models_cache.alab.json";

/// The model catalog cache's file name: [`MODELS_CACHE_FILE`] for codex, `upstream` otherwise.
pub fn models_cache_file(upstream: &'static str) -> &'static str {
    own(upstream, MODELS_CACHE_FILE)
}

/// Environment variable naming an account's `auth.json` file outside the shared `CODEX_HOME`.
///
/// Upstream keeps `auth.json` directly under `CODEX_HOME`, forcing an account pool to build a
/// separate `CODEX_HOME` symlink overlay per login. Naming the auth file alone leaves `CODEX_HOME`
/// (`~/.codex`: sessions, rollouts, config, skills and session records) shared across accounts.
pub const AUTH_PATH_ENV: &str = "CODEX_AUTH_PATH";

/// Admitted override path from [`AUTH_PATH_ENV`] when `active`: a non-empty absolute file path
/// with no trailing directory or `.`/`..` segment, no control or line-separator characters, and
/// not naming an existing directory.
pub fn admitted_auth_path_of(active: bool, value: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if !active {
        return None;
    }
    let raw = value?.to_str()?;
    let last_seg = raw
        .rsplit(['/', std::path::MAIN_SEPARATOR])
        .next()
        .unwrap_or("");
    if raw.is_empty()
        || matches!(last_seg, "" | "." | "..")
        || raw
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return None;
    }
    let path = PathBuf::from(raw);
    (path.is_absolute() && path.file_name().is_some() && !path.is_dir()).then_some(path)
}

/// Active `auth.json` file for `codex_home`: [`AUTH_PATH_ENV`] when [`crate::is_alab_binary`]
/// admits it, else `codex_home/auth.json`.
pub fn auth_file(codex_home: &Path) -> PathBuf {
    admitted_auth_path_of(
        crate::is_alab_binary(),
        std::env::var_os(AUTH_PATH_ENV).as_deref(),
    )
    .unwrap_or_else(|| codex_home.join("auth.json"))
}

/// Keyring/ephemeral store-key target for `codex_home`: the admitted [`AUTH_PATH_ENV`] path
/// (with its nearest existing ancestor canonicalized so pre-creation and post-creation keys match
/// across macOS `/tmp` and `/var` symlinks) when set, else `codex_home` unchanged so default
/// keyring keys stay identical.
pub fn store_key_target(codex_home: &Path) -> PathBuf {
    let Some(override_path) = admitted_auth_path_of(
        crate::is_alab_binary(),
        std::env::var_os(AUTH_PATH_ENV).as_deref(),
    ) else {
        return codex_home.to_path_buf();
    };
    canonicalize_existing_prefix(&override_path)
}

fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    if let Ok(canon) = path.canonicalize() {
        return canon;
    }
    let mut tail = Vec::new();
    let mut cur = path;
    while let Some(name) = cur.file_name() {
        tail.push(name.to_os_string());
        let Some(parent) = cur.parent() else {
            break;
        };
        if let Ok(mut base) = parent.canonicalize() {
            for seg in tail.into_iter().rev() {
                base.push(seg);
            }
            return base;
        }
        cur = parent;
    }
    path.to_path_buf()
}

/// Whether the running process holds an admitted [`AUTH_PATH_ENV`] override when
/// [`crate::is_alab_binary`].
///
/// An explicit [`AUTH_PATH_ENV`] names a file-backed credential slot outside the shared
/// `CODEX_HOME`, so persistent credential modes (`Keyring` and `Auto`) stay on that file rather
/// than consulting `CODEX_HOME`'s shared `SecretsManager` or unlinking `CODEX_AUTH_PATH` into the
/// OS keyring on token refresh.
pub fn has_admitted_auth_path() -> bool {
    admitted_auth_path_of(
        crate::is_alab_binary(),
        std::env::var_os(AUTH_PATH_ENV).as_deref(),
    )
    .is_some()
}

/// Whether `lookup` names an existing admitted [`AUTH_PATH_ENV`] file when `active`.
pub fn has_auth_override_with(
    active: bool,
    lookup: impl FnOnce(&str) -> Option<std::ffi::OsString>,
) -> bool {
    admitted_auth_path_of(active, lookup(AUTH_PATH_ENV).as_deref())
        .is_some_and(|path| !matches!(path.try_exists(), Ok(false)))
}

fn auth_parent(auth_file: &Path) -> &Path {
    auth_file
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn prepare_auth_parent(auth_file: &Path) -> std::io::Result<&Path> {
    match std::fs::symlink_metadata(auth_file) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "refusing to overwrite symlinked auth.json",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let parent = auth_parent(auth_file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(parent)?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(parent)?;
    Ok(parent)
}

/// The file store's directory for the upstream missing-directory delete guard. An admitted
/// account slot can exist even when shared `CODEX_HOME` does not. Other binaries keep their
/// original home, and no directory is created by this read.
pub fn auth_commit_home(codex_home: &Path) -> PathBuf {
    if crate::is_alab_binary() {
        auth_parent(&auth_file(codex_home)).to_path_buf()
    } else {
        codex_home.to_path_buf()
    }
}

/// The commit sidecar for this exact slot name, independent of its extension. The pool's
/// outer refresh lock remains `<slot>.lock`; commits nest inside that hold and must never
/// reopen the same flock through another file description.
pub fn auth_commit_lock_path(auth_file: &Path) -> PathBuf {
    let mut name = auth_file
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_else(|| "auth.json".into());
    name.push(".commit.lock");
    auth_file.with_file_name(name)
}

/// Lock this ALab file slot's stable commit sidecar before a cooperating credential commit. The
/// original slot never follows a final symlink; its newly created parent remains private.
/// The returned file holds the lock until dropped. This preserves upstream's five-second
/// product timeout without holding the lock across any network request.
pub fn lock_auth_file(auth_file: &Path) -> std::io::Result<std::fs::File> {
    use std::fs::TryLockError;
    use std::time::Duration;
    use std::time::Instant;

    prepare_auth_parent(auth_file)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(auth_commit_lock_path(auth_file))?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "credential commit lock is not a regular file",
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(TryLockError::WouldBlock) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "credential commit lock timed out",
                ));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn replaceable_auth_file(auth_file: &Path) -> std::io::Result<()> {
    let metadata = match std::fs::symlink_metadata(auth_file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() || metadata.permissions().readonly() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "credential file is not a writable regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if metadata.nlink() > 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "atomic credential replacement does not support hard-linked files",
            ));
        }
    }
    // Like upstream, respect the destination's ACL/write permission as well as its parent.
    // No-follow and nonblocking flags also refuse a raced symlink or special-file wait.
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(auth_file)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "credential file changed to a non-regular file",
        ));
    }
    Ok(())
}

/// Atomically writes `bytes` to `auth_file` (`0600` on Unix) via a sibling temporary file so
/// concurrent readers (such as aterm's native usage poller) never observe a truncated file and a
/// symlink at `auth_file` is never followed. Read-only and hard-linked files are refused,
/// preserving upstream's replacement guards rather than broadening their permissions.
pub fn save_auth_bytes(auth_file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT_TMP: AtomicU64 = AtomicU64::new(0);

    let parent = prepare_auth_parent(auth_file)?;
    replaceable_auth_file(auth_file)?;
    let file_name = auth_file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("auth.json");
    let seq = NEXT_TMP.fetch_add(1, Ordering::Relaxed);
    let tmp_path = parent.join(format!(
        ".{file_name}.alab-tmp.{}.{seq}",
        std::process::id()
    ));
    let write_tmp = || -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp_path)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        replaceable_auth_file(auth_file)?;
        std::fs::rename(&tmp_path, auth_file)
    };
    let result = write_tmp();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result
}

fn own(upstream: &'static str, alab: &'static str) -> &'static str {
    if crate::active() { alab } else { upstream }
}
