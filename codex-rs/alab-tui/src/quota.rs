// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Weekly usage observations, scoped to the native account and the provider's reset cycle.
//! A graph plots reported observations in time, and an estimate uses elapsed time rather
//! than sample count. Nothing here invents usage while the application was not observing.

use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

const MAX_SAMPLES: usize = 2048;
const SAMPLE_BUCKET_SECONDS: i64 = 5 * 60;
const MIN_ESTIMATE_SECONDS: i64 = 30 * 60;
const MAX_SAMPLE_AGE_SECONDS: i64 = 30 * 60;
const MAX_FILE_BYTES: u64 = 512 * 1024;

/// An authoritative native usage reading. All times are Unix seconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeeklySnapshot {
    pub used_percent: f64,
    pub observed_at: i64,
    pub resets_at: i64,
    pub window_minutes: i64,
}

impl WeeklySnapshot {
    fn changes_segment(self, previous: Self) -> bool {
        self.resets_at != previous.resets_at
            || self.window_minutes != previous.window_minutes
            || self.used_percent < previous.used_percent
    }

    fn valid(self) -> bool {
        self.used_percent.is_finite()
            && (0.0..=100.0).contains(&self.used_percent)
            && self.observed_at >= 0
            && (1..=31 * 24 * 60).contains(&self.window_minutes)
            && self.resets_at > self.observed_at
            && self.resets_at.saturating_sub(self.observed_at) <= self.window_minutes * 60
    }
}

/// A current-cycle forecast based on observed consumption, always presented as an estimate.
#[derive(Clone, Debug, PartialEq)]
pub struct Projection {
    pub percent_per_day: f64,
    pub projected_percent_at_reset: f64,
    pub exhausted_at: Option<i64>,
}

/// A view of one account's current reset cycle. Missing graph buckets mean no observation.
#[derive(Clone, Debug, PartialEq)]
pub struct Weekly {
    pub used_percent: f64,
    pub observed_at: i64,
    pub resets_at: i64,
    pub window_minutes: i64,
    pub sample_count: usize,
    pub projection: Option<Projection>,
    samples: Vec<WeeklySnapshot>,
}

impl Weekly {
    /// Keep persisted history only when its latest reading is the accepted native
    /// observation. A different cycle, window or reading gets a fresh view without
    /// borrowing the stored history's forecast.
    pub fn for_observation(sample: WeeklySnapshot, persisted: Option<Self>) -> Option<Self> {
        if !sample.valid() {
            return None;
        }
        if let Some(weekly) = persisted.filter(|weekly| weekly.samples.last() == Some(&sample)) {
            return Some(weekly.at(sample.observed_at));
        }
        let mut history = History::default();
        history.observe(sample);
        history.view(sample.observed_at)
    }

    /// Re-evaluate freshness without recording a new observation or writing to disk.
    pub fn at(&self, now: i64) -> Self {
        let mut current = self.clone();
        current.projection = projection(&current.samples, now);
        current
    }

    /// Plot actual readings in equal time buckets over the provider's reported cycle.
    /// Dots are unobserved intervals, including the part of the cycle still in the future.
    pub fn sparkline(&self, columns: usize) -> String {
        let columns = columns.min(80);
        if columns == 0 {
            return String::new();
        }
        let levels = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let mut cells = vec!['·'; columns];
        let duration = self.window_minutes * 60;
        let start = self.resets_at.saturating_sub(duration);
        for sample in &self.samples {
            let elapsed = sample.observed_at.saturating_sub(start).max(0);
            let column = ((elapsed as u128 * columns as u128) / duration as u128)
                .min(columns.saturating_sub(1) as u128) as usize;
            let level = ((sample.used_percent / 100.0 * 7.0).round() as usize).min(7);
            cells[column] = levels[level];
        }
        cells.into_iter().collect()
    }
}

/// Bounded observations from exactly one reset cycle. Account identity belongs to Store.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    samples: Vec<WeeklySnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    segment_start: Option<i64>,
    #[serde(skip)]
    pending: Vec<WeeklySnapshot>,
    #[serde(skip)]
    acknowledged: Vec<WeeklySnapshot>,
}

