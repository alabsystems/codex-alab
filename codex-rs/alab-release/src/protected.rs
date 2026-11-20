// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native protected execution for the release controller. Source inputs stay
//! read-only; outputs, private dependency downloads and native capacity claims
//! have explicit write roots. The certified compiler runs under the embedded, source-locked Trust memory keeper.

use crate::Result;
use std::ffi::CStr;
use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;

#[path = "worker.rs"]
mod worker;
pub use worker::Worker;
pub use worker::WorkerCancelled;

/// A held private scratch directory and its closed execution policy.
pub struct Protected {
    scratch: PathBuf,
    profile: String,
    home: PathBuf,
    cargo_home: PathBuf,
    cwd: PathBuf,
    path: std::ffi::OsString,
}

fn account_home() -> Result<PathBuf> {
    let mut storage = vec![0_u8; 4096];
    loop {
        let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut found = std::ptr::null_mut();
        // SAFETY: entry and storage are live writable buffers. The returned
        // pointers are copied before either allocation is released or resized.
        let error = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                entry.as_mut_ptr(),
                storage.as_mut_ptr().cast(),
                storage.len(),
                &mut found,
            )
        };
        if error == libc::ERANGE && storage.len() < 1024 * 1024 {
            storage.resize(storage.len() * 2, 0);
            continue;
        }
        if error != 0 || found.is_null() {
            return Err("cannot resolve the account's home directory".into());
        }
        // SAFETY: success and non-null found establish an initialized passwd;
        // pw_dir is a NUL-terminated string in the still-live storage buffer.
        let pointer = unsafe { entry.assume_init().pw_dir };
        if pointer.is_null() {
            return Err("the account has no home directory".into());
        }
        // SAFETY: getpwuid_r succeeded, and its home pointer is non-null.
        let path = unsafe { CStr::from_ptr(pointer) };
        use std::os::unix::ffi::OsStrExt;
        return directory(Path::new(OsStr::from_bytes(path.to_bytes())));
    }
}

fn directory(path: &Path) -> Result<PathBuf> {
    let path = std::fs::canonicalize(path)?;
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()).into());
    }
    Ok(path)
}

fn owned(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let info = std::fs::metadata(path)?;
    // SAFETY: geteuid has no preconditions.
    if info.uid() != unsafe { libc::geteuid() } || info.mode() & 0o022 != 0 {
        return Err(
            "release writes require owned directories without shared write permissions".into(),
        );
    }
    Ok(())
}

pub(crate) fn owned_directory(path: &Path) -> Result<PathBuf> {
    let path = directory(path)?;
    owned(&path)?;
    Ok(path)
}

/// Keep an enclosing controller's short private temporary namespace. For the
/// ordinary long Darwin TMPDIR, allocate beneath the uid's private directory
/// on the OS's sticky temporary volume. The compiler never gains write access
/// to that parent or to another release's scratch.
pub fn short_scratch(prefix: &str) -> Result<tempfile::TempDir> {
    let base = scratch_parent(true)?;
    Ok(tempfile::Builder::new().prefix(prefix).tempdir_in(base)?)
}

pub(crate) fn scratch_parent(create: bool) -> Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::fs::MetadataExt;

    let system = directory(Path::new("/tmp"))?;
    let temporary = directory(&std::env::temp_dir())?;
    let base = if temporary != system && temporary.as_os_str().len() <= 55 {
        owned(&temporary)?;
        temporary
    } else {
        let metadata = std::fs::metadata(&system)?;
        if metadata.uid() != 0 || metadata.mode() & 0o1000 == 0 {
            return Err("the OS's temporary namespace is not root-owned and sticky".into());
        }
        // SAFETY: geteuid has no preconditions.
        let uid = unsafe { libc::geteuid() };
        let private = system.join(format!("cr-{uid}"));
        let events = private.join(".ubd");
        if !create {
            return Ok(events);
        }
        for dir in [&private, &events] {
            match std::fs::DirBuilder::new().mode(0o700).create(dir) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            let info = std::fs::symlink_metadata(dir)?;
            if !info.is_dir() || info.uid() != uid || info.mode() & 0o077 != 0 {
                return Err("invalid private release scratch directory".into());
            }
        }
        events
    };
    Ok(base)
}

