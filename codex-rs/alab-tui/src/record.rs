// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The session record, `$CODEX_HOME/alab/sessions/<pid>.json`, schema v1 (CONTRACT §4):
//! what aterm reads while the process's identity is live, and the crash witness after.
//! Directory 0700, file 0600, written as `<pid>.json.<nonce>.tmp` then renamed.
//! Custody covers process death and re-exec everywhere. The temporary file is
//! flushed before rename on every platform; Unix also flushes the containing
//! directory chain for power-loss durability.

use codex_alab_core::session::ProcessIdentity;
use codex_alab_core::wall_kind::LimitClass;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

pub const SCHEMA_VERSION: u32 = 1;

/// The record's directory under a `CODEX_HOME`.
pub fn sessions_dir(codex_home: &Path) -> PathBuf {
    codex_home.join("alab").join("sessions")
}

/// The record's path for `pid`.
pub fn path_for(dir: &Path, pid: u32) -> PathBuf {
    dir.join(format!("{pid}.json"))
}

/// `identity` as the record spells it (upstream's untagged camelCase form).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum Identity {
    Linux {
        boot_id: String,
        start_ticks: u64,
    },
    MacOs {
        boot_id: String,
        unique_id: u64,
        start_seconds: u64,
        start_microseconds: u64,
    },
}

impl From<&ProcessIdentity> for Identity {
    fn from(identity: &ProcessIdentity) -> Self {
        match identity.clone() {
            ProcessIdentity::Linux {
                boot_id,
                start_ticks,
            } => Self::Linux {
                boot_id,
                start_ticks,
            },
            ProcessIdentity::MacOs {
                boot_id,
                unique_id,
                start_seconds,
                start_microseconds,
            } => Self::MacOs {
                boot_id,
                unique_id,
                start_seconds,
                start_microseconds,
            },
        }
    }
}

impl From<Identity> for ProcessIdentity {
    fn from(identity: Identity) -> Self {
        match identity {
            Identity::Linux {
                boot_id,
                start_ticks,
            } => Self::Linux {
                boot_id,
                start_ticks,
            },
            Identity::MacOs {
                boot_id,
                unique_id,
                start_seconds,
                start_microseconds,
            } => Self::MacOs {
                boot_id,
                unique_id,
                start_seconds,
                start_microseconds,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Busy,
    Idle,
    Waiting,
    Upgrading,
    Exited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Working,
    Idle,
    Blocked,
    Done,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Autopilot {
    Off,
    Passive,
    Active,
}

/// A confirmed picker attempt hides the ordinary resume route while native custody is
/// unsettled. Only the bound native restart may use this copy of the validated source line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ManualAccountSelection {
    pub revision: String,
    pub selection_ref: String,
    pub request: String,
    pub resume_argv: Vec<String>,
}

impl<'de> Deserialize<'de> for ManualAccountSelection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(remote = "ManualAccountSelection", deny_unknown_fields)]
        struct Wire {
            revision: String,
            selection_ref: String,
            request: String,
            resume_argv: Vec<String>,
        }
        struct Object;
        impl<'de> serde::de::Visitor<'de> for Object {
            type Value = ManualAccountSelection;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a bounded native account selection object")
            }

            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                map: M,
            ) -> Result<Self::Value, M::Error> {
                let held = Wire::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                if held.valid() {
                    Ok(held)
                } else {
                    Err(serde::de::Error::custom("invalid account selection"))
                }
            }
        }
        deserializer.deserialize_map(Object)
    }
}

impl ManualAccountSelection {
    pub(crate) fn selection(&self) -> codex_alab_aterm::account_move::Selection {
        codex_alab_aterm::account_move::Selection {
            revision: self.revision.clone(),
            selection_ref: self.selection_ref.clone(),
            request: self.request.clone(),
        }
    }

    pub(crate) fn valid(&self) -> bool {
        self.selection().valid()
            && (3..=256).contains(&self.resume_argv.len())
            && self
                .resume_argv
                .iter()
                .map(|arg| arg.len() + 1)
                .sum::<usize>()
                <= 32 * 1024
            && self.resume_argv.iter().all(|arg| !arg.contains('\0'))
    }
}

/// Schema v1, field for field and in CONTRACT §4's order. `null` where unknown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub v: u32,
    pub pid: u32,
    pub proc_start: Option<String>,
    pub identity: Option<Identity>,
    pub session_id: Option<String>,
    pub displayed_thread_id: Option<String>,
    pub cwd: Option<String>,
    pub version: String,
    pub exe: Option<String>,
    pub launch_argv0: Option<String>,
    pub exec_generation: u64,
    pub resume_argv: Option<Vec<String>>,
    #[serde(
        default,
        rename = "manual_account_selection",
        skip_serializing_if = "Option::is_none"
    )]
    pub manual_account_selection: Option<ManualAccountSelection>,
    pub env: BTreeMap<String, String>,
    pub status: Status,
    pub exit: Option<String>,
    pub state: Option<State>,
    pub blocked_kind: Option<BlockedKind>,
    pub draft_empty: bool,
    pub last_human_input_at: Option<u64>,
    pub autopilot: Autopilot,
    pub claim: Option<String>,
    pub status_updated_at: u64,
    pub updated_at: u64,
    pub kind: String,
    pub entrypoint: String,
    /// How many relaunch carry-ons in a row led here with no person's turn and no real work
    /// since (CONTRACT §5 carry-on). Added after v1's first fields: a record without it reads
    /// 0, and aterm ignores it.
    #[serde(default)]
    pub carried_on: u32,
    /// The autopilot's act owed as the live upgrade re-executes, or its Land landing (CONTRACT
    /// §5 live upgrade, amended 2026-10-19): written only by the image that leaves, read by the
    /// next. Absent when none (a record without it reads none), and aterm ignores it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "read_owed"
    )]
    pub owed: Option<Owed>,
    /// The NEEDS YOU set an Off took off the person's screen, which no report has raised
    /// again (CONTRACT §4, added 2026-10-20): its key as 16 hex digits ([`needs_key_word`]),
    /// so the image after a re-exec, or a relaunch after a crash, does not put it back from the
    /// history (which shows no Off). Absent when no Off took a set down (a record without it
    /// names none), and aterm ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_off: Option<String>,
    /// The folder the primary thread's worker last ran a command in (CONTRACT §4, added
    /// 2026-10-20): where the autopilot's repository probe reads, carried to the image after a
    /// re-exec and the relaunch after a crash. Absent until a command ran, and aterm ignores
    /// it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
    /// What the primary thread's repository probe reads (CONTRACT §4, added 2026-11-05 after
    /// the review of the forever probe): the folders its worker touched, whether it lost one,
    /// when its session began and the branches its commands created or checked out with the
    /// folder each was named in, carried to the image after a re-exec and the relaunch after a
    /// crash. Absent
    /// until any is known (a record without it carries none), aterm ignores it, and a form this
    /// reader does not know discards only this field.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "read_probe_scope"
    )]
    pub probe_scope: Option<ProbeScope>,
    /// The primary thread's pace (CONTRACT §4, added 2026-10-21): its short streak and the
    /// rung of the wall's ladder it is on, carried to the image after a re-exec and to the
    /// relaunch after a crash. Absent at rest (a record without it carries none), and aterm
    /// ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pace: Option<Pace>,
    /// A provider failure still waiting on its original deadline. Older readers ignore
    /// this optional field; an unknown future form discards only this field.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "read_recovery"
    )]
    pub recovery: Option<Recovery>,
    /// The selected wait of an interrupted in-flight request. This is only a lower
    /// bound on an independently authorized crash carry-on, never authority to send one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "read_in_flight_retry"
    )]
    pub in_flight_retry: Option<InFlightRetry>,
    /// The aterm session the process runs in (CONTRACT §4, added 2026-10-22): the key of
    /// aterm's own books for its tab. A start in the same session takes up the record's `pace`
    /// as aterm's loop takes up those books ([`codex_alab_core::session::same_aterm_session`]).
    /// Absent outside aterm (a record without it names none), and aterm ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aterm_session: Option<String>,
    /// The primary thread's active goal objective (CONTRACT §4, added 2026-10-25): bounded to
    /// [`codex_alab_core::session::GOAL_MAX_BYTES`] UTF-8 bytes after trimming, absent when no
    /// goal is `active` on the primary thread, so aterm reads the session's standing goal from
    /// this record without opening SQLite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    /// The background terminals the TUI's footer counts for the displayed thread (CONTRACT §4,
    /// added 2026-10-29, gap 28): its chat widget's live unified exec processes as of this write,
    /// one a running tool call still waits on included, as `/ps` lists them. A move that alone
    /// changes the record is written once it has stood the writer's interval, unless another
    /// change writes it first. Absent before the first frame and in the busy initializer; aterm's
    /// survey shows it as the tab's `bg=`. A form this reader does not know discards only this
    /// field.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "read_count"
    )]
    pub background_terminals: Option<u32>,
}