impl History {
    /// Record an accepted observation; reject invalid and out-of-order readings.
    /// A reset, changed window, or downward correction starts a separate history.
    /// Reads in the same second replace the latest value without adding an anchor.
    pub fn observe(&mut self, sample: WeeklySnapshot) -> bool {
        if !sample.valid() {
            return false;
        }
        if let Some(last) = self.samples.last().copied() {
            if sample.observed_at < last.observed_at || sample == last {
                return false;
            }
            // Accepted backend corrections can move a future reset either way.
            // Start over rather than retaining a cycle the native display replaced.
            if sample.changes_segment(last) {
                self.samples.clear();
                self.pending.clear();
                self.segment_start = Some(sample.observed_at);
            }
        }
        Self::push(&mut self.samples, sample);
        Self::push(&mut self.pending, sample);
        true
    }

    fn push(samples: &mut Vec<WeeklySnapshot>, sample: WeeklySnapshot) {
        if samples.last().is_some_and(|last| {
            sample.observed_at == last.observed_at
                || (samples.len() >= 2
                    && sample.observed_at / SAMPLE_BUCKET_SECONDS
                        == last.observed_at / SAMPLE_BUCKET_SECONDS)
        }) {
            // Whole-second timestamps can represent distinct accepted backend reads.
            // Keep one value per timestamp, and coalesce later five-minute bins.
            samples.pop();
        }
        samples.push(sample);
        let excess = samples.len().saturating_sub(MAX_SAMPLES);
        samples.drain(..excess);
    }

    /// Current usage and the estimate supported by these observations at `now`.
    pub fn view(&self, now: i64) -> Option<Weekly> {
        let latest = self.samples.last()?;
        Some(Weekly {
            used_percent: latest.used_percent,
            observed_at: latest.observed_at,
            resets_at: latest.resets_at,
            window_minutes: latest.window_minutes,
            sample_count: self.samples.len(),
            projection: projection(&self.samples, now),
            samples: self.samples.clone(),
        })
    }

    /// Whether this history is `stored` but for its latest reading's time, moved within one
    /// five-minute bucket with the same usage, reset and window.
    fn only_retimed(&self, stored: &Self) -> bool {
        self.segment_start == stored.segment_start
            && match (self.samples.split_last(), stored.samples.split_last()) {
                (Some((latest, earlier)), Some((was, stored_earlier))) => {
                    earlier == stored_earlier
                        && latest.observed_at > was.observed_at
                        && latest.observed_at / SAMPLE_BUCKET_SECONDS
                            == was.observed_at / SAMPLE_BUCKET_SECONDS
                        && WeeklySnapshot {
                            observed_at: was.observed_at,
                            ..*latest
                        } == *was
                }
                _ => false,
            }
    }

    fn valid(&self) -> bool {
        if self.samples.len() > MAX_SAMPLES
            || self.samples.iter().any(|sample| !sample.valid())
            || self.segment_start.is_some_and(|start| {
                start < 0
                    || self
                        .samples
                        .first()
                        .is_none_or(|first| first.observed_at < start)
            })
        {
            return false;
        }
        self.samples.windows(2).all(|pair| {
            pair[0].observed_at < pair[1].observed_at
                && pair[0].resets_at == pair[1].resets_at
                && pair[0].window_minutes == pair[1].window_minutes
                && pair[0].used_percent <= pair[1].used_percent
        })
    }
}

