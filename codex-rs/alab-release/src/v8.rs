// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Resolve the source-pinned V8 archive and binding without executing upstream
//! packaging code. Agh downloads into private staging; both levels of SHA-256
//! pins must match before an atomic cache replacement. The caller retains the
//! cache lease until its native compiler supervisor has drained the group.

use crate::Release;
use crate::Result;
use sha2::Digest;
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

const MAX_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

struct Lease(File);

impl Drop for Lease {
    fn drop(&mut self) {
        // CLOEXEC does not prevent a forked child from retaining the open file
        // description before exec. Release the owner's lock before closing it.
        loop {
            // SAFETY: self owns a live, locked descriptor until this drop ends.
            if unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) } == 0
                || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                break;
            }
        }
    }
}

pub struct Artifacts {
    pub version: String,
    pub archive: PathBuf,
    pub binding: PathBuf,
    archive_sha256: String,
    binding_sha256: String,
    // Held until the caller has drained its native compiler group.
    _lease: Lease,
}

impl Artifacts {
    pub fn check(&self) -> Result<()> {
        if hash(&self.archive)? != self.archive_sha256
            || hash(&self.binding)? != self.binding_sha256
        {
            return Err("leased V8 inputs changed".into());
        }
        Ok(())
    }

    pub fn hashes(&self) -> (&str, &str) {
        (&self.archive_sha256, &self.binding_sha256)
    }
}

fn owned_directory(path: &Path) -> Result<()> {
    let info = std::fs::symlink_metadata(path)?;
    // SAFETY: geteuid has no preconditions and returns the process account.
    if !info.is_dir() || info.uid() != unsafe { libc::geteuid() } || info.mode() & 0o022 != 0 {
        return Err(
            "V8 cache requires an owned real directory without shared write permissions".into(),
        );
    }
    Ok(())
}

fn regular(path: &Path) -> Result<File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let info = file.metadata()?;
    if !info.is_file()
        || info.len() == 0
        || info.len() > MAX_ARTIFACT_BYTES
        || info.mode() & 0o022 != 0
    {
        return Err("V8 inputs must be nonempty protected regular files".into());
    }
    Ok(file)
}

