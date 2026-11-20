// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Reuse native preparations that already occupied build numbers in this state,
//! including campaigns created by the manual publisher before the scheduler.
use super::Pin;
use super::Ready;
use super::Reservation;
use crate::Release;
use crate::Result;
use crate::lease::Lease;
use crate::source::Checkout;
use crate::workflow::Campaign;
use std::collections::BTreeMap;
use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

/// The stable state directory is the common allocation lock. Its existing numbered
/// campaign directories and schedule reservations remain the durable ownership records.
pub(crate) struct Allocation {
    pub(crate) state: PathBuf,
    _lease: Lease,
}

impl Allocation {
    pub(crate) fn acquire(state: &Path) -> Result<Self> {
        let info = crate::source::owned(state)?;
        let state = state.canonicalize()?;
        let directory = File::options()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&state)?;
        let opened = directory.metadata()?;
        if (opened.dev(), opened.ino()) != (info.dev(), info.ino()) {
            return Err("release allocation directory changed".into());
        }
        Ok(Self {
            state,
            _lease: Lease::acquire(directory)?,
        })
    }

    pub(crate) fn check_number(&self, release: &Release, number: u64) -> Result<()> {
        let wanted = self.state.join(release.id()?);
        for (root, build, _) in publications(&self.state)? {
            if build == number && root != wanted {
                return Err(
                    format!("package build {number} belongs to another native campaign").into(),
                );
            }
        }
        let journal = self.state.join("regular-schedule");
        match journal.symlink_metadata() {
            Ok(_) => super::check_private_directory(&journal)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        }
        for reservation in super::reservations(&journal)? {
            if reservation.build == number && reservation.release != *release {
                return Err(
                    format!("package build {number} is reserved for another source").into(),
                );
            }
        }
        Ok(())
    }
}

fn publications(state: &Path) -> Result<Vec<(PathBuf, u64, PathBuf)>> {
    let mut entries = Vec::new();
    for child in state.read_dir()? {
        let child = child?;
        let name = child.file_name();
        let hex = name.as_encoded_bytes();
        if hex.len() != 64
            || !hex
                .iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        {
            continue;
        }
        let root = child.path();
        if !child.file_type()?.is_dir() {
            return Err("native release campaign must be a real directory".into());
        }
        for entry in root.read_dir()? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(suffix) = name.to_str().and_then(|n| n.strip_prefix("publication-")) else {
                continue;
            };
            let build = suffix.parse::<u64>()?;
            if suffix != build.to_string() || !entry.file_type()?.is_dir() {
                return Err("invalid native release reservation".into());
            }
            entries.push((root.clone(), build, entry.path()));
        }
    }
    Ok(entries)
}

pub(super) fn recover(
    repo: &Path,
    publisher: &Path,
    state: &Path,
    journal: &Path,
    current: &Release,
    pin: &Pin,
) -> Result<u64> {
    Ok(reconcile(repo, publisher, state, journal, current, pin)?.1)
}

pub(super) fn reconcile(
    repo: &Path,
    publisher: &Path,
    state: &Path,
    journal: &Path,
    current: &Release,
    pin: &Pin,
) -> Result<(Allocation, u64)> {
    let allocation = Allocation::acquire(state)?;
    let reserved = journal.join(format!("reservation-{}.json", current.version));
    // Detect historical conflicts even when a later signed pin or an existing
    // version reservation would otherwise skip reopening these campaigns.
    let mut owners = BTreeMap::new();
    let mut record_owner = |number, root: PathBuf| -> Result<()> {
        if let Some(prior) = owners.get(&number) {
            if *prior != root {
                return Err(
                    format!("package build {number} belongs to two release intents").into(),
                );
            }
        } else {
            owners.insert(number, root);
        }
        Ok(())
    };
    let mut frozen = false;
    for record in super::reservations(journal)? {
        record_owner(record.build, allocation.state.join(record.release.id()?))?;
        frozen |= record.release.version == current.version;
    }
    let publications = publications(&allocation.state)?;
    for (root, build, _) in &publications {
        record_owner(*build, root.clone())?;
    }
    let highest = owners
        .keys()
        .next_back()
        .copied()
        .unwrap_or(0)
        .max(pin.build);
    if frozen {
        // A separately numbered manual campaign still owns its number. It cannot
        // replace the original source or observations chosen for the regular release.
        return Ok((allocation, highest));
    }
    let mut candidate = None;
    for (root, build, path) in publications {
        if build <= pin.build || crate::line(&pin.version)? >= crate::line(&current.version)? {
            continue;
        }
        if !path.join("intent.json").try_exists()? {
            // The original owner had not yet durably bound its producer
            // and source. The directory still occupies its build number.
            continue;
        }
        let prior = Checkout::release_at(repo, &root)?;
        if prior.version != current.version {
            continue;
        }
        // Reopen under the original executable, source, directory and lease
        // checks. A live manual owner keeps its lease until its process drains.
        let campaign = Campaign::open_allocated(repo, &prior, &allocation, build, publisher)?;
        match (campaign.stage("product")?, campaign.stage("ripgrep")?) {
            (Some(product), Some(ripgrep)) => {
                if candidate.is_some() {
                    return Err("multiple completed manual campaigns qualify for the same regular release version".into());
                }
                candidate = Some(Reservation {
                    schema: 1,
                    build,
                    release: prior,
                    ready: Ready { product, ripgrep },
                });
            }
            _ if path.join("native-publication").try_exists()? => {
                return Err(
                    "native signing state exists without its two original build receipts".into(),
                );
            }
            // No external publication was started. Keep the build occupied;
            // a newly qualified source gets a different number.
            _ => {}
        }
    }
    if let Some(record) = candidate {
        crate::save_record(&reserved, &serde_json::to_vec(&record)?)?;
    }
    Ok((allocation, highest))
}