/// [`Record::probe_scope`]: the repository probe's folders, its overflow, its session's start
/// (Unix seconds) and its branches.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeScope {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dirs: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub overflow: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<UsedBranch>,
}

/// [`ProbeScope::branches`]: a branch's name and the folder it was named in (absent where none
/// could be placed, or none written as UTF-8: then it is read in every repository).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsedBranch {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

impl From<codex_alab_core::session::ProbeScope> for ProbeScope {
    fn from(scope: codex_alab_core::session::ProbeScope) -> Self {
        // A folder whose name is no UTF-8 cannot be written: the scope overflows instead, so
        // the image after it never reads it clean.
        let dirs: Vec<String> = scope
            .dirs
            .iter()
            .filter_map(|d| d.to_str().map(str::to_owned))
            .collect();
        Self {
            overflow: scope.overflow || dirs.len() != scope.dirs.len(),
            dirs,
            since: scope.since,
            // A folder whose name is no UTF-8 is written as none: the branch is then read in
            // every repository, never in none.
            branches: scope
                .branches
                .into_iter()
                .map(|b| UsedBranch {
                    dir: b.dir.and_then(|d| d.to_str().map(str::to_owned)),
                    name: b.name,
                })
                .collect(),
        }
    }
}

impl From<ProbeScope> for codex_alab_core::session::ProbeScope {
    fn from(scope: ProbeScope) -> Self {
        use codex_alab_core::session::PROBE_SCOPE_BRANCHES;
        use codex_alab_core::session::PROBE_SCOPE_DIRS;
        let absolute: Vec<std::path::PathBuf> = scope
            .dirs
            .iter()
            .map(std::path::PathBuf::from)
            .filter(|d| d.is_absolute())
            .collect();
        Self {
            // A record that names more than a scope keeps, or a folder no absolute path, lost
            // some: it overflows.
            overflow: scope.overflow
                || absolute.len() != scope.dirs.len()
                || absolute.len() > PROBE_SCOPE_DIRS,
            dirs: absolute.into_iter().take(PROBE_SCOPE_DIRS).collect(),
            since: scope.since,
            // A folder no absolute path is none: read in every repository.
            branches: scope
                .branches
                .into_iter()
                .take(PROBE_SCOPE_BRANCHES)
                .map(|b| codex_alab_core::session::UsedBranch {
                    dir: b
                        .dir
                        .map(std::path::PathBuf::from)
                        .filter(|d| d.is_absolute()),
                    name: b.name,
                })
                .collect(),
        }
    }
}

/// [`Record::probe_scope`] as written; a form this reader does not know discards only this
/// field, so the start's sweep still parses a predecessor's record, and reads as a scope that
/// lost what it held (`overflow`), so the image after it never reads the repository clean over
/// folders it cannot name (review of the forever probe: a cut read is never clean). `null` is
/// none.
fn read_probe_scope<'de, D>(deserializer: D) -> Result<Option<ProbeScope>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_value(value).unwrap_or(ProbeScope {
        overflow: true,
        ..ProbeScope::default()
    })))
}

/// [`Record::background_terminals`] as written, or none for a form this reader does not know (a
/// string, a negative, a fraction, past `u32`): such a value discards only this field, so the
/// start's sweep still parses a predecessor's record (a crash witness) that carries one.
fn read_count<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value.as_u64().and_then(|n| u32::try_from(n).ok()))
}

/// Optional boot-clock coordinates for pacing, never authority to resume work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryClock {
    pub boot_id: String,
    pub continuous_ns: u64,
    pub epoch_ms: u64,
}

fn read_recovery_clock<'de, D>(deserializer: D) -> Result<Option<RecoveryClock>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    ClockField::deserialize(deserializer).map(|clock| clock.0)
}

// Inspect timing keys while Value consumes the original serde stream. RawValue
// reparsing would reset depth and skip numeric and Unicode validation.
struct ClockField(Option<RecoveryClock>);

impl<'de> Deserialize<'de> for ClockField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let clock = deserializer.deserialize_any(ClockedJsonVisitor(JsonScope::Clock))?;
        if clock.invalid_clock {
            return Ok(Self(None));
        }
        Ok(Self(
            serde_json::from_value::<RecoveryClock>(clock.value)
                .ok()
                .filter(|clock| {
                    codex_alab_core::recovery_clock::Anchor::from(clock.clone()).valid()
                }),
        ))
    }
}

#[derive(Clone, Copy)]
enum JsonScope {
    Wait,
    Clock,
}

struct ClockedJson {
    value: serde_json::Value,
    invalid_clock: bool,
}

impl ClockedJson {
    fn plain(value: serde_json::Value) -> Self {
        Self {
            value,
            invalid_clock: false,
        }
    }
}

struct ClockAudit {
    scope: JsonScope,
    first_key: bool,
    seen: u8,
    invalid_clock: bool,
}

impl ClockAudit {
    fn observe(&mut self, key: &str) {
        let first_key = self.first_key;
        self.first_key = false;
        if first_key
            && matches!(self.scope, JsonScope::Wait)
            && key == "$serde_json::private::RawValue"
        {
            // Value expands this first-key encoding before its clock members can be audited.
            // Preserve the decoded wait, but unsupported timing encodings supply no clock.
            self.invalid_clock = true;
        }
        let bit = match (self.scope, key) {
            (JsonScope::Wait, "clock") | (JsonScope::Clock, "bootId") => 1,
            (JsonScope::Wait, _) => return,
            (JsonScope::Clock, "continuousNs") => 2,
            (JsonScope::Clock, "epochMs") => 4,
            (JsonScope::Clock, _) => {
                self.invalid_clock = true;
                return;
            }
        };
        self.invalid_clock |= self.seen & bit != 0;
        self.seen |= bit;
    }
}

struct ClockedMapAccess<'a, A> {
    inner: A,
    audit: &'a mut ClockAudit,
    clock_value: bool,
}

