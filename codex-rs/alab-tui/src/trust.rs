// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Folder trust inside aterm (CONTRACT §5, "folder trust"): codex-alab answers codex's own
//! folder-trust screen at launch the way aterm's Codex lane answered it
//! (`crates/aterm-agent/src/supervise/policy/approval.rs`: `trust_dialog`, rule
//! `trust-dialog@v1`, its roots read by `roots_from_config` and held by `ScratchRoot::holds`,
//! the repository root checked by `codex_repository_root`); that lane is retired (aterm
//! 83bd43c206), so a screen this hook leaves is a person's to answer.
//!
//! One hook (`ALAB(trust)` in `tui/src/onboarding/directory_trust.rs`, right before the screen
//! is built, into `glue/folder_trust.rs`) asks [`judge`]. It trusts only when all of these
//! hold; otherwise the screen shows exactly as upstream's:
//!
//! - this is the codex-alab binary ([`codex_alab_core::active`]) in an aterm session (an
//!   [`Endpoint`] for `$ATERM_PARENT_SESSION_ID`), on the embedded app-server (never a
//!   connected or remote one);
//! - the project is not marked untrusted: a person's explicit untrust is never overridden;
//! - the folder is exactly this process's launch directory (the launch snapshot's cwd), never
//!   a `-C` folder or a resumed task's other folder (`app/resume_config.rs` asks too);
//! - aterm's `harness policy` for this session answered within [`POLICY_TIMEOUT`] with
//!   `effective=on` (aterm folds `enabled`, the agent's own table, a suspension, a hold and
//!   Autopilot `off` into it) and `approve=safe` or `all`, never `none`;
//! - under `approve=safe`, the folder AND the folder codex's trust is written for
//!   (`trust_target`: in a Git repository's subdirectory, the repository root) lie under a trust
//!   root ([`roots_from_config`] over `trust_roots=`), each both as written and where its
//!   symlinks lead, as aterm judges a path ([`RULE`]);
//! - under `approve=all`, a folder the roots do not hold is trusted too ([`RULE_ANY`]): aterm's
//!   full power presses "Trust and continue" for any folder (`full_power`, `trust-any@v1`), and
//!   `all` is aterm's default. The roots still decide first, so a rooted folder keeps its rule.
//!
//! The glue then writes codex's own trust entry through the write path the screen's "Trust and
//! continue" takes (the same standing entry aterm's press writes), and the launch session card
//! says which folder was trusted and why ([`take_notice`]). An older aterm (no `harness
//! policy`), no socket, a timeout or any other error shows the screen (fail closed).

use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;

use codex_alab_aterm::Client;
use codex_alab_aterm::Endpoint;
use codex_alab_aterm::HarnessPolicy;
use codex_alab_aterm::Options;
use codex_alab_aterm::PolicyAnswer;
use codex_alab_aterm::policy::ApproveLevel;
use codex_alab_core::active;
use codex_alab_core::session::shared;

/// The aterm rule a rooted answer mirrors (`RULE_TRUST_DIALOG`).
pub const RULE: &str = "trust-dialog@v1";

/// The aterm rule full power's answer mirrors (`RULE_TRUST_ANY`): any launch folder.
pub const RULE_ANY: &str = "trust-any@v1";

/// How long the launch waits for aterm's policy. aterm answers a local read in milliseconds;
/// past this the screen shows.
pub const POLICY_TIMEOUT: Duration = Duration::from_secs(2);

/// One component of a [`TrustRoot`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum Comp {
    Lit(String),
    /// `<prefix>*` in the last place: any name that starts with the (non-empty) prefix.
    Prefix(String),
}

/// A trust root (`[harness] trust_roots`): an absolute directory, or `<dir>/<prefix>*`. It
/// holds itself and everything under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustRoot {
    comps: Vec<Comp>,
    spec: String,
}

impl TrustRoot {
    /// Exactly the absolute directory `path`; `None` for a relative path or one with `..`.
    fn dir(path: &str, spec: &str) -> Option<TrustRoot> {
        let comps = abs_components(path)?;
        Some(TrustRoot {
            comps: comps.into_iter().map(Comp::Lit).collect(),
            spec: spec.trim().to_string(),
        })
    }

    /// The root as the owner wrote it in aterm.toml.
    pub fn spec(&self) -> &str {
        &self.spec
    }

    /// The pattern with `~` expanded (`/Users/_owner/src/app*`), as aterm labels it.
    pub fn label(&self) -> String {
        let mut label = String::new();
        for comp in &self.comps {
            label.push('/');
            match comp {
                Comp::Lit(name) => label.push_str(name),
                Comp::Prefix(prefix) => {
                    label.push_str(prefix);
                    label.push('*');
                }
            }
        }
        if label.is_empty() {
            label.push('/');
        }
        label
    }

