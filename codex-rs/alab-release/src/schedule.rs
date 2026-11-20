// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Reservations for the worker already authorized to run regular publications.
//! The publisher owns the signed channel. The journal only remembers which
//! immutable arguments to retry after an interrupted build or acknowledgement.
use crate::Release;
use crate::Result;
use crate::lease::Lease;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::fs::DirBuilder;
use std::fs::File;
use std::fs::OpenOptions;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

#[path = "schedule_existing.rs"]
mod existing;
pub(crate) use existing::Allocation;

const PROJECT: &str = "alabsystems/codex-alab";
const TARGET: &str = "aarch64-apple-darwin";

fn output(command: &mut Command) -> Result<String> {
    let result = command.output()?;
    if !result.status.success() {
        return Err(format!(
            "{} failed ({}): {}",
            command.get_program().to_string_lossy(),
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8(result.stdout)?)
}

fn registry_standing(text: &str) -> Result<()> {
    let registry: toml::Value = toml::from_str(text)?;
    let row = registry
        .get("repos")
        .and_then(|rows| rows.get("codex-alab"))
        .ok_or("codex-alab absent from the publication registry")?;
    for (field, expected) in [("dev", PROJECT), ("release", "alabsystems/codex-alab")] {
        let url = row
            .get(field)
            .and_then(toml::Value::as_str)
            .ok_or("registry release route is missing")?;
        if crate::github_repository(url)? != expected {
            return Err(
                format!("registered Codex {field} repository moved away from {expected}").into(),
            );
        }
    }
    let active = match registry.get("lifecycle") {
        None => true,
        Some(map) => match map
            .as_table()
            .ok_or("malformed lifecycle table")?
            .get("codex-alab")
        {
            None => true,
            Some(entry) => entry.get("state").and_then(toml::Value::as_str) == Some("active"),
        },
    };
    if !active
        || row
            .get("public_release")
            .is_some_and(|v| v.as_bool() != Some(true))
        || row.get("released_by").and_then(toml::Value::as_str) != Some("codex-release")
        || row
            .get("atpkg")
            .and_then(|v| v.get("program"))
            .and_then(toml::Value::as_str)
            != Some("codex")
        || row
            .get("atpkg")
            .and_then(|v| v.get("recipe"))
            .and_then(toml::Value::as_str)
            != Some("package")
    {
        return Err(
            "codex-alab has no active open regular package release delegated to codex-release"
                .into(),
        );
    }
    Ok(())
}

/// The development repository carries its publishing policy. A public source
/// snapshot can build the controller without carrying a release authority.
fn registry_reference(policy: &str) -> Result<String> {
    let (source_owner, _) = PROJECT.split_once('/').ok_or("invalid source repository")?;
    let registry = policy.trim_end();
    if registry.split_once('/') != Some((source_owner, "publication")) {
        return Err("release policy must name the source owner's publication registry".into());
    }
    Ok(format!("{registry}@main:registry/repos.toml"))
}

fn authority(repo: &Path) -> Result<()> {
    if crate::repository(repo)? != PROJECT {
        return Err(
            "scheduled releases require the registered codex-alab source repository".into(),
        );
    }
    let policy = output(Command::new("agh").args([
        "cat",
        &format!("{PROJECT}@main:publish/registry-repository"),
        "--max-bytes",
        "256",
    ]))?;
    let registry = registry_reference(&policy)?;
    registry_standing(&output(Command::new("agh").args([
        "cat",
        &registry,
        "--max-bytes",
        "4194304",
    ]))?)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pin {
    version: String,
    build: u64,
}

fn signed_pin(publisher: &Path) -> Result<Pin> {
    let report =
        output(Command::new(publisher).args(["--host", "agh", "status", "--releases", "codex"]))?;
    parse_pin(&report)
}

fn parse_pin(report: &str) -> Result<Pin> {
    let mut pin = None;
    let mut rows = report
        .lines()
        .filter_map(|line| line.strip_prefix("atpkg-publish status: "));
    while let Some(row) = rows.next() {
        if row.starts_with("LAPSED") {
            return Err("the signed channel has lapsed".into());
        }
        let fields: Vec<_> = row.split(" | ").collect();
        if fields.first() != Some(&"codex-alab") {
            continue;
        }
        let detail = rows.next().ok_or("missing signed release detail")?;
        let Some(detail) = detail.strip_prefix("  program codex; channel stable; build ") else {
            continue;
        };
        if fields.len() != 5 || fields[3] != TARGET {
            continue;
        }
        if fields[4] != "Public release metadata; payload not checked" {
            return Err(format!("Codex pin on the signed channel: {}", fields[4]).into());
        }
        crate::check_product_version(fields[1])?;
        let (build, index) = detail
            .split_once("; index ")
            .ok_or("malformed channel report")?;
        let build = build.parse::<u64>()?;
        if build == 0 || index.parse::<u64>()? == 0 || pin.is_some() {
            return Err("ambiguous or invalid stable Codex channel report".into());
        }
        pin = Some(Pin {
            version: fields[1].to_owned(),
            build,
        });
    }
    pin.ok_or_else(|| "no verified regular stable Codex package for this target".into())
}

fn current_release(repo: &Path) -> Result<Release> {
    let fetched = output(
        Command::new("agh")
            .args(["repo", "fetch-main", PROJECT])
            .arg(repo),
    )?;
    let main = crate::fetched_main(repo, fetched.as_bytes())?;
    Release::from_commit(repo, &main, TARGET)
}

pub fn check_dispatch(repo: &Path, publisher: &Path, release: &Release, build: u64) -> Result<()> {
    authority(repo)?;
    let current = current_release(repo)?;
    if current.version != release.version {
        return Err("registered main has moved on to another release version".into());
    }
    crate::git(
        repo,
        &[
            "merge-base",
            "--is-ancestor",
            &release.source,
            &current.source,
        ],
    )?;
    let pin = signed_pin(publisher)?;
    let older = crate::line(&pin.version)? < crate::line(&release.version)?;
    if !(older && pin.build < build || pin.version == release.version && pin.build == build) {
        return Err(
            "a different signed release already occupies or supersedes this reservation".into(),
        );
    }
    Ok(())
}

/// Content-addressed local build observations. The ordinary native publication
/// workflow revalidates them against the frozen intent and binaries.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ready {
    pub product: PathBuf,
    pub ripgrep: PathBuf,
}

impl Ready {
    pub fn check(&self) -> Result<()> {
        for (name, path) in [("build", &self.product), ("ripgrep", &self.ripgrep)] {
            let expected = format!("{name}-{}.json", crate::product::file_hash(path)?);
            if path.file_name().and_then(|s| s.to_str()) != Some(expected.as_str()) {
                return Err("retained build observation changed after qualification".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Producer {
    path: PathBuf,
    sha256: String,
}

impl Producer {
    fn observe(publisher: &Path) -> Result<Self> {
        let path = publisher.canonicalize()?;
        let metadata = path.metadata()?;
        if !metadata.is_file() || metadata.mode() & 0o111 == 0 {
            return Err("the scheduled publisher must be an executable file".into());
        }
        let sha256 = crate::product::file_hash(&path)?;
        Ok(Self { path, sha256 })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProductCheckpoint {
    schema: u32,
    intent: String,
    publisher: Producer,
    product_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReadyCheckpoint {
    schema: u32,
    intent: String,
    publisher: Producer,
    product_sha256: String,
    ripgrep_sha256: String,
}

fn checkpoint_bytes(marker: &Path) -> Result<Option<Vec<u8>>> {
    match marker.symlink_metadata() {
        Ok(metadata) => {
            if metadata.mode() & 0o077 != 0 || metadata.nlink() != 1 {
                return Err("scheduled checkpoint must be a private single-link file".into());
            }
            Ok(Some(crate::record_bytes(marker)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// The scheduler lease protects these pre-reservation checkpoints. Each marker
/// refers only to a content-addressed record in this exact source campaign.
struct Qualification {
    intent: String,
    journal: PathBuf,
    campaign: PathBuf,
    producer: Producer,
}

impl Qualification {
    fn open(state: &Path, release: &Release, publisher: &Path) -> Result<Self> {
        let intent = release.id()?;
        let state = state.canonicalize()?;
        let journal = state.join("regular-schedule");
        check_private_directory(&journal)?;
        Ok(Self {
            campaign: state.join(&intent),
            journal,
            intent,
            producer: Producer::observe(publisher)?,
        })
    }

    fn marker(&self, name: &str) -> PathBuf {
        self.journal.join(format!("{name}-{}.json", self.intent))
    }

    fn check_binding(&self, schema: u32, intent: &str, producer: &Producer) -> Result<()> {
        if schema != 1 || intent != self.intent || producer != &self.producer {
            return Err("scheduled checkpoint has another intent or publisher".into());
        }
        Ok(())
    }

    fn check_publisher(&self, publisher: &Path) -> Result<()> {
        if Producer::observe(publisher)? != self.producer {
            return Err("scheduled publisher changed during qualification".into());
        }
        Ok(())
    }

    fn digest(&self, name: &str, path: &Path) -> Result<String> {
        if path.parent() != Some(self.campaign.as_path()) {
            return Err("observation belongs to another release campaign".into());
        }
        crate::source::owned(&self.campaign)?;
        let digest = format!("{:x}", Sha256::digest(crate::record_bytes(path)?));
        if path != self.campaign.join(format!("{name}-{digest}.json")) {
            return Err("scheduled observation differs from its content address".into());
        }
        Ok(digest)
    }

    fn receipt(&self, name: &str, digest: &str) -> Result<PathBuf> {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid scheduled observation digest".into());
        }
        let path = self.campaign.join(format!("{name}-{digest}.json"));
        self.digest(name, &path)?;
        Ok(path)
    }

    fn product(
        &self,
        publisher: &Path,
        build: impl FnOnce() -> Result<PathBuf>,
    ) -> Result<(PathBuf, String)> {
        let marker = self.marker("product");
        let (bytes, path, digest) = match checkpoint_bytes(&marker)? {
            Some(bytes) => {
                let saved: ProductCheckpoint = serde_json::from_slice(&bytes)?;
                self.check_binding(saved.schema, &saved.intent, &saved.publisher)?;
                let path = self.receipt("build", &saved.product_sha256)?;
                (bytes, path, saved.product_sha256)
            }
            None => {
                let path = build()?;
                let digest = self.digest("build", &path)?;
                let bytes = serde_json::to_vec(&ProductCheckpoint {
                    schema: 1,
                    intent: self.intent.clone(),
                    publisher: self.producer.clone(),
                    product_sha256: digest.clone(),
                })?;
                (bytes, path, digest)
            }
        };
        self.check_publisher(publisher)?;
        crate::save_record(&marker, &bytes)?;
        Ok((path, digest))
    }
}

#[cfg(test)]
fn checkpoint_product(
    state: &Path,
    release: &Release,
    publisher: &Path,
    build: impl FnOnce() -> Result<PathBuf>,
) -> Result<PathBuf> {
    let journal = Qualification::open(state, release, publisher)?;
    Ok(journal.product(publisher, build)?.0)
}

fn qualify_stages(
    state: &Path,
    release: &Release,
    publisher: &Path,
    build: impl FnOnce() -> Result<PathBuf>,
    build_ripgrep: impl FnOnce(&Path) -> Result<PathBuf>,
    check_outputs: impl FnOnce(&Ready) -> Result<()>,
    check_owner: impl Fn() -> Result<()>,
) -> Result<Ready> {
    let journal = Qualification::open(state, release, publisher)?;
    let marker = journal.marker("ready");
    let retained = checkpoint_bytes(&marker)?;
    let (product, product_sha256) = journal.product(publisher, || {
        if retained.is_some() {
            return Err("completed qualification lost its product checkpoint".into());
        }
        build()
    })?;
    check_owner()?;
    let (bytes, ready) = match retained {
        Some(bytes) => {
            let saved: ReadyCheckpoint = serde_json::from_slice(&bytes)?;
            journal.check_binding(saved.schema, &saved.intent, &saved.publisher)?;
            if saved.product_sha256 != product_sha256 {
                return Err("completed qualification names another product".into());
            }
            let ripgrep = journal.receipt("ripgrep", &saved.ripgrep_sha256)?;
            let ready = Ready { product, ripgrep };
            check_outputs(&ready)?;
            (bytes, ready)
        }
        None => {
            let ripgrep = build_ripgrep(&product)?;
            let bytes = serde_json::to_vec(&ReadyCheckpoint {
                schema: 1,
                intent: journal.intent.clone(),
                publisher: journal.producer.clone(),
                product_sha256,
                ripgrep_sha256: journal.digest("ripgrep", &ripgrep)?,
            })?;
            (bytes, Ready { product, ripgrep })
        }
    };
    journal.check_publisher(publisher)?;
    crate::save_record(&marker, &bytes)?;
    check_owner()?;
    Ok(ready)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Reservation {
    schema: u32,
    build: u64,
    release: Release,
    ready: Ready,
}

fn private_directory(dir: &Path) -> Result<()> {
    match DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => File::open(dir.parent().ok_or("missing parent")?)?.sync_all()?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    check_private_directory(dir)
}

fn check_private_directory(dir: &Path) -> Result<()> {
    let info = dir.symlink_metadata()?;
    // SAFETY: geteuid has no preconditions.
    if !info.is_dir() || info.uid() != unsafe { libc::geteuid() } || info.mode() & 0o077 != 0 {
        return Err(format!(
            "release schedule requires an owned private directory: {}",
            dir.display()
        )
        .into());
    }
    Ok(())
}

fn acquire(dir: &Path) -> Result<Lease> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(dir.join("worker.lock"))?;
    let info = file.metadata()?;
    // SAFETY: geteuid has no preconditions.
    if !info.is_file()
        || info.nlink() != 1
        || info.uid() != unsafe { libc::geteuid() }
        || info.mode() & 0o077 != 0
    {
        return Err("invalid regular release worker lease".into());
    }
    Ok(Lease::acquire(file)?)
}

fn reservations(dir: &Path) -> Result<Vec<Reservation>> {
    let mut records = Vec::new();
    for entry in dir.read_dir()? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().ok_or("invalid release journal filename")?;
        let Some(version) = name
            .strip_prefix("reservation-")
            .and_then(|n| n.strip_suffix(".json"))
        else {
            continue;
        };
        let record: Reservation = serde_json::from_slice(&crate::record_bytes(&entry.path())?)?;
        if record.schema != 1 || record.release.version != version || record.build == 0 {
            return Err("invalid release reservation".into());
        }
        crate::check_product_version(version)?;
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
fn reserve(
    dir: &Path,
    release: &Release,
    pin: &Pin,
    day: u64,
    qualify: impl FnOnce(&Path) -> Result<(Ready, Pin, u64)>,
) -> Result<Option<Reservation>> {
    reserve_with_allocation(dir, release, pin, || Ok(day), qualify, |_| Ok(((), 0)))
}

fn reserve_with_allocation<G>(
    dir: &Path,
    release: &Release,
    pin: &Pin,
    utc_day: impl FnOnce() -> Result<u64>,
    qualify: impl FnOnce(&Path) -> Result<(Ready, Pin, u64)>,
    reconcile: impl FnOnce(&Pin) -> Result<(G, u64)>,
) -> Result<Option<Reservation>> {
    let records = reservations(dir)?;
    let mut highest = records
        .iter()
        .map(|r| r.build)
        .max()
        .unwrap_or(0)
        .max(pin.build);
    let existing = records
        .into_iter()
        .find(|r| r.release.version == release.version);
    let wanted = crate::line(&release.version)?;
    let signed = crate::line(&pin.version)?;
    if wanted < signed || wanted == signed && existing.as_ref().is_none_or(|v| v.build != pin.build)
    {
        return Ok(None);
    }
    if let Some(existing) = existing {
        if existing.build < pin.build {
            return Err("signed channel moved past the reserved package build".into());
        }
        return Ok(Some(existing));
    }
    let intent = dir.join(format!("source-{}.json", release.id()?));
    release.save(&intent)?;
    let (ready, latest, occupied) = qualify(&intent)?;
    if latest.build < pin.build {
        return Err("signed package build moved backwards during qualification".into());
    }
    if crate::line(&latest.version)? >= wanted {
        return Ok(None);
    }
    // Neither signed-channel reads nor qualification holds the allocation lock. Manual
    // registration and this final scan plus durable reservation use the same lock.
    let (_allocation, manual_floor) = reconcile(&latest)?;
    for record in reservations(dir)? {
        highest = highest.max(record.build);
        if record.release.version == release.version {
            if record.build < latest.build {
                return Err("signed channel moved past the recovered package build".into());
            }
            return Ok(Some(record));
        }
    }
    highest = highest.max(latest.build).max(occupied).max(manual_floor);
    let day = utc_day()?;
    let first = day.checked_mul(100).ok_or("invalid UTC date")?;
    let next = first
        .checked_add(1)
        .ok_or("invalid UTC date")?
        .max(highest.checked_add(1).ok_or("package build overflow")?);
    if next / 100 != day {
        return Err("UTC date has no available monotonic package build".into());
    }
    let record = Reservation {
        schema: 1,
        ready,
        build: next,
        release: release.clone(),
    };
    crate::save_record(
        &dir.join(format!("reservation-{}.json", release.version)),
        &serde_json::to_vec(&record)?,
    )?;
    Ok(Some(record))
}

pub fn cycle(
    repo: &Path,
    publisher: &Path,
    state: &Path,
    build: impl FnOnce(&Path) -> Result<PathBuf>,
    ripgrep: impl FnOnce(&Path, &Path) -> Result<PathBuf>,
    check_owner: impl Fn() -> Result<()>,
    publish: impl FnOnce(PathBuf, u64, &Ready) -> Result<()>,
) -> Result<()> {
    authority(repo)?;
    // Older native state directories can be group-readable. The new journal
    // and all of its inputs remain owner-only.
    if state.try_exists()? {
        crate::source::owned(state)?;
    } else {
        private_directory(state)?;
    }
    let dir = state.join("regular-schedule");
    private_directory(&dir)?;
    let _lease = acquire(&dir)?;
    let pin = signed_pin(publisher)?;
    let release = current_release(repo)?;
    let occupied = existing::recover(repo, publisher, state, &dir, &release, &pin)?;
    dispatch_with_allocation(
        &dir,
        &release,
        &pin,
        || {
            let day = output(Command::new("/bin/date").args(["-u", "+%Y%m%d"]))?;
            Ok(day.trim().parse()?)
        },
        |intent| {
            let ready = qualify_stages(
                state,
                &release,
                publisher,
                || build(intent),
                |product| ripgrep(intent, product),
                |ready| ready.check_outputs(repo, &release, state),
                check_owner,
            )?;
            authority(repo)?;
            let current = current_release(repo)?;
            if current.version != release.version {
                return Err("main moved on to another regular version during qualification".into());
            }
            crate::git(
                repo,
                &[
                    "merge-base",
                    "--is-ancestor",
                    &release.source,
                    &current.source,
                ],
            )?;
            Ok((ready, signed_pin(publisher)?, occupied))
        },
        |latest| existing::reconcile(repo, publisher, state, &dir, &release, latest),
        |reserved, intent| {
            crate::git(
                repo,
                &[
                    "merge-base",
                    "--is-ancestor",
                    &reserved.release.source,
                    &release.source,
                ],
            )?;
            reserved.release.check_local(repo)?;
            check_dispatch(repo, publisher, &reserved.release, reserved.build)?;
            publish(intent, reserved.build, &reserved.ready)
        },
    )
}

#[cfg(test)]
fn dispatch(
    dir: &Path,
    release: &Release,
    pin: &Pin,
    day: u64,
    qualify: impl FnOnce(&Path) -> Result<(Ready, Pin, u64)>,
    publish: impl FnOnce(&Reservation, PathBuf) -> Result<()>,
) -> Result<()> {
    dispatch_with_allocation(
        dir,
        release,
        pin,
        || Ok(day),
        qualify,
        |_| Ok(((), 0)),
        publish,
    )
}

fn dispatch_with_allocation<G>(
    dir: &Path,
    release: &Release,
    pin: &Pin,
    utc_day: impl FnOnce() -> Result<u64>,
    qualify: impl FnOnce(&Path) -> Result<(Ready, Pin, u64)>,
    reconcile: impl FnOnce(&Pin) -> Result<(G, u64)>,
    publish: impl FnOnce(&Reservation, PathBuf) -> Result<()>,
) -> Result<()> {
    let Some(reservation) =
        reserve_with_allocation(dir, release, pin, utc_day, qualify, reconcile)?
    else {
        println!(
            "Signed Codex {} build {} already covers source VERSION {}.",
            pin.version, pin.build, release.version
        );
        return Ok(());
    };
    let done = dir.join(format!("published-{}.json", reservation.build));
    let binding = serde_json::to_vec(&reservation)?;
    if done.try_exists()? {
        if crate::record_bytes(&done)? != binding
            || pin.build != reservation.build
            || pin.version != reservation.release.version
        {
            return Err(
                "current signed channel disagrees with the completed release reservation".into(),
            );
        }
        return Ok(());
    }
    let intent = dir.join(format!("intent-{}.json", reservation.build));
    reservation.release.save(&intent)?;
    publish(&reservation, intent)?;
    crate::save_record(&done, &binding)
}

#[cfg(test)]
#[path = "schedule_tests.rs"]
mod tests;