impl<'de, A> serde::de::MapAccess<'de> for ClockedMapAccess<'_, A>
where
    A: serde::de::MapAccess<'de>,
{
    type Error = A::Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: serde::de::DeserializeSeed<'de>,
    {
        let Some(key) = self.inner.next_key::<String>()? else {
            return Ok(None);
        };
        self.audit.observe(&key);
        self.clock_value = matches!(self.audit.scope, JsonScope::Wait) && key == "clock";
        seed.deserialize(serde::de::value::StringDeserializer::<A::Error>::new(key))
            .map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: serde::de::DeserializeSeed<'de>,
    {
        if self.clock_value {
            self.clock_value = false;
            let clock = self.inner.next_value::<ClockField>()?;
            let value =
                serde_json::to_value(clock.0).map_err(<A::Error as serde::de::Error>::custom)?;
            seed.deserialize(value)
                .map_err(<A::Error as serde::de::Error>::custom)
        } else {
            self.inner.next_value_seed(seed)
        }
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

struct ClockedJsonVisitor(JsonScope);

impl<'de> serde::de::Visitor<'de> for ClockedJsonVisitor {
    type Value = ClockedJson;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("any valid JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(ClockedJson::plain(serde_json::Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(ClockedJson::plain(serde_json::Value::Number(value.into())))
    }

    fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let deserializer = serde::de::value::I128Deserializer::<E>::new(value);
        serde_json::Number::deserialize(deserializer)
            .map(|number| ClockedJson::plain(serde_json::Value::Number(number)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(ClockedJson::plain(serde_json::Value::Number(value.into())))
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let deserializer = serde::de::value::U128Deserializer::<E>::new(value);
        serde_json::Number::deserialize(deserializer)
            .map(|number| ClockedJson::plain(serde_json::Value::Number(number)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        Ok(ClockedJson::plain(
            serde_json::Number::from_f64(value)
                .map_or(serde_json::Value::Null, serde_json::Value::Number),
        ))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_string(value.to_owned())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(ClockedJson::plain(serde_json::Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(ClockedJson::plain(serde_json::Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(ClockedJson::plain(serde_json::Value::Null))
    }

    fn visit_seq<A>(self, sequence: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        serde_json::Value::deserialize(serde::de::value::SeqAccessDeserializer::new(sequence))
            .map(ClockedJson::plain)
    }

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut audit = ClockAudit {
            scope: self.0,
            first_key: true,
            seen: 0,
            invalid_clock: false,
        };
        // Keep Value's private first-key behavior and the caller's live parser budget.
        let value = serde_json::Value::deserialize(serde::de::value::MapAccessDeserializer::new(
            ClockedMapAccess {
                inner: map,
                audit: &mut audit,
                clock_value: false,
            },
        ))?;
        Ok(ClockedJson {
            value,
            invalid_clock: audit.invalid_clock,
        })
    }
}

fn read_clocked_wait<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let mut wait = deserializer.deserialize_any(ClockedJsonVisitor(JsonScope::Wait))?;
    if wait.invalid_clock
        && let serde_json::Value::Object(fields) = &mut wait.value
    {
        // Ambiguous timing loses only its clock, never the known wait.
        fields.insert("clock".to_string(), serde_json::Value::Null);
    }
    Ok(serde_json::from_value(wait.value).ok())
}

impl From<codex_alab_core::recovery_clock::Anchor> for RecoveryClock {
    fn from(clock: codex_alab_core::recovery_clock::Anchor) -> Self {
        Self {
            boot_id: clock.boot_id,
            continuous_ns: clock.continuous_ns,
            epoch_ms: clock.epoch_ms,
        }
    }
}

impl From<RecoveryClock> for codex_alab_core::recovery_clock::Anchor {
    fn from(clock: RecoveryClock) -> Self {
        Self {
            boot_id: clock.boot_id,
            continuous_ns: clock.continuous_ns,
            epoch_ms: clock.epoch_ms,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InFlightRetry {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "read_recovery_clock"
    )]
    pub clock: Option<RecoveryClock>,
    pub thread: String,
    pub turn_id: String,
    pub not_before: u64,
}

fn read_in_flight_retry<'de, D>(deserializer: D) -> Result<Option<InFlightRetry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(
        read_clocked_wait::<D, InFlightRetry>(deserializer)?.filter(|retry| {
            !retry.thread.trim().is_empty()
                && retry.thread.len() <= 512
                && !retry.turn_id.trim().is_empty()
                && retry.turn_id.len() <= 512
                && retry.not_before != 0
        }),
    )
}

impl From<codex_alab_core::session::InFlightRetry> for InFlightRetry {
    fn from(retry: codex_alab_core::session::InFlightRetry) -> Self {
        Self {
            clock: retry.clock.map(Into::into),
            thread: retry.thread,
            turn_id: retry.turn_id,
            not_before: retry.not_before_ms,
        }
    }
}

impl From<InFlightRetry> for codex_alab_core::session::InFlightRetry {
    fn from(retry: InFlightRetry) -> Self {
        Self {
            clock: retry.clock.map(Into::into),
            thread: retry.thread,
            turn_id: retry.turn_id,
            not_before_ms: retry.not_before,
        }
    }
}

/// A failed turn's recovery intent. It names a point to re-decide under current policy,
/// rather than a prompt that can bypass native usage or goal checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "read_recovery_clock"
    )]
    pub clock: Option<RecoveryClock>,
    pub thread: String,
    pub turn_id: String,
    pub point_at: u64,
    pub not_before: u64,
    pub by_autopilot: bool,
    pub by_person: bool,
    pub wall: RecoveryWall,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RecoveryWall {
    Network {
        what: String,
        /// The process that met the wall measured its route (CONTRACT §4); absent: not
        /// measured, as a record before it reads.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        measured: bool,
    },
    Overlong {
        what: String,
    },
    Entitlement {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        quota: Option<QuotaScope>,
    },
    Server {
        what: String,
        #[serde(rename = "cutOff")]
        cut_off: bool,
        #[serde(rename = "retryAt", default, skip_serializing_if = "Option::is_none")]
        retry_at: Option<u64>,
    },
    Usage {
        #[serde(
            rename = "limitClass",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        limit_class: Option<LimitClass>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        quota: Option<QuotaScope>,
        #[serde(rename = "resetAt", default, skip_serializing_if = "Option::is_none")]
        reset_at: Option<u64>,
        #[serde(
            rename = "resetLabel",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        reset_label: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaScope {
    account: String,
    bucket: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user: Option<String>,
    #[serde(default)]
    ordinary_retry: bool,
    #[serde(default)]
    ordinary_denied: bool,
}

impl Recovery {
    fn bound_details(&mut self) {
        match &mut self.wall {
            RecoveryWall::Server { what, .. }
            | RecoveryWall::Network { what, .. }
            | RecoveryWall::Overlong { what } => bound_recovery_detail(what),
            RecoveryWall::Entitlement { quota } => {
                if quota.as_ref().is_some_and(|q| {
                    !codex_alab_core::quota::Scope {
                        account: q.account.clone(),
                        bucket: q.bucket.clone(),
                        user: q.user.clone(),
                        ordinary_retry: q.ordinary_retry,
                        ordinary_denied: q.ordinary_denied,
                    }
                    .valid()
                }) {
                    *quota = None;
                }
            }
            RecoveryWall::Usage {
                reset_label, quota, ..
            } => {
                if let Some(label) = reset_label {
                    bound_recovery_detail(label);
                }
                if quota.as_ref().is_some_and(|q| {
                    !(codex_alab_core::quota::Scope {
                        account: q.account.clone(),
                        bucket: q.bucket.clone(),
                        user: q.user.clone(),
                        ordinary_retry: q.ordinary_retry,
                        ordinary_denied: q.ordinary_denied,
                    })
                    .valid()
                }) {
                    *quota = None;
                }
            }
        }
    }

    fn valid(&self) -> bool {
        !self.thread.trim().is_empty()
            && self.thread.len() <= 512
            && !self.turn_id.trim().is_empty()
            && self.turn_id.len() <= 512
            && self.point_at <= self.not_before
            && match &self.wall {
                RecoveryWall::Server { what, .. }
                | RecoveryWall::Network { what, .. }
                | RecoveryWall::Overlong { what } => !what.trim().is_empty() && what.len() <= 4096,
                RecoveryWall::Entitlement { .. } => true,
                RecoveryWall::Usage { reset_label, .. } => {
                    reset_label.as_ref().is_none_or(|label| label.len() <= 4096)
                }
            }
    }
}

fn bound_recovery_detail(detail: &mut String) {
    const MAX_BYTES: usize = 4096;
    if detail.len() <= MAX_BYTES {
        return;
    }
    let mut end = MAX_BYTES;
    while !detail.is_char_boundary(end) {
        end -= 1;
    }
    detail.truncate(end);
}

fn read_recovery<'de, D>(deserializer: D) -> Result<Option<Recovery>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(
        read_clocked_wait::<D, Recovery>(deserializer)?.and_then(|mut recovery| {
            recovery.bound_details();
            recovery.valid().then_some(recovery)
        }),
    )
}

impl From<codex_alab_core::session::Recovery> for Recovery {
    fn from(recovery: codex_alab_core::session::Recovery) -> Self {
        let mut stored = Self {
            clock: recovery.clock.map(Into::into),
            thread: recovery.thread,
            turn_id: recovery.turn_id,
            point_at: recovery.point_at_ms,
            not_before: recovery.not_before_ms,
            by_autopilot: recovery.by_autopilot,
            by_person: recovery.by_person,
            wall: match recovery.wall {
                codex_alab_core::session::RecoveryWall::Network { what, measured } => {
                    RecoveryWall::Network { what, measured }
                }
                codex_alab_core::session::RecoveryWall::Overlong { what } => {
                    RecoveryWall::Overlong { what }
                }
                codex_alab_core::session::RecoveryWall::Entitlement { quota } => {
                    RecoveryWall::Entitlement {
                        quota: quota.map(|q| QuotaScope {
                            account: q.account,
                            bucket: q.bucket,
                            user: q.user,
                            ordinary_retry: q.ordinary_retry,
                            ordinary_denied: q.ordinary_denied,
                        }),
                    }
                }
                codex_alab_core::session::RecoveryWall::Server {
                    what,
                    cut_off,
                    retry_at_ms,
                } => RecoveryWall::Server {
                    what,
                    cut_off,
                    retry_at: retry_at_ms,
                },
                codex_alab_core::session::RecoveryWall::Usage {
                    limit_class,
                    reset_at_ms,
                    reset_label,
                    quota,
                } => RecoveryWall::Usage {
                    limit_class: Some(limit_class),
                    reset_at: reset_at_ms,
                    reset_label,
                    quota: quota.map(|q| QuotaScope {
                        account: q.account,
                        bucket: q.bucket,
                        user: q.user,
                        ordinary_retry: q.ordinary_retry,
                        ordinary_denied: q.ordinary_denied,
                    }),
                },
            },
        };
        stored.bound_details();
        stored
    }
}

impl From<Recovery> for codex_alab_core::session::Recovery {
    fn from(recovery: Recovery) -> Self {
        Self {
            clock: recovery.clock.map(Into::into),
            thread: recovery.thread,
            turn_id: recovery.turn_id,
            point_at_ms: recovery.point_at,
            not_before_ms: recovery.not_before,
            by_autopilot: recovery.by_autopilot,
            by_person: recovery.by_person,
            wall: match recovery.wall {
                RecoveryWall::Network { what, measured } => {
                    codex_alab_core::session::RecoveryWall::Network { what, measured }
                }
                RecoveryWall::Overlong { what } => {
                    codex_alab_core::session::RecoveryWall::Overlong { what }
                }
                RecoveryWall::Entitlement { quota } => {
                    codex_alab_core::session::RecoveryWall::Entitlement {
                        quota: quota.map(|q| codex_alab_core::quota::Scope {
                            account: q.account,
                            bucket: q.bucket,
                            user: q.user,
                            ordinary_retry: q.ordinary_retry,
                            ordinary_denied: q.ordinary_denied,
                        }),
                    }
                }
                RecoveryWall::Server {
                    what,
                    cut_off,
                    retry_at,
                } => codex_alab_core::session::RecoveryWall::Server {
                    what,
                    cut_off,
                    retry_at_ms: retry_at,
                },
                RecoveryWall::Usage {
                    limit_class,
                    reset_at,
                    reset_label,
                    quota,
                } => codex_alab_core::session::RecoveryWall::Usage {
                    // The old writer saved the bucket but did not record whether the refusal
                    // classified it as a model limit or Reserve/workspace usage.
                    limit_class: limit_class.unwrap_or_else(|| {
                        if quota
                            .as_ref()
                            .is_some_and(|q| q.bucket == codex_alab_core::wall_kind::ORDINARY_LIMIT)
                        {
                            LimitClass::Ordinary
                        } else {
                            LimitClass::Unverified
                        }
                    }),
                    reset_at_ms: reset_at,
                    reset_label,
                    quota: quota.map(|q| codex_alab_core::quota::Scope {
                        account: q.account,
                        bucket: q.bucket,
                        user: q.user,
                        ordinary_retry: q.ordinary_retry,
                        ordinary_denied: q.ordinary_denied,
                    }),
                },
            },
        }
    }
}

/// The record's `pace` ([`codex_alab_core::session::Pace`]). Every key is optional, so a
/// record with a pace this build cannot read loses only the pace.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pace {
    #[serde(default)]
    pub short_streak: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall: Option<WallMark>,
    /// When each act `continue_per_hour` counts was typed within its window (ms since the
    /// epoch): the next process spends the hour's budget where this one left it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub typed: Vec<u64>,
    /// The act a wall cut, which its tries deliver again (CONTRACT §4); absent where none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<CutAct>,
    /// The done checks this Cruise run typed; absent where none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub checks: u8,
    /// Ruling F's replies, the oldest first (CONTRACT §4); absent where none, so an older
    /// record reads as none kept.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<KeptReply>,
    /// The no-work answers in a row; absent where none.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub replies_run: u32,
    /// When the newest answer counted came (ms since the epoch); absent where none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replies_at: Option<u64>,
    /// The reset the newest answer counted showed (ms since the epoch); absent where none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replies_reset: Option<u64>,
    /// The words of the point ruling F holds; absent where none is held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replies_point: Option<ReplyPoint>,
    /// Land's hold standing (CONTRACT §4); absent where none stands, so an older record reads
    /// as none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub land_hold: Option<LandHold>,
    /// When the person's hour standing began (ms since the epoch; CONTRACT §4); absent where
    /// none holds, so an older record reads as none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub go_ahead_hour: Option<u64>,
    /// The go-ahead's count on the standing park (CONTRACT §4); absent where none was typed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub go_ahead: Option<GoAheadCount>,
    /// The point left to the person (CONTRACT §4); absent where none is left, so an older
    /// record reads as none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_point: Option<LeftPoint>,
    /// A person's Esc standing over the point (CONTRACT §4); absent where none, so an older
    /// record reads as none, and a form this reader does not know discards only this field
    /// (no Esc carried: the point stands as the history shows it).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "read_esc"
    )]
    pub esc: Option<EscHold>,
    /// The goal a turn error blocked at the wall the point stands at (CONTRACT §4); absent where
    /// none, so an older record reads as none, and a form this reader does not know discards
    /// only this field (no goal bound: the wall's try resumes none, and a blocked goal parks).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "read_wall_goal"
    )]
    pub wall_goal: Option<WallGoal>,
    /// A model's successful block is distinct from a provider wall's stop.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "read_wall_goal"
    )]
    pub model_blocked_goal: Option<WallGoal>,
}

