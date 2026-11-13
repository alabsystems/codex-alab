// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Affirmative evidence for Cargo's disposable Git copies, never a cache-path exclusion.

use super::*;
use std::io::Read as _;

const INITIAL_LOG_BYTES: u64 = 4096;

async fn git_strict(root: &Path, args: &[&str], reads: &Reads) -> Option<String> {
    let mut exact = vec!["--no-replace-objects"];
    exact.extend_from_slice(args);
    super::git_strict(root, &exact, reads).await
}

/// Positive observations live only in one `session_reads` invocation. A graph is
/// never shared across probes or across different effective ownership names.
#[derive(Default)]
pub(super) struct Memo {
    entries: Vec<Observation>,
    stored: usize,
}

struct Observation {
    anchor: PathBuf,
    names: Vec<String>,
    members: HashSet<PathBuf>,
}

fn live(reads: &Reads) -> bool {
    if reads
        .deadline
        .is_some_and(|end| tokio::time::Instant::now() < end)
    {
        true
    } else {
        reads.incomplete.store(true, Ordering::SeqCst);
        false
    }
}

impl Memo {
    pub(super) async fn derived(
        &mut self,
        candidate: &Path,
        names: &[String],
        reads: &Reads,
    ) -> bool {
        if !live(reads) {
            return false;
        }
        // Paths arrived through canonical, unattended repository discovery. A hit
        // is the same graph observation, not a second partially refreshed proof.
        if self.entries.iter().any(|copy| {
            copy.names == names
                && candidate.starts_with(&copy.anchor)
                && copy.members.contains(candidate)
        }) {
            return live(reads);
        }
        self.prove(candidate, names, reads).await
    }

    async fn prove(&mut self, candidate: &Path, names: &[String], reads: &Reads) -> bool {
        for anchor in candidate.ancestors().take(STATE_FILES) {
            let Some(package) = anchor.parent() else {
                continue;
            };
            let Some(checkouts) = package.parent() else {
                continue;
            };
            if checkouts.file_name().is_none_or(|name| name != "checkouts") {
                continue;
            }
            let Some(git_home) = checkouts
                .parent()
                .filter(|p| p.file_name().is_some_and(|name| name == "git"))
            else {
                continue;
            };
            let Some(name) = package.file_name() else {
                continue;
            };
            let Some(short) = anchor.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if short.len() < 7 || !short.bytes().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let Some(head) = git_strict(anchor, &["rev-parse", "--verify", "HEAD"], reads).await
            else {
                return false;
            };
            let head = head.trim();
            if !is_head(head) || !head.starts_with(short) {
                return false;
            }
            let Some(database) =
                unattended_dir(&git_home.join("db").join(name), &reads.home, &reads.table)
            else {
                return false;
            };
            let Some(origin) =
                git_strict(anchor, &["config", "--get-all", "remote.origin.url"], reads).await
            else {
                return false;
            };
            let Ok(url) = url::Url::parse(origin.trim()) else {
                return false;
            };
            if origin.lines().count() != 1 || url.query().is_some() || url.fragment().is_some() {
                return false;
            }
            let Ok(origin) = url.to_file_path() else {
                return false;
            };
            if unattended_dir(&origin, &reads.home, &reads.table).as_ref() != Some(&database) {
                return false;
            }
            let Some(database_path) = database.to_str() else {
                return false;
            };
            let git_dir = format!("--git-dir={database_path}");
            if git_strict(
                &database,
                &[&git_dir, "rev-parse", "--is-bare-repository"],
                reads,
            )
            .await
            .as_deref()
            .map(str::trim)
                != Some("true")
            {
                return false;
            }
            let reference = format!("refs/commit/{head}");
            if git_strict(
                &database,
                &[&git_dir, "rev-parse", "--verify", &reference],
                reads,
            )
            .await
            .as_deref()
            .map(str::trim)
                != Some(head)
                || git_strict(&database, &[&git_dir, "cat-file", "-t", head], reads)
                    .await
                    .as_deref()
                    .map(str::trim)
                    != Some("commit")
            {
                return false;
            }
            let Some(members) = graph(anchor, head, names, git_home, reads).await else {
                return false;
            };
            if !members.contains(candidate) || !live(reads) {
                return false;
            }
            // Charge anchors, member entries and ownership-name bytes globally. A
            // complete proof remains valid if there is no room to retain its memo.
            let cost = names
                .iter()
                .fold(1usize.checked_add(members.len()), |sum, name| {
                    sum.and_then(|sum| sum.checked_add(name.len().max(1)))
                });
            if let Some(total) = cost.and_then(|cost| self.stored.checked_add(cost))
                && total <= STATE_FILES
            {
                self.stored = total;
                self.entries.push(Observation {
                    anchor: anchor.to_path_buf(),
                    names: names.to_vec(),
                    members,
                });
            }
            return live(reads);
        }
        false
    }
}