fn quote(path: &Path) -> Result<String> {
    Ok(serde_json::to_string(
        path.to_str().ok_or("sandbox paths must be UTF-8")?,
    )?)
}

fn home_or_above(path: &Path, homes: &[PathBuf]) -> bool {
    homes.iter().any(|home| home.starts_with(path))
}

fn rule(profile: &mut String, kind: &str, paths: &[PathBuf]) -> Result<()> {
    for path in paths {
        profile.push_str(&format!(" ({kind} {})", quote(path)?));
    }
    Ok(())
}

fn policy(
    homes: &[PathBuf],
    reads: &[PathBuf],
    readonly: &[PathBuf],
    writes: &[PathBuf],
    cargo_home: &Path,
    scratch: &Path,
) -> Result<String> {
    if reads
        .iter()
        .chain(writes)
        .any(|path| home_or_above(path, homes))
    {
        return Err("a release root must not expose a home or its ancestor".into());
    }
    let mut profile = "(version 1)\n(allow default)\n(deny file-read-data".to_owned();
    rule(&mut profile, "subpath", homes)?;
    profile.push_str(")\n(allow file-read-data");
    rule(&mut profile, "subpath", reads)?;
    rule(&mut profile, "subpath", writes)?;
    for home in homes {
        rule(&mut profile, "literal", &[home.join(".gitconfig")])?;
    }
    // Directory metadata permits the native compiler front to authenticate its
    // execution root, without making ancestor directories' contents readable.
    for path in reads.iter().chain(writes) {
        for ancestor in path.ancestors().skip(1) {
            if homes.iter().any(|home| ancestor.starts_with(home)) {
                rule(&mut profile, "literal", &[ancestor.to_path_buf()])?;
            }
        }
    }
    // Cargo reads ancestor config files even when the source and dependency
    // home live in separate trees. They are inputs, never writable outputs.
    for path in reads
        .iter()
        .map(PathBuf::as_path)
        .chain(std::iter::once(cargo_home))
    {
        for ancestor in path.ancestors() {
            if homes.iter().any(|home| ancestor.starts_with(home)) {
                rule(
                    &mut profile,
                    "literal",
                    &[
                        ancestor.join(".cargo/config"),
                        ancestor.join(".cargo/config.toml"),
                    ],
                )?;
            }
        }
    }
    profile.push_str(")\n(deny file-read-data");
    for home in homes {
        let global_cargo =
            std::fs::canonicalize(home.join(".cargo")).unwrap_or_else(|_| home.join(".cargo"));
        rule(
            &mut profile,
            "literal",
            &[
                home.join(".cargo/credentials"),
                home.join(".cargo/credentials.toml"),
                home.join(".config/git/credentials"),
                global_cargo.join("credentials"),
                global_cargo.join("credentials.toml"),
            ],
        )?;
    }
    rule(
        &mut profile,
        "literal",
        &[
            cargo_home.join("credentials"),
            cargo_home.join("credentials.toml"),
        ],
    )?;
    profile.push_str(")\n(deny file-write*)\n(allow file-write* (subpath \"/dev\")");
    rule(&mut profile, "subpath", writes)?;
    profile.push_str(")\n(deny file-write*");
    rule(
        &mut profile,
        "literal",
        &[
            cargo_home.join("config"),
            cargo_home.join("config.toml"),
            cargo_home.join(".alab-seeded"),
        ],
    )?;
    rule(&mut profile, "subpath", &[cargo_home.join(".cargo")])?;
    profile.push_str(")\n");
    // Read inputs remain read-only even if nested beneath a writable cache.
    if !readonly.is_empty() {
        profile.push_str("(deny file-write*");
        rule(&mut profile, "subpath", readonly)?;
        profile.push_str(")\n");
    }
    profile.push_str("(deny network-outbound (remote unix-socket))\n");
    profile.push_str(&format!(
        "(allow network-outbound (remote unix-socket (subpath {})) (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")))\n",
        quote(scratch)?
    ));
    profile.push_str(
        "(deny mach-lookup (global-name \"com.apple.SecurityServer\") (global-name \"com.apple.securityd.xpc\")\n\
          (global-name \"com.apple.pasteboard.1\") (global-name \"com.apple.coreservices.launchservicesd\")\n\
          (global-name \"com.apple.cfprefsd.daemon\") (global-name \"com.apple.cfprefsd.agent\") (local-name \"com.apple.cfprefsd.agent\"))\n\
         (deny ipc-posix-shm-read* (ipc-posix-name-prefix \"apple.cfprefs.\") (ipc-posix-name-prefix \"/apple.cfprefs.\"))\n\
         (deny user-preference-read)\n(deny appleevent-send)\n\
         (deny process-exec (literal \"/bin/launchctl\") (literal \"/usr/bin/open\") (literal \"/usr/bin/osascript\") (literal \"/usr/bin/security\"))\n",
    );
    Ok(profile)
}