/// [`Pace::esc`] as written, or none for a form this reader does not know, or one that names no
/// time or no turn.
fn read_esc<'de, D>(deserializer: D) -> Result<Option<EscHold>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value::<EscHold>(value)
        .ok()
        .filter(|esc| esc.at != 0 && !esc.turn.trim().is_empty() && esc.turn.len() <= 512))
}

/// [`Pace::wall_goal`] as written, or none for a form this reader does not know, or one that
/// names no goal or no turn (the image after reads the turn's history for a person's stop, so a
/// goal it cannot place is none to resume).
fn read_wall_goal<'de, D>(deserializer: D) -> Result<Option<WallGoal>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value::<WallGoal>(value)
        .ok()
        .filter(|goal| {
            !goal.id.trim().is_empty()
                && goal.id.len() <= 512
                && !goal.turn.trim().is_empty()
                && goal.turn.len() <= 512
                && goal
                    .chronology_fence_generation
                    .as_ref()
                    .is_none_or(|generation| {
                        codex_alab_core::goal_chronology_fence::FenceGeneration::from_hex(
                            generation,
                        )
                        .is_some()
                    })
        }))
}

/// The record's `pace.esc` ([`codex_alab_core::session::EscHold`]): `at` ms since the epoch,
/// `turn` the native turn it stopped.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EscHold {
    pub at: u64,
    pub turn: String,
}

/// The record's `pace.wallGoal` ([`codex_alab_core::session::WallGoal`]): the goal's identity as
/// the wall left it, `budget` absent where it has none, and `turn`, the native turn whose turn
/// error blocked it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WallGoal {
    pub id: String,
    pub objective: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<i64>,
    pub turn: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chronology_fence_generation: Option<String>,
}

/// The record's `pace.leftPoint` ([`codex_alab_core::session::LeftPoint`]): the point's words as
/// `repliesPoint` writes them, and `opens` where its exchange opened the session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftPoint {
    #[serde(flatten)]
    pub point: ReplyPoint,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub opens: bool,
    /// Older records do not prove the opening was taskless.
    #[serde(default = "unknown_left_point_work")]
    pub ran_tools: bool,
    /// Unknown history in an older record cannot establish a taskless opening.
    #[serde(default = "unknown_left_point_work")]
    pub had_questions: bool,
}

fn unknown_left_point_work() -> bool {
    true
}

/// The record's `pace.goAhead` ([`codex_alab_core::session::GoAheadCount`]): `acts` absent at 0,
/// `first` ms since the epoch, absent where none.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoAheadCount {
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub acts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first: Option<u64>,
}

/// The record's `pace.landHold` ([`codex_alab_core::session::LandHold`]): `since` ms since the
/// epoch, `nudges` absent at 0.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LandHold {
    pub since: u64,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub nudges: u32,
}

/// The record's `pace.repliesPoint` ([`codex_alab_core::session::ReplyPoint`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyPoint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earlier: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub questions: bool,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}

/// The record's `pace.replies[]` ([`codex_alab_core::session::KeptReply`]): the mark as 16
/// lowercase hex digits, as `needsOff` writes a 64-bit key.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeptReply {
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub answers: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub walled: bool,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub tries: u32,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_zero(n: &u8) -> bool {
    *n == 0
}

/// The record's `pace.cut` ([`codex_alab_core::session::CutAct`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CutAct {
    pub text: String,
    pub mode: String,
}

/// The record's `pace.wall` ([`codex_alab_core::session::WallMark`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WallMark {
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub first_seen: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_try: Option<u64>,
    /// The appearance the ladder counts from (aterm's `since`); absent from a build that
    /// carried none, whose ladder counts from `lastTry`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    /// The last try still owes its answer: the process died in its turn.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub answer_owed: bool,
}