    /// Whether the absolute path `comps` is this root or inside it.
    pub fn holds(&self, comps: &[String]) -> bool {
        comps.len() >= self.comps.len()
            && self.comps.iter().zip(comps).all(|(root, name)| match root {
                Comp::Lit(lit) => lit == name,
                Comp::Prefix(prefix) => name.starts_with(prefix.as_str()),
            })
    }
}

/// Roots from aterm's `trust_roots` text, as aterm reads them: `~` and `~/` are `home`, a
/// trailing `*` matches any suffix of the last component (`~/src/app*`, never a bare `*`),
/// resolved lexically, and `/tmp/claude-*` (or `/private/tmp/claude-*`) is this user's own
/// `claude-<uid>`. A spec not absolute after `~`, holding `..`, or globbing anywhere else is
/// dropped, never widened.
pub fn roots_from_config(specs: &[String], home: Option<&Path>, uid: u32) -> Vec<TrustRoot> {
    let home = home.and_then(Path::to_str);
    let mut out = Vec::new();
    for written in specs {
        let own = own_claude_tmp(written, uid);
        let spec = own.trim().trim_end_matches('/');
        let full = if spec == "~" {
            match home {
                Some(home) => home.to_string(),
                None => continue,
            }
        } else if let Some(rest) = spec.strip_prefix("~/") {
            match home {
                Some(home) => format!("{}/{rest}", home.trim_end_matches('/')),
                None => continue,
            }
        } else {
            spec.to_string()
        };
        let Some((parent, last)) = full.rsplit_once('/') else {
            continue;
        };
        let parent = if parent.is_empty() { "/" } else { parent };
        if parent.contains(['*', '?', '[']) || last.contains(['?', '[']) {
            continue;
        }
        let root = match last.find('*') {
            None => TrustRoot::dir(&full, written),
            Some(at) if at + 1 == last.len() && at > 0 => {
                TrustRoot::dir(parent, written).map(|mut root| {
                    root.comps.push(Comp::Prefix(last[..at].to_string()));
                    root
                })
            }
            Some(_) => None,
        };
        out.extend(root);
    }
    out
}

/// aterm's own-tmp rewrite (`own_claude_tmp`): the shared `claude-*` scratch glob names only
/// this user's directory.
fn own_claude_tmp(spec: &str, uid: u32) -> String {
    let trimmed = spec.trim().trim_end_matches('/');
    for base in ["/private/tmp", "/tmp"] {
        if trimmed == format!("{base}/claude-*") {
            return format!("{base}/claude-{uid}");
        }
    }
    spec.to_string()
}

/// The components of an absolute path, `.` and empty ones dropped; `None` for a relative path
/// or one with `..`.
pub fn abs_components(path: &str) -> Option<Vec<String>> {
    let rest = path.strip_prefix('/')?;
    let mut out = Vec::new();
    for comp in rest.split('/') {
        match comp {
            "" | "." => {}
            ".." => return None,
            comp => out.push(comp.to_string()),
        }
    }
    Some(out)
}

/// `comps` with every symlink resolved: the deepest existing ancestor canonicalized, plus the
/// components that do not exist yet as written. `None` for a dangling link or a refused look
/// (aterm's `real_components`).
pub fn real_components(comps: &[String]) -> Option<Vec<String>> {
    let mut k = comps.len();
    loop {
        let path = format!("/{}", comps[..k].join("/"));
        match std::fs::canonicalize(&path) {
            Ok(real) => {
                let mut out = abs_components(real.to_str()?)?;
                out.extend(comps[k..].iter().cloned());
                return Some(out);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound && k > 0 => {
                if std::fs::symlink_metadata(&path).is_ok() {
                    return None;
                }
                k -= 1;
            }
            Err(_) => return None,
        }
    }
}

/// A folder path as components: UTF-8, absolute, no pattern character, no `..` (aterm's
/// `resolve_abs` on the dialog's path).
fn comps_of(path: &Path) -> Result<Vec<String>, String> {
    let shown = path.display();
    let text = path
        .to_str()
        .ok_or_else(|| format!("{shown} is not a UTF-8 path"))?;
    if text.contains(['*', '?', '[', '{']) {
        return Err(format!("{shown} holds a pattern character"));
    }
    abs_components(text).ok_or_else(|| format!("{shown} is not an absolute path without `..`"))
}

/// What codex knows of the folder its screen would ask about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    /// The TUI runs its own app-server (`AppServerTarget::Embedded`).
    pub embedded: bool,
    /// The project is marked untrusted (`trust_level = "untrusted"`): the screen offers its
    /// restricted mode, and a person's untrust stands.
    pub restricted: bool,
    /// This process's launch directory, from the launch snapshot.
    pub launch_cwd: Option<PathBuf>,
    /// The folder the screen names (`RemoteProjectTrust::cwd`).
    pub folder: PathBuf,
    /// The folder codex writes the trust for (`RemoteProjectTrust::trust_target`).
    pub trust_target: PathBuf,
}