fn projection(samples: &[WeeklySnapshot], now: i64) -> Option<Projection> {
    let first = samples.first()?;
    let last = samples.last()?;
    let elapsed = last.observed_at - first.observed_at;
    if samples.len() < 3
        || elapsed < MIN_ESTIMATE_SECONDS
        || now < last.observed_at
        || now >= last.resets_at
        || now - last.observed_at > MAX_SAMPLE_AGE_SECONDS
    {
        return None;
    }
    let rate = (last.used_percent - first.used_percent) / elapsed as f64;
    let projected_percent_at_reset =
        last.used_percent + rate * (last.resets_at - last.observed_at) as f64;
    let exhausted_at = if rate > 0.0 {
        let seconds = ((100.0 - last.used_percent) / rate).ceil();
        (seconds <= (last.resets_at - last.observed_at) as f64)
            .then(|| last.observed_at.saturating_add(seconds as i64))
    } else {
        None
    };
    Some(Projection {
        percent_per_day: rate * 24.0 * 60.0 * 60.0,
        projected_percent_at_reset,
        exhausted_at,
    })
}

/// Durable per-account history, shared by tabs using this Codex home.
#[derive(Clone, Debug)]
pub struct Store {
    directory: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    v: u8,
    account: String,
    history: History,
}

impl Store {
    pub fn new(codex_home: &Path) -> Self {
        Self {
            directory: codex_home.join("alab").join("quota"),
        }
    }

    /// Merge and persist one native reading. Lock contention returns WouldBlock, so a
    /// foreground caller can keep its current view instead of waiting on another tab.
    pub fn observe(&self, account: &str, sample: WeeklySnapshot) -> io::Result<Weekly> {
        self.observe_with_history(account, sample, &mut History::default())
    }