impl From<codex_alab_core::session::Pace> for Pace {
    fn from(pace: codex_alab_core::session::Pace) -> Self {
        Pace {
            short_streak: pace.short_streak,
            wall: pace.wall.map(|wall| WallMark {
                class: wall.class,
                attempts: wall.attempts,
                first_seen: wall.first_seen_ms,
                last_try: wall.last_try_ms,
                since: wall.since_ms,
                answer_owed: wall.answer_owed,
            }),
            typed: pace.typed_ms,
            cut: pace.cut.map(|cut| CutAct {
                text: cut.text,
                mode: cut.mode,
            }),
            checks: pace.checks,
            replies: pace
                .replies
                .into_iter()
                .map(|k| KeptReply {
                    mark: needs_key_word(k.mark),
                    answers: k.answers,
                    walled: k.walled,
                    tries: k.tries,
                })
                .collect(),
            replies_run: pace.replies_run,
            replies_at: pace.replies_at_ms,
            replies_reset: pace.replies_reset_ms,
            replies_point: pace.replies_point.map(|point| ReplyPoint {
                text: point.text,
                earlier: point.earlier,
                questions: point.questions,
            }),
            land_hold: pace.land_hold.map(|hold| LandHold {
                since: hold.since_ms,
                nudges: hold.nudges,
            }),
            go_ahead_hour: pace.go_ahead_hour_ms,
            go_ahead: pace.go_ahead.map(|count| GoAheadCount {
                acts: count.acts,
                first: count.first_ms,
            }),
            left_point: pace.left_point.map(|left| LeftPoint {
                point: ReplyPoint {
                    text: left.point.text,
                    earlier: left.point.earlier,
                    questions: left.point.questions,
                },
                opens: left.opens,
                ran_tools: left.ran_tools,
                had_questions: left.had_questions,
            }),
            esc: pace.esc.map(|esc| EscHold {
                at: esc.at_ms,
                turn: esc.turn,
            }),
            wall_goal: pace.wall_goal.map(|goal| WallGoal {
                id: goal.goal_id,
                objective: goal.objective,
                budget: goal.token_budget,
                turn: goal.turn,
                chronology_fence_generation: goal
                    .chronology_fence_generation
                    .map(codex_alab_core::goal_chronology_fence::FenceGeneration::to_hex),
            }),
            model_blocked_goal: pace.model_blocked_goal.map(|goal| WallGoal {
                id: goal.goal_id,
                objective: goal.objective,
                budget: goal.token_budget,
                turn: goal.turn,
                chronology_fence_generation: goal
                    .chronology_fence_generation
                    .map(codex_alab_core::goal_chronology_fence::FenceGeneration::to_hex),
            }),
        }
    }
}

impl From<Pace> for codex_alab_core::session::Pace {
    fn from(pace: Pace) -> Self {
        codex_alab_core::session::Pace {
            short_streak: pace.short_streak,
            wall: pace.wall.map(|wall| codex_alab_core::session::WallMark {
                class: wall.class,
                attempts: wall.attempts,
                first_seen_ms: wall.first_seen,
                last_try_ms: wall.last_try,
                since_ms: wall.since,
                answer_owed: wall.answer_owed,
            }),
            typed_ms: pace.typed,
            cut: pace.cut.map(|cut| codex_alab_core::session::CutAct {
                text: cut.text,
                mode: cut.mode,
            }),
            checks: pace.checks,
            // A mark this build cannot read is no reply kept.
            replies: pace
                .replies
                .into_iter()
                .filter_map(|k| {
                    Some(codex_alab_core::session::KeptReply {
                        mark: needs_key_of(&k.mark)?,
                        answers: k.answers,
                        walled: k.walled,
                        tries: k.tries,
                    })
                })
                .collect(),
            replies_run: pace.replies_run,
            replies_at_ms: pace.replies_at,
            replies_reset_ms: pace.replies_reset,
            replies_point: pace
                .replies_point
                .map(|point| codex_alab_core::session::ReplyPoint {
                    text: point.text,
                    earlier: point.earlier,
                    questions: point.questions,
                }),
            land_hold: pace
                .land_hold
                .map(|hold| codex_alab_core::session::LandHold {
                    since_ms: hold.since,
                    nudges: hold.nudges,
                }),
            go_ahead_hour_ms: pace.go_ahead_hour,
            go_ahead: pace
                .go_ahead
                .map(|count| codex_alab_core::session::GoAheadCount {
                    acts: count.acts,
                    first_ms: count.first,
                }),
            left_point: pace
                .left_point
                .map(|left| codex_alab_core::session::LeftPoint {
                    point: codex_alab_core::session::ReplyPoint {
                        text: left.point.text,
                        earlier: left.point.earlier,
                        questions: left.point.questions,
                    },
                    opens: left.opens,
                    ran_tools: left.ran_tools,
                    had_questions: left.had_questions,
                }),
            esc: pace.esc.map(|esc| codex_alab_core::session::EscHold {
                at_ms: esc.at,
                turn: esc.turn,
            }),
            // A person's stops are each process's own to count: a carried goal is bound at none,
            // where the image after begins its count.
            wall_goal: pace
                .wall_goal
                .map(|goal| codex_alab_core::session::WallGoal {
                    goal_id: goal.id,
                    objective: goal.objective,
                    token_budget: goal.budget,
                    chronology_fence_generation: goal
                        .chronology_fence_generation
                        .as_deref()
                        .and_then(
                            codex_alab_core::goal_chronology_fence::FenceGeneration::from_hex,
                        ),
                    turn: goal.turn,
                    person_stops: 0,
                }),
            model_blocked_goal: pace.model_blocked_goal.map(|goal| {
                codex_alab_core::session::WallGoal {
                    goal_id: goal.id,
                    objective: goal.objective,
                    token_budget: goal.budget,
                    chronology_fence_generation: goal
                        .chronology_fence_generation
                        .as_deref()
                        .and_then(
                            codex_alab_core::goal_chronology_fence::FenceGeneration::from_hex,
                        ),
                    turn: goal.turn,
                    person_stops: 0,
                }
            }),
        }
    }
}

/// The record's `needsOff` for a needs set's key: 16 lowercase hex digits (a JSON number
/// would lose a 64-bit key's low digits in any reader that reads numbers as doubles).
pub fn needs_key_word(key: u64) -> String {
    format!("{key:016x}")
}

/// The key a record's `needsOff` names; `None` for anything but 16 hex digits.
pub fn needs_key_of(word: &str) -> Option<u64> {
    if word.len() != 16 || !word.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(word, 16).ok()
}

/// The record's `owed` ([`codex_alab_core::session::OwedAct`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Owed {
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_origin: Option<NativeOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renewal: Option<AccountHandoff>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<AccountHandoff>,
    /// Absent for a landing alone (CONTRACT §4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// The words the act is typed with, exactly as the old image decided them; with `rule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The Autopilot word they were decided under (`cruise`, `land`); with `rule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default)]
    pub not_before: u64,
    /// The open work a done claim's act decided before the repository was read names; present,
    /// the next image reads the repository once the act is due and places its suffix
    /// (CONTRACT §4); with `rule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_owed: Option<Vec<String>>,
    /// The usage window's resume: a usage-limited goal is resumed first, else the words are
    /// typed (CONTRACT §4); with `rule` `usage-resume@v1`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resume_goal: bool,
    /// A wall's try that delivers a land act again: its yield's clean defense lands the task
    /// (CONTRACT §4); with `rule` a wall's try. Absent: none, as a record before it reads.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub landing: bool,
    #[serde(default)]
    pub landed: bool,
}