async fn graph(
    anchor: &Path,
    head: &str,
    names: &[String],
    git_home: &Path,
    reads: &Reads,
) -> Option<HashSet<PathBuf>> {
    let mut queue = vec![(anchor.to_path_buf(), head.to_owned())];
    let mut seen = HashSet::from([anchor.to_path_buf()]);
    let mut at = 0;
    let mut budget = 1usize;
    while at < queue.len() {
        if !live(reads) {
            return None;
        }
        let (root, pin) = queue[at].clone();
        at += 1;
        reads.fs_step();
        let metadata = std::fs::symlink_metadata(root.join(".git")).ok()?;
        if !metadata.is_file() && !metadata.is_dir() {
            return None;
        }
        if git_strict(&root, &["rev-parse", "--verify", "HEAD"], reads)
            .await?
            .trim()
            != pin
        {
            return None;
        }
        let branch = git_strict(&root, &["symbolic-ref", "--quiet", "HEAD"], reads).await?;
        let branch = branch.trim();
        let name = branch.strip_prefix("refs/heads/")?;
        if names.iter().any(|owned| owned == name || owned == branch) {
            return None;
        }
        let refs = git_strict(
            &root,
            &["for-each-ref", "--format=%(refname) %(objectname)"],
            reads,
        )
        .await?;
        if git_strict(
            &root,
            &["config", "--bool", "--get", "core.logAllRefUpdates"],
            reads,
        )
        .await?
        .trim()
            != "true"
        {
            return None;
        }
        let initial = initial_log(&root, &pin, reads).await?;
        // Removed stash refs do not license ignoring their surviving history either.
        if !stash_log_absent(&root, reads).await? {
            return None;
        }
        let children = children(&root, reads).await?;
        let marker = root.join(".cargo-ok");
        reads.fs_step();
        let marker = open_regular(&marker)?.metadata().ok()?;
        if marker.len() != 0 {
            return None;
        }
        let baseline = initial.min(change_stamp(&marker)?);
        let mut head_seen = false;
        let mut tags = Vec::new();
        for line in refs.lines() {
            let (reference, oid) = line.split_once(' ')?;
            if reference == branch && oid == pin {
                head_seen = true;
            } else if reference.starts_with("refs/tags/upstream/") && is_head(oid) {
                budget = budget.checked_add(1)?;
                if budget > STATE_FILES {
                    return None;
                }
                tags.push((reference, oid));
            } else {
                return None;
            }
        }
        if !head_seen {
            return None;
        }
        if !tags.is_empty() {
            let database = tag_database(&root, git_home, reads).await?;
            for (reference, oid) in tags {
                if !imported_tag(&root, &database, reference, oid, baseline, reads).await? {
                    return None;
                }
            }
        }
        let status = git_strict(
            &root,
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--ignore-submodules=none",
            ],
            reads,
        )
        .await?;
        for entry in status.split('\0').filter(|e| !e.is_empty()) {
            budget = budget.checked_add(1)?;
            if budget > STATE_FILES {
                return None;
            }
            if entry == "?? .cargo-ok" {
                continue;
            }
            let (code, path) = (entry.get(..2)?, entry.get(3..)?);
            if code != " M"
                || !children
                    .iter()
                    .any(|child| child.0 == path && child.2.is_some())
            {
                return None;
            }
        }
        for (_, dir, pin) in children {
            if let Some(pin) = pin {
                budget = budget.checked_add(1)?;
                if budget > STATE_FILES || !seen.insert(dir.clone()) {
                    return None;
                }
                queue.push((dir, pin));
            }
        }
    }
    live(reads).then_some(seen)
}