impl Protected {
    /// Construct the release boundary from actual directories and account state.
    /// A missing OS sandbox refuses; no execution falls back to an uncontained run.
    pub fn new(
        repo: &Path,
        cargo_home: &Path,
        read: &[PathBuf],
        write: &[PathBuf],
    ) -> Result<Self> {
        if !cfg!(target_os = "macos") || !Path::new("/usr/bin/sandbox-exec").is_file() {
            return Err("native release execution requires macOS sandbox-exec".into());
        }
        let home = directory(&PathBuf::from(
            std::env::var_os("HOME").ok_or("HOME is unset")?,
        ))?;
        let account = account_home()?;
        let homes = vec![home.clone(), account.clone()];
        let cwd = directory(repo)?;
        let cargo_home = owned_directory(cargo_home)?;
        if cargo_home.starts_with(&cwd)
            || homes.iter().any(|home| {
                let global = std::fs::canonicalize(home.join(".cargo"))
                    .unwrap_or_else(|_| home.join(".cargo"));
                cargo_home.starts_with(global)
            })
        {
            return Err("release dependencies require a private Cargo home".into());
        }
        let common = crate::git(
            &cwd,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let common = directory(Path::new(common.trim()))?;
        let mut reads = vec![cwd.clone(), common.clone()];
        for path in read {
            reads.push(std::fs::canonicalize(path)?);
        }
        for path in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
            if path.is_dir() && homes.iter().any(|home| path.starts_with(home)) {
                reads.push(directory(&path)?);
            }
        }
        if let Some(bin) = std::env::var_os("ATPKG_BIN") {
            let bin = directory(Path::new(&bin))?;
            let store = bin.parent().ok_or("atpkg bin has no parent")?.join("store");
            reads.push(directory(&store)?);
            let abrew = bin.parent().ok_or("atpkg bin has no parent")?.join("abrew");
            if abrew.is_dir() {
                reads.push(directory(&abrew)?);
            }
        }
        for variable in ["ATPKG_PROJECT_FRONT", "ATPKG_PROJECT_EXECUTION"] {
            if let Some(path) = std::env::var_os(variable) {
                reads.push(std::fs::canonicalize(path)?);
            }
        }
        if let Some(path) = std::env::var_os("RUSTUP_HOME") {
            reads.push(directory(Path::new(&path))?);
        } else if home.join(".rustup").is_dir() {
            reads.push(directory(&home.join(".rustup"))?);
        }
        for home in &homes {
            for relative in [
                ".trust",
                ".config/git",
                ".local/share/uv/python",
                "Library/Caches/aterm/.ubd/registered-builds.noindex",
            ] {
                let path = home.join(relative);
                if path.is_dir() {
                    reads.push(directory(&path)?);
                }
            }
        }
        let capacity = account.join("Library/Caches/aterm/capacity");
        if directory(&capacity)? != capacity {
            return Err("native capacity directory is aliased".into());
        }
        let scratch = short_scratch("cr.")?;
        let scratch_path = directory(scratch.path())?;
        owned(&scratch_path)?;
        let mut paths: Vec<_> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata("/bin/ps")?.permissions().mode() & 0o6000 != 0 {
            let tools = scratch_path.join("tools");
            std::fs::create_dir(&tools)?;
            let ps = tools.join("ps");
            std::fs::copy("/bin/ps", &ps)?;
            std::fs::set_permissions(&ps, std::fs::Permissions::from_mode(0o755))?;
            let signed = Command::new("/usr/bin/codesign")
                .args(["--force", "--sign", "-"])
                .arg(&ps)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .output()?;
            if !signed.status.success() {
                return Err("could not sign the unprivileged process inspector".into());
            }
            let pid = std::process::id().to_string();
            let observed = Command::new(&ps)
                .args(["-o", "pid=", "-p", &pid])
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .output()?;
            if !observed.status.success() || String::from_utf8(observed.stdout)?.trim() != pid {
                return Err(
                    "the unprivileged process inspector could not observe its controller".into(),
                );
            }
            reads.push(tools.clone());
            paths.insert(0, tools);
        }
        let mut writes = vec![cargo_home.clone(), capacity, scratch_path.clone()];
        for path in write {
            let path = owned_directory(path)?;
            if reads
                .iter()
                .any(|input| path.starts_with(input) || input.starts_with(&path))
            {
                return Err("release output overlaps a read-only input".into());
            }
            writes.push(path);
        }
        // The common Git directory is readable metadata, with only the private
        // Cargo home beneath it writable. Immutable source and tool inputs still
        // receive the final read-only fence.
        let readonly: Vec<_> = reads
            .iter()
            .filter(|path| *path != &common)
            .cloned()
            .collect();
        let profile = policy(
            &homes,
            &reads,
            &readonly,
            &writes,
            &cargo_home,
            &scratch_path,
        )?;
        Ok(Self {
            // A leader's exit does not prove its descendants drained. The outer
            // native supervisor records this path and owns subsequent cleanup.
            scratch: scratch.keep(),
            profile,
            home,
            cargo_home,
            cwd,
            path: std::env::join_paths(paths)?,
        })
    }