impl Gate {
    /// The gate for this process: its launch directory from the launch snapshot.
    pub fn at_launch(embedded: bool, restricted: bool, folder: &Path, trust_target: &Path) -> Gate {
        Gate {
            embedded,
            restricted,
            launch_cwd: shared().launch().and_then(|launch| launch.cwd.clone()),
            folder: folder.to_path_buf(),
            trust_target: trust_target.to_path_buf(),
        }
    }
}

/// A trust codex-alab may write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// The folder the screen would have named.
    pub folder: PathBuf,
    /// The folder the trust entry is written for.
    pub trust_target: PathBuf,
    /// The trust root that holds the folder, as the owner wrote it; `None` under full power
    /// ([`RULE_ANY`]).
    pub root: Option<String>,
    /// `approve=` as aterm sent it.
    pub approve: ApproveLevel,
}

impl Grant {
    /// The aterm rule this grant mirrors.
    pub fn rule(&self) -> &'static str {
        if self.root.is_some() { RULE } else { RULE_ANY }
    }
}

/// The launch session card's line for a [`Grant`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// What was trusted, under which root.
    pub message: String,
    /// Where it came from.
    pub hint: String,
}

impl Grant {
    /// The line the launch session card shows.
    pub fn notice(&self) -> Notice {
        let why = match &self.root {
            Some(root) => format!("under aterm's trust root {root}"),
            None => "under aterm's full power (approve=all)".to_string(),
        };
        let message = if self.folder == self.trust_target {
            format!("Trusted {} {why}", self.folder.display())
        } else {
            format!(
                "Trusted {} (the repository root of {}) {why}",
                self.trust_target.display(),
                self.folder.display()
            )
        };
        Notice {
            message,
            hint: format!(
                "aterm [harness] approve={}, as \"Trust and continue\" would",
                self.approve.word()
            ),
        }
    }
}

/// Whether the folder is trusted without the screen, and why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Write the trust and skip the screen.
    Trust(Grant),
    /// Show the screen, as upstream does; the reason is for the log.
    Show(String),
}

/// The checks that need no aterm: an embedded app-server, no untrust, the launch folder.
pub fn precheck(gate: &Gate) -> Result<(), String> {
    if !gate.embedded {
        return Err("a connected app-server: its folders are not this process's".to_string());
    }
    if gate.restricted {
        return Err(format!(
            "{} is marked untrusted, and a person's untrust stands",
            gate.trust_target.display()
        ));
    }
    let Some(launch) = gate.launch_cwd.as_deref() else {
        return Err("no launch directory was recorded".to_string());
    };
    if comps_of(&gate.folder)? != comps_of(launch)? {
        return Err(format!(
            "{} is not this process's launch folder {}",
            gate.folder.display(),
            launch.display()
        ));
    }
    Ok(())
}

/// The decision, given aterm's policy for this session. `home` expands `~` in the roots, `uid`
/// names this user's `claude-<uid>`, and `resolve` is where a path's symlinks lead
/// ([`real_components`] in production).
pub fn decide(
    gate: &Gate,
    policy: &HarnessPolicy,
    home: Option<&Path>,
    uid: u32,
    resolve: impl Fn(&[String]) -> Option<Vec<String>>,
) -> Verdict {
    if let Err(why) = precheck(gate) {
        return Verdict::Show(why);
    }
    if !policy.effective {
        return Verdict::Show(format!(
            "aterm's harness is off for this session (effective=off why={})",
            policy.why
        ));
    }
    let approve = policy.approve_level();
    if approve == ApproveLevel::None {
        return Verdict::Show("approve=none: every folder is a person's".to_string());
    }
    let grant = |root: Option<String>| {
        Verdict::Trust(Grant {
            folder: gate.folder.clone(),
            trust_target: gate.trust_target.clone(),
            root,
            approve,
        })
    };
    match rooted(gate, policy, home, uid, &resolve) {
        Ok(root) => grant(Some(root)),
        // Full power trusts any folder, as aterm's `full_power` does (`trust-any@v1`).
        Err(_) if approve == ApproveLevel::All => grant(None),
        Err(why) => Verdict::Show(why),
    }
}