impl Owed {
    /// The account handoff the act carries, with its purpose: `renewal` is the same account's,
    /// and `rotation` a move to another account, whose purpose its `rule` names
    /// ([`codex_alab_core::account_handoff::Purpose::of_rotation_rule`]). A `rotation` under
    /// any other rule, or both fields, carries none.
    pub(crate) fn handoff(
        &self,
    ) -> Option<(&AccountHandoff, codex_alab_core::account_handoff::Purpose)> {
        use codex_alab_core::account_handoff::Purpose;
        match (&self.renewal, &self.rotation) {
            (Some(value), None) => Some((value, Purpose::Login)),
            (None, Some(value)) => Some((
                value,
                self.rule.as_deref().and_then(Purpose::of_rotation_rule)?,
            )),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountHandoff {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
    #[serde(default)]
    pub openai_scope: bool,
    #[serde(default)]
    pub local: bool,
    pub account: String,
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<HandoffDestination>,
    pub receipt: Option<String>,
    pub turn: Option<String>,
    #[serde(default)]
    pub accepted: bool,
    pub starts: u64,
    pub goal_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffDestination {
    pub account: String,
    pub user: String,
}

impl AccountHandoff {
    fn into_core(
        self,
        purpose: codex_alab_core::account_handoff::Purpose,
    ) -> codex_alab_core::session::AccountHandoff {
        codex_alab_core::session::AccountHandoff {
            purpose,
            limit: self.limit,
            openai_scope: self.openai_scope,
            local: self.local,
            account: self.account,
            user: self.user,
            destination: self.destination.map(|target| {
                codex_alab_core::account_handoff::Destination {
                    account: target.account,
                    user: target.user,
                }
            }),
            receipt: self.receipt,
            turn: self.turn,
            accepted: self.accepted,
            starts: self.starts,
            goal_id: self.goal_id,
        }
    }
}

impl From<codex_alab_core::session::AccountHandoff> for AccountHandoff {
    fn from(value: codex_alab_core::session::AccountHandoff) -> Self {
        Self {
            limit: value.limit,
            openai_scope: value.openai_scope,
            local: value.local,
            account: value.account,
            user: value.user,
            destination: value.destination.map(|target| HandoffDestination {
                account: target.account,
                user: target.user,
            }),
            receipt: value.receipt,
            turn: value.turn,
            accepted: value.accepted,
            starts: value.starts,
            goal_id: value.goal_id,
        }
    }
}

#[cfg(test)]
mod renewal_wire_tests {
    use super::AccountHandoff;

    #[test]
    fn renewal_handoff_matches_native_camel_case_contract() {
        let handoff = AccountHandoff {
            destination: None,
            limit: None,
            openai_scope: true,
            local: true,
            account: "account".into(),
            user: "user".into(),
            receipt: Some("receipt".into()),
            turn: Some("turn".into()),
            accepted: true,
            starts: 1,
            goal_id: Some("goal".into()),
        };
        let wire = serde_json::json!({
            "openaiScope": true,
            "local": true,
            "account": "account",
            "user": "user",
            "receipt": "receipt",
            "turn": "turn",
            "accepted": true,
            "starts": 1,
            "goalId": "goal"
        });
        assert_eq!(serde_json::to_value(&handoff).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<AccountHandoff>(wire).unwrap(),
            handoff
        );
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeOrigin {
    pub turn_id: String,
    pub phase: NativePhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NativePhase {
    Running,
    Complete,
    Interrupted,
    Aborted,
}

fn read_owed<'de, D>(deserializer: D) -> Result<Option<Owed>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.as_object().is_some_and(|owed| {
        owed.get("nativeOrigin")
            .is_some_and(serde_json::Value::is_null)
    }) {
        return Ok(None);
    }
    Ok(serde_json::from_value::<Owed>(value).ok().filter(|owed| {
        owed.native_origin
            .as_ref()
            .is_none_or(|origin| !origin.turn_id.trim().is_empty() && origin.turn_id.len() <= 512)
            && {
                // A `rotation` is told apart by its rule: under any rule but a move's the act is
                // dropped, as an image before `login-rotate@v1` drops one it reads as a quota's.
                let handoff = match (&owed.renewal, &owed.rotation) {
                    (Some(_), Some(_)) => return false,
                    (None, None) => None,
                    _ => match owed.handoff() {
                        Some(handoff) => Some(handoff),
                        None => return false,
                    },
                };
                handoff.is_none_or(|(value, purpose)| {
                    owed.rule.as_deref() == Some(purpose.rule())
                        && !owed.landed
                        && owed.native_origin.as_ref().is_some_and(|origin| {
                            origin.phase == NativePhase::Complete
                                && value
                                    .clone()
                                    .into_core(purpose)
                                    .valid_for_origin(&origin.turn_id)
                        })
                })
            }
    }))
}

#[cfg(test)]
mod account_handoff_wire_tests {
    use super::*;
    use codex_alab_core::account_handoff::AccountHandoff;
    use codex_alab_core::account_handoff::Destination;
    use codex_alab_core::account_handoff::Purpose;

    #[derive(Deserialize)]
    struct Carry {
        #[serde(deserialize_with = "read_owed")]
        owed: Option<Owed>,
    }

    fn quota() -> codex_alab_core::session::OwedAct {
        codex_alab_core::session::OwedAct {
            thread: "thread".into(),
            native_origin: Some(codex_alab_core::session::NativeOrigin {
                turn_id: "original-turn".into(),
                phase: codex_alab_core::session::NativePhase::Complete,
            }),
            renewal: Some(AccountHandoff {
                purpose: Purpose::WeeklyQuota,
                limit: None,
                openai_scope: true,
                local: false,
                account: "a".repeat(64),
                user: "b".repeat(64),
                destination: Some(Destination {
                    account: "c".repeat(64),
                    user: "d".repeat(64),
                }),
                receipt: Some("e".repeat(32)),
                turn: None,
                accepted: false,
                starts: 1,
                goal_id: Some("original-goal".into()),
            }),
            rule: Some(Purpose::WeeklyQuota.rule().into()),
            mode: Some("cruise".into()),
            ..Default::default()
        }
    }

    #[test]
    fn quota_handoff_round_trips_with_its_destination_and_original_context() {
        let expected = quota();
        let wire = serde_json::to_value(Owed::from(expected.clone())).unwrap();
        assert!(wire.get("renewal").is_none());
        assert!(wire["rotation"].get("limit").is_none());
        assert_eq!(wire["rotation"]["destination"]["account"], "c".repeat(64));
        let restored: Carry = serde_json::from_value(serde_json::json!({"owed":wire})).unwrap();
        assert_eq!(restored.owed.map(Into::into), Some(expected));
    }

    #[test]
    fn named_limit_round_trips_only_with_a_quota_rotation() {
        let mut expected = quota();
        expected.renewal.as_mut().unwrap().limit = Some("codex_other".into());
        let wire = serde_json::to_value(Owed::from(expected.clone())).unwrap();
        assert_eq!(wire["rotation"]["limit"], "codex_other");
        let restore = |wire: serde_json::Value| {
            serde_json::from_value::<Carry>(serde_json::json!({"owed": wire}))
                .unwrap()
                .owed
                .map(codex_alab_core::session::OwedAct::from)
        };
        assert_eq!(restore(wire.clone()), Some(expected));
        for invalid in ["", "codex", "codex-other", "UPPER"] {
            let mut malformed = wire.clone();
            malformed["rotation"]["limit"] = invalid.into();
            assert!(restore(malformed).is_none(), "{invalid:?}");
        }
        let mut login_rotation = wire;
        login_rotation["rule"] = Purpose::LoginRotation.rule().into();
        assert!(restore(login_rotation).is_none());

        let mut login = quota();
        let handoff = login.renewal.as_mut().unwrap();
        handoff.purpose = Purpose::Login;
        handoff.destination = None;
        login.rule = Some(Purpose::Login.rule().into());
        let mut wire = serde_json::to_value(Owed::from(login)).unwrap();
        assert!(restore(wire.clone()).is_some());
        wire["renewal"]["limit"] = "codex_other".into();
        assert!(restore(wire).is_none());
    }

    /// A LOST LOGIN'S MOVE IS KEPT UNDER `rotation`, TOLD APART BY ITS RULE: it round-trips with
    /// its destination as `login-rotate@v1`, and the weekly-quota handoff's wire is unchanged
    /// (`rotation` under `usage-rotate@v1`). A `rotation` under any other rule (an older or newer
    /// image's, or the same account's renewal rule) is dropped whole, so no image types on an act
    /// it cannot place; so is a record that carries both fields.
    #[test]
    fn a_login_rotation_round_trips_under_rotation_by_its_rule() {
        let mut moved = quota();
        let handoff = moved.renewal.as_mut().unwrap();
        handoff.purpose = Purpose::LoginRotation;
        moved.rule = Some(Purpose::LoginRotation.rule().into());
        moved.text = Some("Continue the original task.".into());
        let wire = serde_json::to_value(Owed::from(moved.clone())).unwrap();
        assert!(wire.get("renewal").is_none());
        assert_eq!(wire["rule"], "login-rotate@v1");
        assert_eq!(wire["rotation"]["destination"]["account"], "c".repeat(64));
        assert!(
            wire["rotation"].get("purpose").is_none(),
            "the rule names it"
        );
        let restored: Carry = serde_json::from_value(serde_json::json!({"owed":wire})).unwrap();
        let restored = restored.owed.map(codex_alab_core::session::OwedAct::from);
        assert_eq!(restored.as_ref(), Some(&moved));
        assert_eq!(
            restored
                .and_then(|owed| owed.renewal)
                .map(|handoff| handoff.purpose),
            Some(Purpose::LoginRotation)
        );

        let quota_wire = serde_json::to_value(Owed::from(quota())).unwrap();
        assert_eq!(quota_wire["rule"], "usage-rotate@v1");
        assert_eq!(
            quota_wire["rotation"], wire["rotation"],
            "the same handoff wire"
        );

        for rule in ["login-back@v1", "login-rotate@v2", "continue@v1"] {
            let mut other = wire.clone();
            other["rule"] = serde_json::json!(rule);
            let restored: Carry =
                serde_json::from_value(serde_json::json!({"owed":other})).unwrap();
            assert!(restored.owed.is_none(), "{rule}");
        }
        let mut unruled = wire.clone();
        unruled.as_object_mut().unwrap().remove("rule");
        let restored: Carry = serde_json::from_value(serde_json::json!({"owed":unruled})).unwrap();
        assert!(restored.owed.is_none(), "no rule");
        let mut both = wire;
        both["renewal"] = both["rotation"].clone();
        let restored: Carry = serde_json::from_value(serde_json::json!({"owed":both})).unwrap();
        assert!(restored.owed.is_none(), "both fields");
        // The same account's renewal is never kept under `rotation`.
        let mut renewal = quota();
        let handoff = renewal.renewal.as_mut().unwrap();
        handoff.purpose = Purpose::Login;
        handoff.destination = None;
        renewal.rule = Some(Purpose::Login.rule().into());
        let renewal_wire = serde_json::to_value(Owed::from(renewal)).unwrap();
        assert!(renewal_wire.get("rotation").is_none());
        assert!(renewal_wire.get("renewal").is_some());
    }

    #[test]
    fn ambiguous_same_account_and_wrong_turn_rotation_records_are_refused() {
        for case in 0..3 {
            let mut wire = serde_json::to_value(Owed::from(quota())).unwrap();
            match case {
                0 => wire["renewal"] = wire["rotation"].clone(),
                1 => wire["rotation"]["destination"]["account"] = serde_json::json!("a".repeat(64)),
                _ => wire["rotation"]["turn"] = serde_json::json!("unrelated-turn"),
            }
            let restored: Carry = serde_json::from_value(serde_json::json!({"owed":wire})).unwrap();
            assert!(restored.owed.is_none());
        }
    }
}

impl From<codex_alab_core::session::NativeOrigin> for NativeOrigin {
    fn from(origin: codex_alab_core::session::NativeOrigin) -> Self {
        Self {
            turn_id: origin.turn_id,
            phase: match origin.phase {
                codex_alab_core::session::NativePhase::Running => NativePhase::Running,
                codex_alab_core::session::NativePhase::Complete => NativePhase::Complete,
                codex_alab_core::session::NativePhase::Interrupted => NativePhase::Interrupted,
                codex_alab_core::session::NativePhase::Aborted => NativePhase::Aborted,
            },
        }
    }
}

impl From<NativeOrigin> for codex_alab_core::session::NativeOrigin {
    fn from(origin: NativeOrigin) -> Self {
        Self {
            turn_id: origin.turn_id,
            phase: match origin.phase {
                NativePhase::Running => codex_alab_core::session::NativePhase::Running,
                NativePhase::Complete => codex_alab_core::session::NativePhase::Complete,
                NativePhase::Interrupted => codex_alab_core::session::NativePhase::Interrupted,
                NativePhase::Aborted => codex_alab_core::session::NativePhase::Aborted,
            },
        }
    }
}

impl From<codex_alab_core::session::OwedAct> for Owed {
    fn from(owed: codex_alab_core::session::OwedAct) -> Self {
        let (renewal, rotation) = match owed.renewal {
            Some(handoff) if handoff.purpose.moves_account() => (None, Some(handoff.into())),
            handoff => (handoff.map(Into::into), None),
        };
        Owed {
            thread: owed.thread,
            native_origin: owed.native_origin.map(Into::into),
            renewal,
            rotation,
            rule: owed.rule,
            text: owed.text,
            mode: owed.mode,
            not_before: owed.not_before_ms,
            repo_owed: owed.repo_owed,
            resume_goal: owed.resume_goal,
            landing: owed.landing,
            landed: owed.landed,
        }
    }
}

impl From<Owed> for codex_alab_core::session::OwedAct {
    fn from(owed: Owed) -> Self {
        use codex_alab_core::account_handoff::Purpose;
        let handoff = match (owed.renewal, owed.rotation) {
            (Some(value), None) => Some(value.into_core(Purpose::Login)),
            (None, Some(value)) => owed
                .rule
                .as_deref()
                .and_then(Purpose::of_rotation_rule)
                .map(|purpose| value.into_core(purpose)),
            _ => None,
        };
        codex_alab_core::session::OwedAct {
            thread: owed.thread,
            native_origin: owed.native_origin.map(Into::into),
            renewal: handoff,
            rule: owed.rule,
            text: owed.text,
            mode: owed.mode,
            not_before_ms: owed.not_before,
            repo_owed: owed.repo_owed,
            resume_goal: owed.resume_goal,
            landing: owed.landing,
            landed: owed.landed,
        }
    }
}

impl Record {
    /// Equal but for `updatedAt` and `lastHumanInputAt` (a rewrite with nothing new is
    /// skipped). The human stamp rides along on a write another change causes: aterm reads
    /// none, and a person's typing or scrolling restamps it at every key.
    pub fn same_content(&self, other: &Self) -> bool {
        let mut other = other.clone();
        other.updated_at = self.updated_at;
        other.last_human_input_at = self.last_human_input_at;
        *self == other
    }

    /// [`Self::same_content`] but for `backgroundTerminals` too: `other` only moves the count,
    /// which the writer leaves unwritten while that move is held (CONTRACT §4,
    /// [`codex_alab_core::session::Shared::background_terminals_held`]).
    pub fn same_but_background_terminals(&self, other: &Self) -> bool {
        let mut other = other.clone();
        other.background_terminals = self.background_terminals;
        self.same_content(&other)
    }
}

/// Create the record directory (0700, parents too) and tighten it if it already exists.
pub fn ensure_dir(dir: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    if !fs::symlink_metadata(dir)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session record directory is not a regular directory",
        ));
    }
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

/// Write `record` durably and atomically: a 0600 temporary file beside it,
/// synced, renamed, then followed by the platform's directory durability boundary.
pub fn write_atomic(dir: &Path, record: &Record) -> io::Result<()> {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let bytes = serde_json::to_vec(record).map_err(io::Error::other)?;
    let target = path_for(dir, record.pid);
    let nonce = format!(
        "{:x}{:x}",
        codex_alab_core::session::now_ms(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    );
    let temporary = dir.join(format!("{}.json.{nonce}.tmp", record.pid));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &target)?;
        // The production path is CODEX_HOME/alab/sessions. Unix syncs this
        // directory and its two parents; Windows retains the already-synced
        // file because it exposes no documented standard-user directory flush.
        let codex_home = dir
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| io::Error::other("session record directory is outside Codex home"))?;
        codex_alab_core::goal_chronology_fence::sync_parent_chain(codex_home, &target)?;
        Ok(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Maximum serialized record size (matching `aterm`'s `self_claimed::RECORD_MAX_BYTES`).
const RECORD_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Read and parse a record (`None` if it is missing or not a v1 record).
pub fn read(path: &Path) -> Option<Record> {
    use std::io::Read;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > RECORD_MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    file.take(RECORD_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > RECORD_MAX_BYTES {
        return None;
    }
    let record: Record = serde_json::from_slice(&bytes).ok()?;
    (record.v == SCHEMA_VERSION).then_some(record)
}

/// Remove `pid`'s record; a missing one is fine.
pub fn remove(dir: &Path, pid: u32) -> io::Result<()> {
    match fs::remove_file(path_for(dir, pid)) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// An orderly empty revocation kept only when predecessor cleanup is pending.
pub const HANDOFF_CLEARED: &str = "handoff-cleared";

/// Retire only the exact dead record captured by a startup. Rename gives this
/// cleanup custody before validation, so a replacement cannot be unlinked by a
/// check against the previous contents of the canonical pid path.
#[cfg(test)]
pub(crate) fn retire_if_unchanged(
    dir: &Path,
    source: &Record,
    is_live: impl Fn(u32, Option<&ProcessIdentity>) -> bool,
    after_rename: impl FnOnce(),
) -> io::Result<bool> {
    retire_path_if_unchanged(
        dir,
        &path_for(dir, source.pid),
        source,
        is_live,
        after_rename,
    )
}

pub(crate) fn retire_held_source(
    dir: &Path,
    source: &Record,
    is_live: impl Fn(u32, Option<&ProcessIdentity>) -> bool,
) -> io::Result<()> {
    // A previous cleanup interrupted in quarantine is still a normal sweep
    // candidate. Collect before renaming so we never visit our new quarantine.
    let paths = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_str().and_then(owner_pid) == Some(source.pid))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    for path in paths {
        if read(&path).as_ref() == Some(source) {
            retire_path_if_unchanged(dir, &path, source, &is_live, || {})?;
        }
    }
    Ok(())
}

fn retire_path_if_unchanged(
    dir: &Path,
    path: &Path,
    source: &Record,
    is_live: impl Fn(u32, Option<&ProcessIdentity>) -> bool,
    after_rename: impl FnOnce(),
) -> io::Result<bool> {
    let identity = source.identity.clone().map(ProcessIdentity::from);
    capture_path_if(
        dir,
        path,
        source.pid,
        CapturedAction::Retire,
        |captured| {
            read(captured).as_ref() == Some(source) && !is_live(source.pid, identity.as_ref())
        },
        after_rename,
    )
}

enum CapturedAction {
    Retire,
    Restore,
}

/// Take custody before validating the observed publication. A held observation
/// is usable only after restoring that same object into an empty original slot.
fn capture_path_if(
    dir: &Path,
    path: &Path,
    pid: u32,
    action: CapturedAction,
    valid: impl FnOnce(&Path) -> bool,
    after_rename: impl FnOnce(),
) -> io::Result<bool> {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    // The ordinary sweep recognizes this temporary too. Even an interrupted
    // restoration leaves the captured record discoverable by a later startup.
    let quarantine = loop {
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let candidate = dir.join(format!(
            "{}.json.{}-{}-{nonce}.tmp",
            pid,
            std::process::id(),
            codex_alab_core::session::now_ms()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(_) => break candidate,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    };
    if let Err(err) = fs::rename(path, &quarantine) {
        let _ = fs::remove_file(&quarantine);
        return if err.kind() == io::ErrorKind::NotFound {
            Ok(false)
        } else {
            Err(err)
        };
    }
    after_rename();
    let valid = valid(&quarantine);
    if valid && matches!(action, CapturedAction::Retire) {
        fs::remove_file(&quarantine)?;
        return Ok(true);
    }
    // A new identity/content belongs to its writer. Restore only into an empty
    // slot; a newer canonical record always wins and the quarantine is preserved.
    match fs::hard_link(&quarantine, path) {
        Ok(()) => {
            let _ = fs::remove_file(&quarantine);
            return Ok(valid);
        }
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err),
    }
    Ok(false)
}

/// Renaming into quarantine changes ctime, so compare the inode and its observed
/// type, size and modification time instead. Parsed equality alone loses fields
/// belonging to a future writer; an unreadable candidate has no parsed identity.
fn same_file_snapshot(observed: &fs::Metadata, captured: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(captured).is_ok_and(|current| {
        observed.dev() == current.dev()
            && observed.ino() == current.ino()
            && observed.mode() == current.mode()
            && observed.len() == current.len()
            && observed.mtime() == current.mtime()
            && observed.mtime_nsec() == current.mtime_nsec()
    })
}

/// The pid a record file (`<pid>.json`) or its temporary (`<pid>.json.<nonce>.tmp`) belongs to.
fn owner_pid(name: &str) -> Option<u32> {
    let stem = name.strip_suffix(".json").or_else(|| {
        let (pid, nonce) = name.strip_suffix(".tmp")?.split_once(".json.")?;
        (!nonce.is_empty()).then_some(pid)
    })?;
    if stem.is_empty()
        || (stem.len() > 1 && stem.starts_with('0'))
        || !stem.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    stem.parse().ok().filter(|&pid| pid > 0)
}

/// Remove the records (and stray temporaries) of dead identities, any pid but `own_pid`,
/// last written before `before_ms` (this process's start). A record whose identity is live
/// is kept, and so is one written after this process started. Returns how many it removed.
pub fn sweep(
    dir: &Path,
    own_pid: u32,
    before_ms: u64,
    is_live: impl Fn(u32, Option<&ProcessIdentity>) -> bool,
) -> usize {
    sweep_with(dir, own_pid, before_ms, is_live, |_| Sweep::Take, |_, _| {})
}

/// What the start-up sweep does with one dead record ([`sweep_with`], [`judge`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sweep {
    /// This process's own predecessor: removed, and handed on (its crash witness).
    Take,
    /// Handed on, but retained until its successor commits a replacement for
    /// durable recovery (the module's persistence scope).
    Hold,
    /// A relaunch another process may still be owed: left for that process's start.
    Leave,
    /// Nobody's any more: removed.
    Remove,
}

/// The start-up sweep's verdict on one dead record, for a process started at `started_ms`
/// with `own_argv`. aterm relaunches each tab by typing its record's `resumeArgv`, so the
/// record whose `resumeArgv` is exactly this process's argv is this process's own
/// predecessor; one naming another line is another tab's (when aterm relaunches several
/// codex-alab tabs together, each start must find its own, not the first start take them
/// all), and stands until its own relaunch takes it or until it is older than `horizon`
/// (no relaunch is owed from it any more: [`codex_alab_core::session::RELAUNCH_HORIZON`]).
/// A record with no `resumeArgv` names no relaunch, and neither does one whose end aterm reads
/// as orderly ([`relaunch_owed`]): another tab's such record is removed at once, not kept for a
/// day (it would only keep its package from the pruner). This process's own is taken whatever
/// it says (its witness is the carry-on's to judge).
pub fn judge(record: &Record, own_argv: &[OsString], started_ms: u64, horizon: u64) -> Sweep {
    let Some(argv) = &record.resume_argv else {
        return Sweep::Remove;
    };
    let own = argv.len() == own_argv.len()
        && argv
            .iter()
            .zip(own_argv)
            .all(|(want, got)| got.to_str() == Some(want.as_str()));
    if own {
        Sweep::Take
    } else if (relaunch_owed(record)
        || (record.status == Status::Exited && record.exit.as_deref() == Some(HANDOFF_CLEARED)))
        && started_ms.saturating_sub(record.updated_at) <= horizon
    {
        Sweep::Leave
    } else {
        Sweep::Remove
    }
}

/// Whether aterm may still type a relaunch line for `record` (CONTRACT §4): no orderly end
/// is written, so the crash rule may relaunch it: `exit` absent (a panic or SIGKILL, or a
/// surviving snapshot after an OS crash or power loss), `signal:15` (aterm's stall or memory
/// remedy, which may fire at an idle point too) or
/// `signal:1` (aterm's restore after its own crash relaunches it), and `status` not `exited`.
/// A `fatal` exit owes none: the crash rule reads it as orderly, and the Codex lane that read
/// its status 1 as a crash and relaunched it under its back-off is retired (aterm
/// 83bd43c206; CONTRACT §4 as amended 2026-10-17), so its record is removed at the next start
/// like any orderly end's (the review of 363789134 kept it a day for that lane). A SIGINT (a
/// person's) or any other signal aterm never sends is orderly: no relaunch is owed. Whether
/// that relaunch would also carry on (a busy predecessor, no chain) does not matter here:
/// aterm types the line either way, and the pruner must keep the package it names.
pub fn relaunch_owed(record: &Record) -> bool {
    record.status != Status::Exited
        && codex_alab_core::session::exit_relaunchable(record.exit.as_deref())
}

/// [`sweep`], asking `verdict` what to do with each dead record that could be read
/// ([`judge`]; one that cannot be read is removed), and handing each one it took or held to
/// `on_taken` (with its pid). Returns how many it removed.
pub fn sweep_with(
    dir: &Path,
    own_pid: u32,
    before_ms: u64,
    is_live: impl Fn(u32, Option<&ProcessIdentity>) -> bool,
    verdict: impl Fn(&Record) -> Sweep,
    mut on_taken: impl FnMut(u32, &Record),
) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    // A restoration conflict can retain our new quarantine. It belongs to a
    // later census, not another iteration of this same startup's observations.
    let entries = entries.flatten().collect::<Vec<_>>();
    let mut removed = 0;
    for entry in entries {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(owner_pid) else {
            continue;
        };
        if pid == own_pid {
            continue;
        }
        let path = entry.path();
        let Ok(observed) = fs::symlink_metadata(&path) else {
            continue;
        };
        let record = read(&path).filter(|record| record.pid == pid);
        let written_ms = record.as_ref().map(|record| record.updated_at).or_else(|| {
            let modified = observed.modified().ok()?;
            let elapsed = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
            u64::try_from(elapsed.as_millis()).ok()
        });
        if written_ms.is_none_or(|written| written >= before_ms) {
            continue;
        }
        let identity = record
            .as_ref()
            .and_then(|record| record.identity.clone())
            .map(ProcessIdentity::from);
        if is_live(pid, identity.as_ref()) {
            continue;
        }
        let verdict = record.as_ref().map_or(Sweep::Remove, &verdict);
        if verdict == Sweep::Leave {
            continue;
        }
        let action = if verdict == Sweep::Hold {
            CapturedAction::Restore
        } else {
            CapturedAction::Retire
        };
        let captured = capture_path_if(
            dir,
            &path,
            pid,
            action,
            |captured| {
                same_file_snapshot(&observed, captured)
                    && read(captured).filter(|record| record.pid == pid).as_ref() == record.as_ref()
                    && !is_live(pid, identity.as_ref())
            },
            || {},
        );
        if matches!(captured, Ok(true)) {
            if verdict != Sweep::Hold {
                removed += 1;
            }
            if let (Sweep::Take | Sweep::Hold, Some(record)) = (verdict, &record) {
                on_taken(pid, record);
            }
        }
    }
    removed
}