    /// Retained until the outer release supervisor establishes group drain.
    pub fn scratch_path(&self) -> &Path {
        &self.scratch
    }

    /// The caller may add only its stage's declared build inputs to this scrubbed
    /// environment. This wrapper establishes isolation, never a successful check.
    pub fn command(&self, program: &OsStr, args: &[std::ffi::OsString]) -> Command {
        self.profile_command(
            Command::new("/usr/bin/sandbox-exec"),
            &self.profile,
            program,
            args,
        )
    }

    fn certified_profile(&self) -> Result<String> {
        let socket = std::fs::canonicalize("/tmp")?
            .join(format!("trustd-runtime-locks-{}", unsafe {
                libc::geteuid()
            }))
            .join("trust-memory-jobserver.sock");
        let mut profile = self.profile.clone();
        profile.push_str(&format!(
            "(allow network-outbound (remote unix-socket (path-literal {})))\n",
            quote(&socket)?
        ));
        Ok(profile)
    }

    fn profile_command(
        &self,
        mut command: Command,
        profile: &str,
        program: &OsStr,
        args: &[std::ffi::OsString],
    ) -> Command {
        command
            .args(["-p", profile])
            .arg(program)
            .args(args)
            .current_dir(&self.cwd)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", &self.path)
            .env("TMPDIR", &self.scratch)
            .env("CARGO_HOME", &self.cargo_home)
            .env("TARGO_HOME", &self.cargo_home)
            .env("CARGO_NET_GIT_FETCH_WITH_CLI", "true");
        for key in [
            "USER",
            "LOGNAME",
            "SHELL",
            "TERM",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
            "RUSTUP_HOME",
            "ATPKG_BIN",
            "CARGO_BUILD_JOBS",
            "RUST_TEST_THREADS",
            "ATPKG_PROJECT_FRONT",
            "ATPKG_PROJECT_TARGO",
            "ATPKG_PROJECT_TRUSTC",
            "ATPKG_PROJECT_TRUSTDOC",
            "ATPKG_PROJECT_PAYLOAD",
            "ATPKG_PROJECT_EXECUTION",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
    }

    pub fn run(&self, program: &OsStr, args: &[std::ffi::OsString]) -> Result<ExitStatus> {
        Ok(self.command(program, args).status()?)
    }
}

#[cfg(target_os = "macos")]
pub fn verify_native_wrapper() -> Result<()> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let dependencies = root.path().join("dependencies");
    let output = root.path().join("output");
    for path in [&source, &dependencies, &output] {
        std::fs::create_dir(path)?;
    }
    crate::git(&source, &["init", "--quiet"])?;
    std::fs::write(source.join("input"), "immutable input")?;
    let guard = Protected::new(&source, &dependencies, &[], std::slice::from_ref(&output))?;
    let environment = guard.command(OsStr::new("/usr/bin/env"), &[]).output()?;
    if !environment.status.success() {
        return Err(format!(
            "native sandbox control could not run: {}",
            String::from_utf8_lossy(&environment.stderr)
        )
        .into());
    }
    let environment = String::from_utf8(environment.stdout)?;
    for line in environment.lines() {
        assert!(!line.starts_with("CODEX_"));
        assert!(!line.starts_with("ANTHROPIC_"));
        assert!(!line.starts_with("GITHUB_TOKEN="));
        assert!(!line.starts_with("RUSTC="));
        assert!(!line.starts_with("RUSTFLAGS="));
    }
    let changed = guard
        .command(
            OsStr::new("/usr/bin/touch"),
            &[source.join("changed").into_os_string()],
        )
        .output()?;
    assert!(!changed.status.success());
    let artifact = guard
        .command(
            OsStr::new("/usr/bin/touch"),
            &[output.join("artifact").into_os_string()],
        )
        .output()?;
    assert!(artifact.status.success());
    assert!(Protected::new(&source, &dependencies, &[], std::slice::from_ref(&source)).is_err());
    // The known env/touch test children fork no workers and have exited.
    std::fs::remove_dir_all(guard.scratch_path())?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn verify_native_policy() -> Result<()> {
    let root = tempfile::tempdir()?;
    let root = directory(root.path())?;
    let home = root.join("home");
    let source = home.join("source");
    let output = home.join("output");
    let cargo = home.join("dependencies");
    let scratch = root.join("scratch");
    for dir in [&source, &output, &cargo, &scratch] {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(source.join("input"), "source bytes")?;
    std::fs::write(cargo.join("credentials.toml"), "private fixture")?;
    std::fs::write(
        cargo.join("config.toml"),
        "private dependency configuration",
    )?;
    let profile = policy(
        std::slice::from_ref(&home),
        std::slice::from_ref(&source),
        std::slice::from_ref(&source),
        &[output.clone(), cargo.clone(), scratch.clone()],
        &cargo,
        &scratch,
    )?;
    let run = |program: &str, path: &Path| -> Result<std::process::Output> {
        Ok(Command::new("/usr/bin/sandbox-exec")
            .args(["-p", &profile, program])
            .arg(path)
            .output()?)
    };
    let read = run("/bin/cat", &source.join("input"))?;
    if !read.status.success() || read.stdout != b"source bytes" {
        return Err(format!(
            "native sandbox source-read control failed: {}",
            String::from_utf8_lossy(&read.stderr)
        )
        .into());
    }
    assert!(
        !run("/bin/cat", &cargo.join("credentials.toml"))?
            .status
            .success()
    );
    assert!(
        !run("/usr/bin/touch", &source.join("changed"))?
            .status
            .success()
    );
    assert!(
        !run("/usr/bin/touch", &cargo.join("config.toml"))?
            .status
            .success()
    );
    assert!(
        run("/usr/bin/touch", &output.join("artifact"))?
            .status
            .success()
    );
    assert!(
        run("/usr/bin/touch", &cargo.join("download"))?
            .status
            .success()
    );
    assert!(
        policy(
            std::slice::from_ref(&home),
            std::slice::from_ref(&home),
            &[],
            &[],
            &cargo,
            &scratch
        )
        .is_err()
    );
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn certified_compile_uses_embedded_outside_keeper_and_only_its_exact_socket() {
        let private = tempfile::tempdir().unwrap();
        let protected = Protected {
            scratch: private.path().into(),
            profile: "(version 1)\n(deny network-outbound (remote unix-socket))\n".into(),
            home: private.path().into(),
            cargo_home: private.path().into(),
            cwd: private.path().into(),
            path: "/usr/bin:/bin".into(),
        };
        let source = OsStr::new("/checkout/alab/certified-build.sh");
        let canonical = std::ffi::OsString::from("--verified");
        let worker = Worker::new().unwrap();
        let compiler = worker
            .command(&protected, source, std::slice::from_ref(&canonical), true)
            .unwrap();
        let args: Vec<_> = compiler.get_args().collect();
        assert_eq!(compiler.get_program(), OsStr::new("/usr/bin/python3"));
        assert_eq!(
            &args[..4],
            ["-I", "-B", "-S", "-c"].map(OsStr::new).as_slice()
        );
        assert_eq!(
            args[4],
            OsStr::new(include_str!("../../../alab/native_worker.py"))
        );
        assert_eq!(
            args[5],
            OsStr::new(include_str!("../../../alab/command_guard.py"))
        );
        assert_eq!(
            args[6],
            OsStr::new(include_str!("../../../alab/trust_memory.py"))
        );
        assert_eq!(args[8], OsStr::new("compiler"));
        assert_eq!(args[9], OsStr::new("/usr/bin/sandbox-exec"));
        assert_eq!(args[10], OsStr::new("-p"));
        let expected = std::fs::canonicalize("/tmp").unwrap().join(format!(
            "trustd-runtime-locks-{}/trust-memory-jobserver.sock",
            unsafe { libc::geteuid() }
        ));
        let rule = format!(
            "(allow network-outbound (remote unix-socket (path-literal {})))\n",
            quote(&expected).unwrap()
        );
        assert_eq!(
            args[11],
            OsStr::new(&format!("{}{rule}", protected.profile))
        );
        assert_eq!(args[12], source);
        assert_eq!(args[13], canonical.as_os_str());
        let plain = protected.command(source, &[]);
        assert_eq!(plain.get_program(), OsStr::new("/usr/bin/sandbox-exec"));
        assert!(
            !plain
                .get_args()
                .any(|w| w.to_string_lossy().contains("trust-memory-jobserver"))
        );
    }

    #[test]
    #[ignore = "macOS refuses nested sandbox_apply; check_selected runs the native verify-sandbox stage"]
    fn native_wrapper_runs_with_scrubbed_environment_and_read_only_source() -> crate::Result<()> {
        super::verify_native_wrapper()
    }

    #[test]
    #[ignore = "macOS refuses nested sandbox_apply; check_selected runs the native verify-sandbox stage"]
    fn actual_kernel_policy_preserves_sources_and_credentials_and_permits_outputs()
    -> crate::Result<()> {
        super::verify_native_policy()
    }
}