/// The trust root (as the owner wrote it) that holds both the folder and codex's trust target,
/// or why none does ([`RULE`]).
fn rooted(
    gate: &Gate,
    policy: &HarnessPolicy,
    home: Option<&Path>,
    uid: u32,
    resolve: &impl Fn(&[String]) -> Option<Vec<String>>,
) -> Result<String, String> {
    let roots = roots_from_config(&policy.trust_roots(), home, uid);
    if roots.is_empty() {
        return Err("no trust root aterm would take (trust_roots)".to_string());
    }
    let root = held(&gate.folder, &roots, resolve)?;
    if let Err(why) = held(&gate.trust_target, &roots, resolve) {
        return Err(format!(
            "codex's trust for {} is written for {}: {why}",
            gate.folder.display(),
            gate.trust_target.display()
        ));
    }
    Ok(root.spec().to_string())
}

/// The root that holds `path` as written, when one also holds it where its symlinks lead.
fn held<'r>(
    path: &Path,
    roots: &'r [TrustRoot],
    resolve: &impl Fn(&[String]) -> Option<Vec<String>>,
) -> Result<&'r TrustRoot, String> {
    let comps = comps_of(path)?;
    let Some(root) = roots.iter().find(|root| root.holds(&comps)) else {
        return Err(format!("{} is under no trust root", path.display()));
    };
    let real = resolve(&comps)
        .ok_or_else(|| format!("{}'s symlinks cannot be resolved", path.display()))?;
    if real != comps && !roots.iter().any(|root| root.holds(&real)) {
        return Err(format!(
            "{} leads to /{}, under no trust root",
            path.display(),
            real.join("/")
        ));
    }
    Ok(root)
}

/// `@self harness policy` on a connection of its own, closed after the read, within `within`.
/// An `ERR` (an older aterm) or an `OK` this client cannot read, no answer in time, or no
/// connection is the reason the screen shows.
pub async fn read_policy(endpoint: Endpoint, within: Duration) -> Result<HarnessPolicy, String> {
    let options = Options {
        timeout: within,
        ..Options::default()
    };
    let read = async {
        let client = Client::connect(endpoint, options)
            .await
            .map_err(|err| err.to_string())?;
        let answer = client.harness_policy().await;
        client.close();
        match answer.map_err(|err| err.to_string())? {
            PolicyAnswer::Supported(policy) => Ok(policy),
            PolicyAnswer::Unsupported(line) => {
                Err(format!("aterm gave no harness policy ({line})"))
            }
        }
    };
    match tokio::time::timeout(within, read).await {
        Ok(read) => read,
        Err(_) => Err(format!(
            "aterm did not answer harness policy within {} ms",
            within.as_millis()
        )),
    }
}

/// [`judge`] with its inputs named: the session's endpoint (`None` outside aterm), `home` and
/// `uid` for the roots. Checks that need no aterm come first, so nothing connects for them.
pub async fn judge_with(
    gate: &Gate,
    endpoint: Option<Endpoint>,
    home: Option<&Path>,
    uid: u32,
) -> Verdict {
    if let Err(why) = precheck(gate) {
        return Verdict::Show(why);
    }
    let Some(endpoint) = endpoint else {
        return Verdict::Show("not inside an aterm session".to_string());
    };
    match read_policy(endpoint, POLICY_TIMEOUT).await {
        Ok(policy) => decide(gate, &policy, home, uid, real_components),
        Err(why) => Verdict::Show(why),
    }
}

/// Whether the hook asks at all: only in the codex-alab binary.
pub fn armed() -> bool {
    active()
}

/// The verdict for this process's folder-trust screen (module doc). Inert unless [`active`].
pub async fn judge(gate: &Gate) -> Verdict {
    if !active() {
        return Verdict::Show("not codex-alab".to_string());
    }
    let home = shared()
        .launch()
        .and_then(|launch| launch.var("HOME"))
        .map(PathBuf::from);
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    judge_with(gate, Endpoint::from_env(), home.as_deref(), uid).await
}

static NOTICE: Mutex<Option<Notice>> = Mutex::new(None);

/// The trust was written: the launch session card says so ([`take_notice`]).
pub fn note_trusted(grant: &Grant) {
    *NOTICE.lock().unwrap_or_else(PoisonError::into_inner) = Some(grant.notice());
}

/// The line for the launch session card, once (`glue/banner_cell.rs`). Inert unless
/// [`active`].
pub fn take_notice() -> Option<Notice> {
    if !active() {
        return None;
    }
    NOTICE.lock().unwrap_or_else(PoisonError::into_inner).take()
}