fn hash(path: &Path) -> Result<String> {
    let mut file = regular(path)?;
    let before = file.metadata()?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let after = file.metadata()?;
    let identity = |info: &std::fs::Metadata| {
        (
            info.dev(),
            info.ino(),
            info.len(),
            info.mtime(),
            info.mtime_nsec(),
            info.ctime(),
            info.ctime_nsec(),
            info.mode(),
        )
    };
    if identity(&before) != identity(&after) {
        return Err("V8 input changed while hashing".into());
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn checksums(text: &str) -> Result<BTreeMap<String, String>> {
    if text.len() > 64 * 1024 {
        return Err("V8 checksum manifest is too large".into());
    }
    let mut entries = BTreeMap::new();
    for line in text.lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 2
            || parts[0].len() != 64
            || !parts[0]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || parts[1].starts_with('.')
            || parts[1].contains(['/', '\\'])
            || entries
                .insert(parts[1].to_owned(), parts[0].to_owned())
                .is_some()
        {
            return Err("V8 checksum manifest contains an invalid or duplicate entry".into());
        }
    }
    if entries.is_empty() {
        return Err("V8 checksum manifest is empty".into());
    }
    Ok(entries)
}

fn ensure(
    directory: &Path,
    name: &str,
    expected: &str,
    fetch: &mut impl FnMut(&str, &Path) -> Result<()>,
) -> Result<PathBuf> {
    let path = directory.join(name);
    match std::fs::symlink_metadata(&path) {
        Ok(info) => {
            if !info.is_file() {
                return Err("V8 cache contains a non-regular entry".into());
            }
            if info.len() > 0 && info.len() <= MAX_ARTIFACT_BYTES && hash(&path)? == expected {
                return Ok(path);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let staging = tempfile::Builder::new()
        .prefix(".v8-download-")
        .tempdir_in(directory)?;
    let temporary = staging.path().join(name);
    fetch(name, &temporary)?;
    if hash(&temporary)? != expected {
        return Err(format!("V8 download does not match its source pin: {name}").into());
    }
    File::open(&temporary)?.sync_all()?;
    std::fs::rename(&temporary, &path)?;
    File::open(directory)?.sync_all()?;
    Ok(path)
}

fn resolve_pair(
    version: &str,
    target: &str,
    pins: &str,
    cache: &Path,
    mut fetch: impl FnMut(&str, &Path) -> Result<()>,
) -> Result<Artifacts> {
    if !crate::core(version) || target != "aarch64-apple-darwin" {
        return Err("unsupported V8 version or target".into());
    }
    owned_directory(cache)?;
    let directory = cache.join(format!("rusty-v8-{version}-{target}"));
    match std::fs::create_dir(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    owned_directory(&directory)?;
    let lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(directory.join(".lease"))?;
    let info = lease.metadata()?;
    // SAFETY: geteuid has no preconditions.
    if !info.is_file() || info.uid() != unsafe { libc::geteuid() } || info.mode() & 0o022 != 0 {
        return Err("invalid V8 cache lease file".into());
    }
    // SAFETY: lease owns a live descriptor. A busy lease is a resource wait, not
    // permission to replace data while another compiler reads it.
    if unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let lease = Lease(lease);
    let profile = format!("ptrcomp_sandbox_release_{target}");
    let manifest_name = format!("rusty_v8_{profile}.sha256");
    let archive_name = format!("librusty_v8_{profile}.a.gz");
    let binding_name = format!("src_binding_{profile}.rs");
    let pins = checksums(pins)?;
    let pin = pins
        .get(&manifest_name)
        .ok_or("source has no V8 manifest pin for this target")?;
    let manifest = ensure(&directory, &manifest_name, pin, &mut fetch)?;
    let mut text = String::new();
    regular(&manifest)?
        .take(64 * 1024 + 1)
        .read_to_string(&mut text)?;
    let entries = checksums(&text)?;
    if entries.len() != 2
        || !entries.contains_key(&archive_name)
        || !entries.contains_key(&binding_name)
    {
        return Err("V8 manifest must cover exactly the archive and binding".into());
    }
    let archive = ensure(
        &directory,
        &archive_name,
        &entries[&archive_name],
        &mut fetch,
    )?;
    let binding = ensure(
        &directory,
        &binding_name,
        &entries[&binding_name],
        &mut fetch,
    )?;
    Ok(Artifacts {
        version: version.to_owned(),
        archive,
        binding,
        archive_sha256: entries[&archive_name].clone(),
        binding_sha256: entries[&binding_name].clone(),
        _lease: lease,
    })
}

/// Resolve only source-pinned inputs, with no environment overrides or execution
/// of the source repository's packaging scripts.
pub fn resolve(repo: &Path, release: &Release, cache: &Path) -> Result<Artifacts> {
    release.check_local(repo)?;
    let object = format!("{}:codex-rs/Cargo.lock", release.source);
    let lock: toml::Value = toml::from_str(&crate::git(repo, &["cat-file", "blob", &object])?)?;
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or("invalid source dependency lock")?;
    let versions: std::collections::BTreeSet<_> = packages
        .iter()
        .filter(|row| row.get("name").and_then(toml::Value::as_str) == Some("v8"))
        .filter_map(|row| row.get("version").and_then(toml::Value::as_str))
        .collect();
    if versions.len() != 1 {
        return Err("source must lock exactly one V8 version".into());
    }
    let version = versions.first().ok_or("source has no V8 version")?;
    if !crate::core(version) {
        return Err("invalid locked V8 version".into());
    }
    let pin_path = format!(
        "third_party/v8/rusty_v8_{}_release_manifests.sha256",
        version.replace('.', "_")
    );
    let object = format!("{}:{pin_path}", release.source);
    let pins = crate::git(repo, &["cat-file", "blob", &object])?;
    let tag = format!("rusty-v8-v{version}");
    resolve_pair(
        version,
        &release.target,
        &pins,
        cache,
        |name, destination| {
            let output = std::process::Command::new("agh")
                .args([
                    "release",
                    "download",
                    "openai/codex",
                    &tag,
                    "--asset",
                    name,
                    "--output",
                ])
                .arg(destination)
                .output()?;
            if !output.status.success() {
                return Err(format!("Agh could not download the pinned V8 input {name}").into());
            }
            Ok(())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::fd::FromRawFd;
    use std::time::Duration;
    use std::time::Instant;

    struct ForkedChild(Option<libc::pid_t>);

    impl ForkedChild {
        fn try_wait(&mut self) -> std::io::Result<Option<i32>> {
            let pid = self
                .0
                .ok_or_else(|| std::io::Error::other("child has already been reaped"))?;
            let mut status = 0;
            loop {
                // SAFETY: pid is this fixture's unreaped child; status is writable.
                let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
                if result == pid {
                    self.0 = None;
                    return Ok(Some(status));
                }
                if result == 0 {
                    return Ok(None);
                }
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }

        fn wait(&mut self) -> Result<i32> {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(status) = self.try_wait()? {
                    return Ok(status);
                }
                if Instant::now() >= deadline {
                    return Err("forked lease fixture did not exit".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Drop for ForkedChild {
        fn drop(&mut self) {
            if let Some(pid) = self.0.take() {
                // SAFETY: this fixture still owns this unreaped child. It only
                // waits on a pipe, and must be reaped even if an assertion fails.
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                    loop {
                        if libc::waitpid(pid, std::ptr::null_mut(), 0) >= 0
                            || std::io::Error::last_os_error().kind()
                                != std::io::ErrorKind::Interrupted
                        {
                            break;
                        }
                    }
                }
            }
        }
    }

    fn pipe() -> std::io::Result<(File, File)> {
        let mut fds = [-1; 2];
        // SAFETY: pipe initializes both descriptors on success; each is then
        // transferred once into a File owned by this fixture.
        unsafe {
            if libc::pipe(fds.as_mut_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok((File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])))
        }
    }

    #[test]
    fn dropping_the_cache_lease_releases_a_fork_inherited_lock() -> Result<()> {
        let cache = tempfile::tempdir()?;
        let profile = "ptrcomp_sandbox_release_aarch64-apple-darwin";
        let payload = b"pinned fixture";
        let digest = format!("{:x}", Sha256::digest(payload));
        let manifest =
            format!("{digest}  librusty_v8_{profile}.a.gz\n{digest}  src_binding_{profile}.rs\n");
        let pins = format!(
            "{:x}  rusty_v8_{profile}.sha256\n",
            Sha256::digest(manifest.as_bytes())
        );
        let resolve = || {
            resolve_pair(
                "152.2.0",
                "aarch64-apple-darwin",
                &pins,
                cache.path(),
                |name, path| {
                    std::fs::write(
                        path,
                        if name.ends_with(".sha256") {
                            manifest.as_bytes()
                        } else {
                            payload
                        },
                    )?;
                    Ok(())
                },
            )
        };
        let owner = resolve()?;
        let lease_fd = owner._lease.0.as_raw_fd();
        // SAFETY: owner retains this descriptor. CLOEXEC still permits fork inheritance.
        assert_ne!(
            unsafe { libc::fcntl(lease_fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        let (mut ready_read, ready_write) = pipe()?;
        let (release_read, mut release_write) = pipe()?;
        // SAFETY: the child below calls only async-signal-safe libc functions
        // and exits without running inherited Rust destructors.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if pid == 0 {
            // SAFETY: all descriptors were opened before fork. The alarm also
            // bounds the child if the parent never sends its release byte.
            unsafe {
                libc::signal(libc::SIGALRM, libc::SIG_DFL);
                libc::alarm(15);
                libc::close(ready_read.as_raw_fd());
                libc::close(release_write.as_raw_fd());
                if libc::fcntl(lease_fd, libc::F_GETFD) < 0
                    || libc::write(ready_write.as_raw_fd(), b"R".as_ptr().cast(), 1) != 1
                {
                    libc::_exit(2);
                }
                let mut release = 0_u8;
                let count = libc::read(
                    release_read.as_raw_fd(),
                    (&mut release as *mut u8).cast(),
                    1,
                );
                libc::_exit(if count == 1 && release == b'X' { 0 } else { 3 });
            }
        }
        let mut child = ForkedChild(Some(pid));
        drop(ready_write);
        drop(release_read);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("forked lease fixture did not become ready".into());
            }
            let mut poll = libc::pollfd {
                fd: ready_read.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: poll points to one initialized entry and ready_read is live.
            let result = unsafe { libc::poll(&mut poll, 1, remaining.as_millis() as i32) };
            if result > 0 {
                break;
            }
            if result < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        let mut ready = [0];
        ready_read.read_exact(&mut ready)?;
        assert_eq!(ready, *b"R");
        assert!(child.try_wait()?.is_none());
        let busy = resolve().err().ok_or("live owner lost its lease")?;
        assert_eq!(
            busy.downcast_ref::<std::io::Error>()
                .map(std::io::Error::kind),
            Some(std::io::ErrorKind::WouldBlock)
        );
        drop(owner);
        let next = resolve()?;
        next.check()?;
        assert!(
            child.try_wait()?.is_none(),
            "child must still hold the inherited descriptor"
        );
        release_write.write_all(b"X")?;
        let status = child.wait()?;
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);
        Ok(())
    }

    #[test]
    fn verified_cache_repairs_corruption_but_rejects_links_and_concurrent_replacement() -> Result<()>
    {
        let cache = tempfile::tempdir()?;
        let profile = "ptrcomp_sandbox_release_aarch64-apple-darwin";
        let archive = format!("librusty_v8_{profile}.a.gz");
        let binding = format!("src_binding_{profile}.rs");
        let manifest = format!("rusty_v8_{profile}.sha256");
        let digest = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
        let text = format!(
            "{}  {archive}\n{}  {binding}\n",
            digest(b"archive fixture"),
            digest(b"binding fixture")
        );
        let pins = format!("{}  {manifest}\n", digest(text.as_bytes()));
        let files = BTreeMap::from([
            (manifest.as_str(), text.as_bytes()),
            (archive.as_str(), b"archive fixture".as_slice()),
            (binding.as_str(), b"binding fixture".as_slice()),
        ]);
        let calls = std::cell::Cell::new(0);
        let mut fetch = |name: &str, path: &Path| -> Result<()> {
            calls.set(calls.get() + 1);
            std::fs::write(path, files.get(name).ok_or("unexpected download")?)?;
            Ok(())
        };
        let first = resolve_pair(
            "152.2.0",
            "aarch64-apple-darwin",
            &pins,
            cache.path(),
            &mut fetch,
        )?;
        assert_eq!(calls.get(), 3);
        assert!(
            resolve_pair(
                "152.2.0",
                "aarch64-apple-darwin",
                &pins,
                cache.path(),
                &mut fetch
            )
            .is_err()
        );
        let archive_path = first.archive.clone();
        drop(first);
        drop(resolve_pair(
            "152.2.0",
            "aarch64-apple-darwin",
            &pins,
            cache.path(),
            &mut fetch,
        )?);
        assert_eq!(calls.get(), 3);
        std::fs::write(&archive_path, "corrupt fixture")?;
        drop(resolve_pair(
            "152.2.0",
            "aarch64-apple-darwin",
            &pins,
            cache.path(),
            &mut fetch,
        )?);
        assert_eq!(calls.get(), 4);
        assert_eq!(std::fs::read(&archive_path)?, b"archive fixture");
        std::fs::remove_file(&archive_path)?;
        let outside = cache.path().join("outside");
        std::fs::write(&outside, "outside fixture")?;
        std::os::unix::fs::symlink(&outside, &archive_path)?;
        assert!(
            resolve_pair(
                "152.2.0",
                "aarch64-apple-darwin",
                &pins,
                cache.path(),
                &mut fetch
            )
            .is_err()
        );
        assert_eq!(calls.get(), 4);
        assert_eq!(std::fs::read(outside)?, b"outside fixture");
        Ok(())
    }

    #[test]
    fn a_manifest_download_never_authorizes_unpinned_or_extra_payloads() -> Result<()> {
        let cache = tempfile::tempdir()?;
        let manifest = "rusty_v8_ptrcomp_sandbox_release_aarch64-apple-darwin.sha256";
        let payload = format!("{}  unexpected-payload\n", "a".repeat(64));
        let pins = format!("{:x}  {manifest}\n", Sha256::digest(payload.as_bytes()));
        let calls = std::cell::Cell::new(0);
        let fetch = |name: &str, path: &Path| -> Result<()> {
            assert_eq!(name, manifest);
            calls.set(calls.get() + 1);
            std::fs::write(path, &payload)?;
            Ok(())
        };
        assert!(
            resolve_pair(
                "152.2.0",
                "aarch64-apple-darwin",
                &pins,
                cache.path(),
                fetch
            )
            .is_err()
        );
        assert_eq!(calls.get(), 1);
        assert!(checksums(&format!("{}  ../outside\n", "a".repeat(64))).is_err());
        Ok(())
    }
}