async fn stash_log_absent(root: &Path, reads: &Reads) -> Option<bool> {
    let path = git_strict(root, &["rev-parse", "--git-path", "logs/refs/stash"], reads).await?;
    let path = root.join(path.trim());
    let resolved = crate::mounts::resolve_local_or_missing(&path, &reads.table).ok()?;
    reads.fs_step();
    Some(
        matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
            && resolved.is_none(),
    )
}

fn change_stamp(meta: &std::fs::Metadata) -> Option<(u64, u32)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        Some((
            meta.ctime().try_into().ok()?,
            meta.ctime_nsec().try_into().ok()?,
        ))
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

async fn initial_log(root: &Path, pin: &str, reads: &Reads) -> Option<(u64, u32)> {
    let path = git_strict(root, &["rev-parse", "--git-path", "logs/HEAD"], reads).await?;
    let file = open_git_file(root, &path, reads).ok()??;
    reads.fs_step();
    if !std::fs::symlink_metadata(root.join(path.trim()))
        .ok()?
        .is_file()
    {
        return None;
    }
    let metadata = file.metadata().ok()?;
    let changed = change_stamp(&metadata)?;
    if changed.0 == 0 || metadata.len() > INITIAL_LOG_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(INITIAL_LOG_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > INITIAL_LOG_BYTES {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    let line = lines.next()?;
    if lines.next().is_some() || reflog_time(line) != Some(changed.0) {
        return None;
    }
    let (meta, why) = line.split_once('\t')?;
    let mut fields = meta.split_whitespace();
    (fields.next()? == "0".repeat(pin.len())
        && fields.next()? == pin
        && why == format!("reset: moving to {pin}"))
    .then_some(changed)
}

async fn database_read(database: &Path, args: &[&str], reads: &Reads) -> Option<String> {
    let git_dir = format!("--git-dir={}", database.to_str()?);
    let mut exact = vec![git_dir.as_str()];
    exact.extend_from_slice(args);
    git_strict(database, &exact, reads).await
}

async fn tag_database(root: &Path, git_home: &Path, reads: &Reads) -> Option<PathBuf> {
    let origin = git_strict(root, &["config", "--get-all", "remote.origin.url"], reads).await?;
    let url = url::Url::parse(origin.trim()).ok()?;
    if origin.lines().count() != 1 || url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    let database = unattended_dir(&url.to_file_path().ok()?, &reads.home, &reads.table)?;
    let db_home = unattended_dir(&git_home.join("db"), &reads.home, &reads.table)?;
    if database.parent() != Some(db_home.as_path())
        || database_read(&database, &["rev-parse", "--is-bare-repository"], reads)
            .await?
            .trim()
            != "true"
    {
        return None;
    }
    Some(database)
}

async fn imported_tag(
    root: &Path,
    database: &Path,
    reference: &str,
    oid: &str,
    baseline: (u64, u32),
    reads: &Reads,
) -> Option<bool> {
    let path = git_strict(root, &["rev-parse", "--git-path", reference], reads).await?;
    let file = open_git_file(root, &path, reads).ok()??;
    reads.fs_step();
    if !std::fs::symlink_metadata(root.join(path.trim()))
        .ok()?
        .is_file()
    {
        return Some(false);
    }
    let metadata = file.metadata().ok()?;
    if metadata.len() > 128 || change_stamp(&metadata)? > baseline {
        return Some(false);
    }
    let mut bytes = Vec::new();
    file.take(129).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 128 || String::from_utf8(bytes).ok()?.trim() != oid {
        return Some(false);
    }
    if database_read(database, &["rev-parse", "--verify", reference], reads)
        .await?
        .trim()
        != oid
    {
        return Some(false);
    }
    let commit = format!("{reference}^{{commit}}");
    let local = git_strict(root, &["rev-parse", "--verify", &commit], reads).await?;
    let remote = database_read(database, &["rev-parse", "--verify", &commit], reads).await?;
    Some(is_head(local.trim()) && local.trim() == remote.trim())
}

/// Every index gitlink must have one valid registration. Only an explicitly update-none,
/// empty placeholder may lack its own repository; it is never a candidate graph member.
async fn children(root: &Path, reads: &Reads) -> Option<Vec<(String, PathBuf, Option<String>)>> {
    // Status may hide tracked edits behind assume-unchanged or skip-worktree bits. A factory
    // copy has ordinary index entries; any such suppression loses this positive proof.
    let flags = git_strict(root, &["ls-files", "-v", "-z"], reads).await?;
    if flags
        .split('\0')
        .filter(|e| !e.is_empty())
        .any(|entry| !entry.starts_with("H "))
    {
        return None;
    }
    let staged = git_strict(root, &["ls-files", "--stage", "-z"], reads).await?;
    let mut pins = Vec::new();
    for entry in staged.split('\0').filter(|e| !e.is_empty()) {
        let (meta, path) = entry.split_once('\t')?;
        if path == ".cargo-ok" {
            return None;
        }
        if let Some(rest) = meta.strip_prefix("160000 ") {
            let (pin, stage) = rest.split_once(' ')?;
            if !is_head(pin) || stage != "0" || pins.len() >= STATE_FILES {
                return None;
            }
            pins.push((path.to_owned(), pin.to_owned()));
        }
    }
    let modules = root.join(".gitmodules");
    reads.fs_step();
    match std::fs::symlink_metadata(&modules) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && pins.is_empty() => {
            return Some(Vec::new());
        }
        Ok(meta) if meta.is_file() => {}
        _ => return None,
    }
    let config = git_strict(
        root,
        &["config", "-z", "--file", ".gitmodules", "--list"],
        reads,
    )
    .await?;
    let entries: Vec<_> = config
        .split('\0')
        .filter(|e| !e.is_empty())
        .map(|e| e.split_once('\n'))
        .collect::<Option<_>>()?;
    let mut children: Vec<(String, PathBuf, Option<String>)> = Vec::new();
    for (key, path) in &entries {
        let Some(section) = key
            .strip_prefix("submodule.")
            .and_then(|k| k.strip_suffix(".path"))
        else {
            continue;
        };
        if path.is_empty()
            || Path::new(path)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || children.len() >= STATE_FILES
        {
            return None;
        }
        let (_, pin) = pins.iter().find(|(registered, _)| registered == *path)?;
        let dir = unattended_dir(&root.join(path), &reads.home, &reads.table)?;
        if dir == root
            || !dir.starts_with(root)
            || children.iter().any(|(p, d, _)| p == *path || *d == dir)
        {
            return None;
        }
        reads.fs_step();
        let pin = match std::fs::symlink_metadata(dir.join(".git")) {
            Ok(meta) if meta.is_file() || meta.is_dir() => Some(pin.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let update = format!("submodule.{section}.update");
                if !entries
                    .iter()
                    .any(|(key, value)| *key == update && *value == "none")
                    || std::fs::read_dir(&dir).ok()?.next().is_some()
                {
                    return None;
                }
                None
            }
            _ => return None,
        };
        children.push(((*path).to_owned(), dir, pin));
    }
    if pins
        .iter()
        .any(|(path, _)| !children.iter().any(|child| child.0 == *path))
    {
        return None;
    }
    Some(children)
}