    /// Keep accepted observations in this account's memory even when storage is
    /// unavailable. The caller must discard `accepted` on an account-generation
    /// change. A successful write also saves readings retained during earlier errors;
    /// an actual process exit before that write necessarily loses unwritten readings.
    pub fn observe_with_history(
        &self,
        account: &str,
        sample: WeeklySnapshot,
        accepted: &mut History,
    ) -> io::Result<Weekly> {
        if account.is_empty() || account.len() > 4096 || !sample.valid() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid quota observation",
            ));
        }
        accepted.observe(sample);
        let mut directory = fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        directory.mode(0o700);
        directory.create(&self.directory)?;
        #[cfg(unix)]
        fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))?;
        let path = self.path(account);
        let _guard = Lock::acquire(&path.with_extension("lock"))?;
        let stored = self.load(account)?;
        let mut history = stored.clone();
        let current = accepted.samples.last() == Some(&sample);
        let newer_stored = stored
            .samples
            .last()
            .is_some_and(|last| last.observed_at > sample.observed_at);
        if current && !newer_stored {
            let mut start = stored.segment_start.max(accepted.segment_start);
            if stored.samples.last().is_some_and(|last| {
                start.is_none_or(|start| last.observed_at >= start) && sample.changes_segment(*last)
            }) {
                // Deduplication must not erase the very same-second decrease
                // that invalidates earlier durable forecast anchors.
                start = Some(sample.observed_at);
            }
            let mut observations = BTreeMap::new();
            // A missing or malformed file cannot erase already accepted memory.
            // A real durable history supplies acknowledged anchors, including
            // another tab's historical corrections.
            let acknowledged = if stored.samples.is_empty() {
                &accepted.samples
            } else {
                &stored.samples
            };
            for observation in acknowledged {
                if observation.resets_at == sample.resets_at
                    && observation.window_minutes == sample.window_minutes
                    && start.is_none_or(|start| observation.observed_at >= start)
                {
                    observations.insert(observation.observed_at, *observation);
                }
            }
            let baseline: BTreeMap<_, _> = accepted
                .acknowledged
                .iter()
                .map(|observation| (observation.observed_at, observation))
                .collect();
            for observation in &accepted.pending {
                if start.is_some_and(|start| observation.observed_at < start) {
                    continue;
                }
                if observation.observed_at != sample.observed_at
                    && observations
                        .get(&observation.observed_at)
                        .is_some_and(|durable| {
                            durable != observation
                                && baseline.get(&observation.observed_at).copied() != Some(durable)
                        })
                {
                    // Two tabs changed a historical whole-second reading after
                    // our acknowledged baseline. Their order is unknowable; drop
                    // the ambiguous anchor and older forecast segment.
                    start = start.max(Some(observation.observed_at.saturating_add(1)));
                    continue;
                }
                // Replay only unacknowledged native readings. Their same-second
                // corrections must survive errors until a successful save.
                observations.insert(observation.observed_at, *observation);
            }
            observations.retain(|observed_at, _| start.is_none_or(|start| *observed_at >= start));
            history = History {
                segment_start: start,
                ..History::default()
            };
            for observation in observations.into_values() {
                history.observe(observation);
            }
        } else {
            history.observe(sample);
        }
        history.pending.clear();
        // The usage poll repeats its reading every 5 to 60 s, and the history keeps one point
        // per five-minute bucket, so most reads move only the latest point's time. Writing
        // that (with an fsync) on every poll bought nothing: such a read stays pending in
        // `accepted`, as after a failed write, the next bucket's write saves it, and this
        // tab's view below already shows it.
        let retimed = history.only_retimed(&stored);
        if history != stored && !retimed {
            self.save(&path, account, &history)?;
        }
        // A retimed read still hydrates the display; only its tail remains unwritten.
        if current && history.samples.last() == Some(&sample) {
            *accepted = history.clone();
            if retimed {
                accepted.acknowledged = stored.samples;
                accepted.pending.push(sample);
            } else {
                accepted.acknowledged = accepted.samples.clone();
            }
        }
        let now = history
            .samples
            .last()
            .map_or(sample.observed_at, |last| last.observed_at);
        history
            .view(now)
            .ok_or_else(|| io::Error::other("quota history is empty"))
    }

    /// Read this account only; the caller can show it as historical while awaiting refresh.
    pub fn read(&self, account: &str, now: i64) -> io::Result<Option<Weekly>> {
        self.load(account).map(|history| history.view(now))
    }

    fn path(&self, account: &str) -> PathBuf {
        // A stable filename fingerprint, never an email or untrusted path component.
        // The full identity is checked inside the file, so even a collision cannot leak it.
        let hash = account
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            });
        self.directory.join(format!("{hash:016x}.json"))
    }

    fn load(&self, account: &str) -> io::Result<History> {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        // A malformed cache entry must not park the foreground usage handler
        // while opening a FIFO. Judge the same open handle before reading it.
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK);
        let file = match options.open(self.path(account)) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(History::default()),
            Err(err) => return Err(err),
        };
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "quota history is not a regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Ok(History::default());
        }
        let Ok(stored) = serde_json::from_slice::<Stored>(&bytes) else {
            return Ok(History::default());
        };
        if stored.account != account {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "quota account does not match",
            ));
        }
        Ok(if stored.v == 1 && stored.history.valid() {
            stored.history
        } else {
            History::default()
        })
    }

    fn save(&self, path: &Path, account: &str, history: &History) -> io::Result<()> {
        static NONCE: AtomicU64 = AtomicU64::new(0);
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let temporary = path.with_extension(format!(
            "{}.{:x}.{nonce}.tmp",
            std::process::id(),
            codex_alab_core::session::now_ms()
        ));
        Self::save_to_temporary(path, &temporary, account, history)
    }

    fn save_to_temporary(
        path: &Path,
        temporary: &Path,
        account: &str,
        history: &History,
    ) -> io::Result<()> {
        let stored = Stored {
            v: 1,
            account: account.to_owned(),
            history: history.clone(),
        };
        let bytes = serde_json::to_vec(&stored).map_err(io::Error::other)?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        // Cleanup owns this path only after create_new succeeds. In particular,
        // an existing candidate belongs to somebody else and must stay intact.
        let mut file = options.open(temporary)?;
        let written = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(temporary, path)
        })();
        drop(file);
        if written.is_err() {
            let _ = fs::remove_file(temporary);
        }
        written
    }
}

struct Lock(File);

impl Lock {
    fn acquire(path: &Path) -> io::Result<Self> {
        let mut options = fs::OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options.open(path)?;
        file.try_lock().map_err(io::Error::from)?;
        Ok(Self(file))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Release on the open description before closing, including after a fork.
        let _ = self.0.unlock();
    }
}

#[cfg(test)]
#[path = "quota_tests.rs"]
mod tests;
