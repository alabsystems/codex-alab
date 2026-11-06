// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! THE HOST'S SLEEP, MEASURED: when this machine last woke, and how long it slept.
//!
//! `std::time::Instant`, tokio's clock and every timer armed on them stop while macOS sleeps
//! (`CLOCK_UPTIME_RAW`) and while Linux is suspended (`CLOCK_MONOTONIC`). A wait armed before a
//! sleep therefore comes due after the wake only once it has had all the awake time it still
//! had, however long the host slept, and a wait that comes due while the host is up for a few
//! seconds in a dark wake (the lid closed, often with no network) runs there. Both happened to a
//! usage-limit continuation on 2026-10-24 (alab/CAPACITY_RECOVERY.md, "Sleep and wake"): it fired
//! inside a 19 s dark wake with the network down, its request then sat in backoff across the
//! following sleeps, and the person pressed Esc to get the session going again.
//!
//! The kernel counts the time asleep. On macOS `CLOCK_MONOTONIC_RAW` runs on through a sleep at
//! the rate at which `CLOCK_UPTIME_RAW` runs awake; on Linux `CLOCK_BOOTTIME` runs on through a
//! suspend where `CLOCK_MONOTONIC` stops. Their difference is the time asleep since boot
//! ([`asleep_total`]), and no wall-clock step moves it.
//!
//! A reading ([`observe`]) that finds that total grown by [`MIN_SLEEP`] or more since the reading
//! before records a [`Wake`]: when the host woke, on the `Instant` clock, and how long it slept.
//! Readers poll only while they wait: a wait reads every [`POLL`] ([`sleep_until`], [`timeout`]),
//! a pending autopilot timer every [`POLL`], and the autopilot at each decision. Nothing reads
//! while nothing waits, and nothing here starts a thread, so a reading can come long after the
//! wake it finds (a turn streamed, or the session stood idle). The wake is therefore dated by the
//! kernel's own record of it where there is one (macOS: `kern.wake_abs_time`, the awake clock at
//! the last wake, [`awake_since_wake`]), whenever it is read; where there is none (Linux), by the
//! reading itself, the latest the wake can have been, so a hold counted from it is longer than
//! it need be, never shorter. On a platform with no count of the time asleep every reading is
//! `None`: no wake is ever measured, and every caller behaves as it did before this module.

use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

/// A growth of the time asleep shorter than this between two readings is the two clocks read a
/// moment apart, not a sleep.
pub const MIN_SLEEP: Duration = Duration::from_secs(1);
/// How often a waiting reader reads the count: the most a wake is seen late.
pub const POLL: Duration = Duration::from_secs(1);
/// Whether this platform has the clock pair, independently of a transient failed sample.
#[must_use]
pub const fn supported() -> bool {
    cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "android"
    ))
}
/// After a wake, how long the host must stay awake before codex-alab types an act a wake can
/// misfire (aterm's `WAKE_SETTLE`). A dark wake lasts seconds (2 s and 19 s on 2026-10-24), so an
/// act held this long after every wake is never typed in one.
pub const WAKE_SETTLE: Duration = Duration::from_secs(30);
/// After a wake, the pause before a cut-short retry: the network's own wake (Wi-Fi joining,
/// DHCP) takes about that long.
pub const AFTER_WAKE: Duration = Duration::from_secs(2);

/// A sleep a reading found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wake {
    /// When the host woke, on the `Instant` clock: the kernel's record of its last wake where
    /// it keeps one and that record is after the reading before (which found the host awake),
    /// else when the reading that found the sleep ran (the latest the wake can have been).
    pub at: Instant,
    /// How long the host slept since the reading before.
    pub slept: Duration,
    /// The process's count of wakes found, this one included: a reader that saw an older count
    /// knows a wake came since.
    pub seq: u64,
}

/// The readings so far (pure: every reading is passed in).
#[derive(Debug, Default, Clone)]
pub struct WakeState {
    asleep: Option<Duration>,
    /// When the last accepted reading ran: the host was awake then, so a wake found by the
    /// next reading came after it.
    read_at: Option<Instant>,
    last: Option<Wake>,
    seq: u64,
}

impl WakeState {
    /// The time asleep since boot read as `asleep` at `now`: the last wake found, this reading's
    /// own when the total grew by [`MIN_SLEEP`] or more since the reading before, dated `now`
    /// (the kernel's record of the wake is [`Self::observe_woke`]'s). The first reading only
    /// sets the baseline. A lower, stale sample cannot rewind it and make a later reading credit
    /// the same sleep again.
    pub fn observe(&mut self, now: Instant, asleep: Duration) -> Option<Wake> {
        self.observe_woke(now, asleep, || None)
    }

    /// [`Self::observe`], with `awake` the kernel's count of the time the host has been awake
    /// since its last wake ([`awake_since_wake`]), asked only when this reading finds a sleep:
    /// the wake is dated that long before `now`, unless that date is not after the reading
    /// before (which found the host awake, so it is no record of this wake) or the kernel keeps
    /// none; then it is dated `now`, the latest it can have been.
    pub fn observe_woke(
        &mut self,
        now: Instant,
        asleep: Duration,
        awake: impl FnOnce() -> Option<Duration>,
    ) -> Option<Wake> {
        if self.asleep.is_some_and(|before| asleep < before) {
            return self.last;
        }
        let before = self.asleep.replace(asleep);
        let read_before = self.read_at.replace(now);
        if let Some(before) = before
            && let Some(slept) = asleep.checked_sub(before)
            && slept >= MIN_SLEEP
        {
            let at = awake()
                .and_then(|awake| now.checked_sub(awake))
                .filter(|at| read_before.is_some_and(|read| *at >= read))
                .unwrap_or(now);
            self.seq = self.seq.saturating_add(1);
            self.last = Some(Wake {
                at,
                slept,
                seq: self.seq,
            });
        }
        self.last
    }

    /// The last wake found, without a reading.
    #[must_use]
    pub fn last(&self) -> Option<Wake> {
        self.last
    }
}

static STATE: Mutex<WakeState> = Mutex::new(WakeState {
    asleep: None,
    read_at: None,
    last: None,
    seq: 0,
});

/// The host's time asleep since boot, where the platform counts it.
#[must_use]
pub fn asleep_total() -> Option<Duration> {
    #[cfg(target_os = "macos")]
    {
        sample_asleep_with(|clock| {
            clock_ns(match clock {
                SleepClock::Through => libc::CLOCK_MONOTONIC_RAW,
                SleepClock::Awake => libc::CLOCK_UPTIME_RAW,
            })
        })
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        sample_asleep_with(|clock| {
            clock_ns(match clock {
                SleepClock::Through => libc::CLOCK_BOOTTIME,
                SleepClock::Awake => libc::CLOCK_MONOTONIC,
            })
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
    {
        None
    }
}

/// How long the host has been awake since its last wake, where the kernel records that wake:
/// on macOS `kern.wake_abs_time` is the awake clock (`mach_absolute_time`, which
/// `CLOCK_UPTIME_RAW` and `Instant` count in nanoseconds) at the last wake, dark wakes
/// included, in timebase ticks of `hw.tbfrequency` a second. `None` before any wake, and
/// always where the kernel keeps no such record (Linux).
#[must_use]
pub fn awake_since_wake() -> Option<Duration> {
    #[cfg(target_os = "macos")]
    {
        let woke = sysctl_u64(c"kern.wake_abs_time").filter(|ticks| *ticks > 0)?;
        let hz = sysctl_u64(c"hw.tbfrequency").filter(|hz| *hz > 0)?;
        let woke_ns = u128::from(woke).checked_mul(1_000_000_000)? / u128::from(hz);
        let now_ns = u128::from(clock_ns(libc::CLOCK_UPTIME_RAW)?);
        let awake = u64::try_from(now_ns.checked_sub(woke_ns)?).ok()?;
        Some(Duration::from_nanos(awake))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// An integer sysctl, read by name (4 or 8 bytes wide).
#[cfg(target_os = "macos")]
fn sysctl_u64(name: &std::ffi::CStr) -> Option<u64> {
    let mut buf = [0_u8; 8];
    let mut len: libc::size_t = buf.len();
    // SAFETY: `name` is NUL-terminated, `buf` is writable for `len` bytes and `len` says so, so
    // the kernel writes at most that many; no new value is given (null, 0).
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    match len {
        8 => Some(u64::from_ne_bytes(buf)),
        4 => {
            let [a, b, c, d, ..] = buf;
            Some(u64::from(u32::from_ne_bytes([a, b, c, d])))
        }
        _ => None,
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
#[derive(Clone, Copy)]
enum SleepClock {
    Awake,
    Through,
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
fn sample_asleep_with(mut clock: impl FnMut(SleepClock) -> Option<u64>) -> Option<Duration> {
    let before = clock(SleepClock::Awake)?;
    let through = clock(SleepClock::Through)?;
    let after = clock(SleepClock::Awake)?;
    // A preemption between clock reads is awake time, not host sleep. Reject
    // uncertain samples well below the one-second wake threshold.
    if after.checked_sub(before)? >= 1_000_000 {
        return None;
    }
    Some(Duration::from_nanos(through.saturating_sub(after)))
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
fn clock_ns(clock: libc::clockid_t) -> Option<u64> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid, writable timespec for the duration of the call, and the clock
    // ids are the platform's own constants.
    if unsafe { libc::clock_gettime(clock, &raw mut ts) } != 0 {
        return None;
    }
    let secs = u64::try_from(ts.tv_sec).ok()?;
    let nanos = u64::try_from(ts.tv_nsec).ok()?;
    secs.checked_mul(1_000_000_000)?.checked_add(nanos)
}

/// One reading of the count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// The host's time asleep since boot.
    pub asleep: Duration,
    /// The last wake this process found, this reading's own when the host slept since the
    /// reading before.
    pub last: Option<Wake>,
}

/// Read the count now. `None` where the platform has no count.
pub fn read() -> Option<Reading> {
    read_with(&STATE, asleep_total, awake_since_wake)
}

/// [`read`] from `sample` (the time asleep since boot) and `awake` (the kernel's time awake since
/// the last wake, asked only when the reading finds a sleep).
fn read_with(
    state: &Mutex<WakeState>,
    sample: impl FnOnce() -> Option<Duration>,
    awake: impl FnOnce() -> Option<Duration>,
) -> Option<Reading> {
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Sampling and observation have one order across readers. Publish only the
    // accepted total so downstream readers cannot rewind their own sleep marks.
    let asleep = sample()?;
    let now = Instant::now();
    let last = state.observe_woke(now, asleep, awake);
    Some(Reading {
        asleep: state.asleep?,
        last,
    })
}

/// Read the count now: the last wake this process found, this reading's own when the host slept
/// since the reading before. `None` before any wake, and always where the platform has no count.
pub fn observe() -> Option<Wake> {
    read().and_then(|reading| reading.last)
}

/// An absolute deadline and the host's sleep count when it was selected. The
/// original instant is retained so repeated reads never credit one sleep twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline {
    at: Instant,
    asleep: Option<Duration>,
}

impl Deadline {
    /// Capture the accepted sleep count alongside an absolute deadline.
    #[must_use]
    pub fn new(at: Instant) -> Self {
        Self::with_reading(at, read())
    }

    /// Preserve a consumer's original instant without measuring the host's sleep.
    #[must_use]
    pub fn untracked(at: Instant) -> Self {
        Self::with_reading(at, None)
    }

    /// Capture a supplied accepted reading (the deterministic clock seam).
    #[must_use]
    pub fn with_reading(at: Instant, reading: Option<Reading>) -> Self {
        Self {
            at,
            asleep: reading.map(|reading| reading.asleep),
        }
    }

    /// Retain a known origin, or establish one from the first available handoff reading.
    #[must_use]
    pub fn with_baseline(self, reading: Option<Reading>) -> Self {
        Self {
            asleep: self.asleep.or(reading.map(|reading| reading.asleep)),
            ..self
        }
    }

    /// The deadline after counting sleep since its selection, without a wake floor.
    #[must_use]
    pub fn current(self) -> Instant {
        if self.asleep.is_none() {
            return self.at;
        }
        self.at_reading(read(), Instant::now())
    }

    /// Evaluate against a supplied accepted reading. Missing or lower counts do
    /// not shorten the deadline; an underflow is already due at `now`.
    #[must_use]
    pub fn at_reading(self, reading: Option<Reading>, now: Instant) -> Instant {
        let slept = self
            .asleep
            .zip(reading)
            .and_then(|(before, reading)| reading.asleep.checked_sub(before))
            .filter(|slept| *slept >= MIN_SLEEP)
            .unwrap_or_default();
        self.at.checked_sub(slept).unwrap_or(now.min(self.at))
    }
}

/// THE DEADLINE A WAKE LEAVES A WAIT: `deadline`, armed on the `Instant` clock before the sleep
/// `wake` found, as a wait of real time: the time slept counts toward it, so it comes that much
/// sooner, but never sooner than [`AFTER_WAKE`] past the wake (the network's own wake) and never
/// later than it was.
#[must_use]
pub fn deadline_after(deadline: Instant, wake: &Wake) -> Instant {
    let floor = wake.at.checked_add(AFTER_WAKE).unwrap_or(wake.at);
    deadline
        .checked_sub(wake.slept)
        .unwrap_or(wake.at)
        .max(floor)
        .min(deadline)
}

/// A WAIT THE HOST'S SLEEP COUNTS TOWARD: until `deadline`, or sooner by the time the host slept
/// meanwhile ([`deadline_after`]), so a backoff a sleep interrupted ends [`AFTER_WAKE`] after a
/// wake that outlasted it instead of when the awake clock has made up its remainder. Returns
/// whether a wake cut it short. Where the platform counts no sleep, it is tokio's
/// `sleep_until(deadline)`.
pub async fn sleep_until(deadline: tokio::time::Instant) -> bool {
    ArmedWait::new(deadline).wait().await
}

/// A wait whose sleep baseline is captured synchronously, before a caller awaits
/// reporting the selected deadline or first polls the waiting future.
#[derive(Debug, Clone, Copy)]
pub struct ArmedWait {
    deadline: WaitDeadline,
    minimum: Option<WaitDeadline>,
    counts_sleep: bool,
}

#[derive(Debug, Clone, Copy)]
struct WaitDeadline {
    at: tokio::time::Instant,
    asleep: Option<Duration>,
}

impl WaitDeadline {
    fn observe(&mut self, reading: Reading, now: tokio::time::Instant) {
        let Some(before) = self.asleep else {
            self.asleep = Some(reading.asleep);
            return;
        };
        let Some(slept) = reading
            .asleep
            .checked_sub(before)
            .filter(|slept| *slept >= MIN_SLEEP)
        else {
            return;
        };
        self.asleep = Some(reading.asleep);
        let wake = Wake {
            at: reading.last.map_or(now.into_std(), |wake| wake.at),
            slept,
            seq: reading.last.map_or(0, |wake| wake.seq),
        };
        self.at = tokio::time::Instant::from_std(deadline_after(self.at.into_std(), &wake));
    }
}

impl ArmedWait {
    /// Capture the host's accepted count now, alongside the selected deadline.
    #[must_use]
    pub fn new(deadline: tokio::time::Instant) -> Self {
        Self::capture_with(deadline, supported(), read)
    }

    fn capture_with(
        deadline: tokio::time::Instant,
        counts_sleep: bool,
        read: impl FnOnce() -> Option<Reading>,
    ) -> Self {
        Self {
            deadline: WaitDeadline {
                at: deadline,
                asleep: counts_sleep
                    .then(read)
                    .flatten()
                    .map(|reading| reading.asleep),
            },
            minimum: None,
            counts_sleep,
        }
    }

    /// Normalize retained advice and capture its wait from one accepted reading.
    /// A local minimum has its own origin, so prior provider sleep cannot consume it.
    #[must_use]
    pub fn from_deadline(deadline: Deadline, minimum: Option<tokio::time::Instant>) -> Self {
        Self::from_deadline_with(deadline, minimum, supported(), read)
    }

    fn from_deadline_with(
        deadline: Deadline,
        minimum: Option<tokio::time::Instant>,
        counts_sleep: bool,
        read: impl FnOnce() -> Option<Reading>,
    ) -> Self {
        let reading = counts_sleep.then(read).flatten();
        let asleep = reading.map(|reading| reading.asleep);
        Self {
            deadline: WaitDeadline {
                at: tokio::time::Instant::from_std(
                    deadline.at_reading(reading, tokio::time::Instant::now().into_std()),
                ),
                // A rejected selection sample cannot erase a known receipt origin.
                asleep: match (asleep, deadline.asleep) {
                    (Some(current), Some(receipt)) => Some(current.max(receipt)),
                    (current, receipt) => current.or(receipt),
                },
            },
            minimum: minimum.map(|at| WaitDeadline { at, asleep }),
            counts_sleep,
        }
    }

    /// The effective selected deadline, before the waiting future starts polling.
    #[must_use]
    pub fn deadline(self) -> tokio::time::Instant {
        self.minimum
            .map_or(self.deadline.at, |minimum| self.deadline.at.max(minimum.at))
    }

    /// Select retained advice from an injected clock without a second sample.
    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub fn from_deadline_with_clock(
        deadline: Deadline,
        minimum: Option<tokio::time::Instant>,
        counts_sleep: bool,
        read: impl FnOnce() -> Option<Reading>,
    ) -> Self {
        Self::from_deadline_with(deadline, minimum, counts_sleep, read)
    }

    /// Capture from an injected clock without changing the process's clock state.
    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub fn capture_with_clock(
        deadline: tokio::time::Instant,
        counts_sleep: bool,
        read: impl FnOnce() -> Option<Reading>,
    ) -> Self {
        Self::capture_with(deadline, counts_sleep, read)
    }

    /// Wait from the captured baseline; the caller retains cancellation ownership.
    pub async fn wait(self) -> bool {
        self.wait_with(POLL, read).await
    }

    /// Report an effective deadline cut while the wait still owns its continuation.
    pub async fn wait_observed(self, changed: impl FnMut(tokio::time::Instant)) -> bool {
        self.wait_observed_with(POLL, read, changed).await
    }

    async fn wait_with(self, poll: Duration, read: impl FnMut() -> Option<Reading>) -> bool {
        self.wait_observed_with(poll, read, |_| {}).await
    }

    async fn wait_observed_with(
        self,
        poll: Duration,
        read: impl FnMut() -> Option<Reading>,
        changed: impl FnMut(tokio::time::Instant),
    ) -> bool {
        if self.counts_sleep {
            sleep_tracks_until(self.deadline, self.minimum, poll, read, changed).await
        } else {
            tokio::time::sleep_until(self.deadline()).await;
            false
        }
    }

    /// Wait against an injected clock, retaining the synchronously captured origin.
    #[cfg(any(test, debug_assertions))]
    pub async fn wait_with_clock(
        self,
        poll: Duration,
        read: impl FnMut() -> Option<Reading>,
    ) -> bool {
        self.wait_with(poll, read).await
    }

    /// Observe cuts from an injected clock while retaining exact wait ownership.
    #[cfg(any(test, debug_assertions))]
    pub async fn wait_with_clock_observed(
        self,
        poll: Duration,
        read: impl FnMut() -> Option<Reading>,
        changed: impl FnMut(tokio::time::Instant),
    ) -> bool {
        self.wait_observed_with(poll, read, changed).await
    }
}

/// [`sleep_until`] reading cumulative sleep every `poll` (the tests' seam), with
/// `armed` captured at selection. Another reader may observe multiple wakes between
/// this waiter's polls: their full elapsed sleep counts once. A transient initial
/// failure keeps polling; its first valid reading establishes the baseline and
/// cannot credit sleep whose origin is unknown. The deadline and the wakes' instants
/// are on tokio's clock, which is the `Instant` clock outside tests.
pub async fn sleep_until_with(
    deadline: tokio::time::Instant,
    poll: Duration,
    armed: Option<Reading>,
    read: impl FnMut() -> Option<Reading>,
) -> bool {
    sleep_tracks_until(
        WaitDeadline {
            at: deadline,
            asleep: armed.map(|reading| reading.asleep),
        },
        None,
        poll,
        read,
        |_| {},
    )
    .await
}

async fn sleep_tracks_until(
    mut deadline: WaitDeadline,
    mut minimum: Option<WaitDeadline>,
    poll: Duration,
    mut read: impl FnMut() -> Option<Reading>,
    mut changed: impl FnMut(tokio::time::Instant),
) -> bool {
    let mut cut = false;
    loop {
        let now = tokio::time::Instant::now();
        let at = minimum.map_or(deadline.at, |minimum| deadline.at.max(minimum.at));
        if now >= at {
            return cut;
        }
        tokio::time::sleep_until(at.min(now + poll)).await;
        let Some(reading) = read() else {
            continue;
        };
        let now = tokio::time::Instant::now();
        deadline.observe(reading, now);
        if let Some(minimum) = minimum.as_mut() {
            minimum.observe(reading, now);
        }
        let after = minimum.map_or(deadline.at, |minimum| deadline.at.max(minimum.at));
        if after < at {
            cut = true;
            changed(after);
        }
    }
}

/// A TIMEOUT THE HOST'S SLEEP COUNTS TOWARD (a Responses stream's idle timeout; review of
/// 2026-10-24): `fut`'s output, or tokio's `Elapsed` once `duration` of real time has passed
/// with it pending ([`settled_deadline`]). A stream the lid closed on mid-turn, idle across a
/// sleep longer than what was left of its timeout (upstream's 300 s), is given up once the host
/// has stayed awake [`WAKE_SETTLE`] after the wake, and its turn retries; on the awake clock
/// alone it waited out the rest of its 300 s while the person pressed Esc. The settle, not
/// [`AFTER_WAKE`], because giving a stream up starts a new request: in a dark wake (seconds,
/// the lid still closed) that request would be cut by the next sleep and sent again at every
/// dark wake through the night, spending the usage the person's limits count. A shorter sleep
/// only shortens the timeout by the time slept, so a stream the server still holds is not
/// discarded for a nap. Where ALab is not active (upstream's own tests) or the platform counts
/// no sleep, tokio's `timeout`.
///
/// # Errors
/// `Elapsed` once the time has run out, as tokio's `timeout` returns it.
pub async fn timeout<F: Future>(
    duration: Duration,
    fut: F,
) -> Result<F::Output, tokio::time::error::Elapsed> {
    if !crate::active() || !supported() {
        return tokio::time::timeout(duration, fut).await;
    }
    timeout_with(duration, POLL, read, fut).await
}

/// [`timeout`] reading the count from `read` every `poll` (the tests' seam; [`read`] outside
/// them), first as it is armed; a reading that fails (`None`, a rejected clock sample) is
/// skipped. A `duration` too long for the clock is tokio's `timeout`.
///
/// # Errors
/// `Elapsed` once the time has run out.
pub async fn timeout_with<F: Future>(
    duration: Duration,
    poll: Duration,
    mut read: impl FnMut() -> Option<Reading>,
    fut: F,
) -> Result<F::Output, tokio::time::error::Elapsed> {
    let Some(deadline) = tokio::time::Instant::now().checked_add(duration) else {
        return tokio::time::timeout(duration, fut).await;
    };
    let armed_at = tokio::time::Instant::now().into_std();
    let armed = read();
    let mut fut = std::pin::pin!(fut);
    let mut wait = std::pin::pin!(settle_until(deadline, poll, armed_at, armed, read));
    let ready = std::future::poll_fn(|cx| {
        // The future first, as tokio's own timeout polls it: what is ready is never lost.
        if let std::task::Poll::Ready(out) = fut.as_mut().poll(cx) {
            return std::task::Poll::Ready(Some(out));
        }
        wait.as_mut().poll(cx).map(|()| None)
    })
    .await;
    match ready {
        Some(out) => Ok(out),
        // Out of time: one last poll of the future, then tokio's own `Elapsed`.
        None => tokio::time::timeout(Duration::ZERO, fut).await,
    }
}

/// THE DEADLINE OF A TIMEOUT THE SLEEP COUNTS TOWARD: `deadline`, armed on the `Instant` clock,
/// less the time the host has slept since (`slept`), but never sooner than [`WAKE_SETTLE`]
/// after the host's last wake (`woke`) and never later than it was.
#[must_use]
pub fn settled_deadline(deadline: Instant, slept: Duration, woke: Instant) -> Instant {
    let floor = woke.checked_add(WAKE_SETTLE).unwrap_or(woke);
    deadline
        .checked_sub(slept)
        .unwrap_or(woke)
        .max(floor)
        .min(deadline)
}

/// Until `deadline` ([`settled_deadline`]): each reading that finds a wake newer than the one
/// `armed` knew, and dated after the arming at `armed_at` (an arming whose own reading failed
/// knows no wake, and a wake before it is history), recomputes it from the armed deadline, all
/// the time slept since the arming (the total asleep then and now, whoever else read meanwhile;
/// with no armed total, the wake's own sleep) and that wake, so a later wake settles again: a
/// second dark wake does not inherit the first one's settle.
async fn settle_until(
    deadline: tokio::time::Instant,
    poll: Duration,
    armed_at: Instant,
    armed: Option<Reading>,
    mut read: impl FnMut() -> Option<Reading>,
) {
    let base = armed.map(|reading| reading.asleep);
    let seen = armed.and_then(|reading| reading.last).map(|wake| wake.seq);
    let mut due = deadline;
    loop {
        let now = tokio::time::Instant::now();
        if now >= due {
            return;
        }
        tokio::time::sleep_until(due.min(now + poll)).await;
        let Some(reading) = read() else {
            continue;
        };
        let Some(wake) = reading
            .last
            .filter(|wake| Some(wake.seq) != seen && wake.at >= armed_at)
        else {
            continue;
        };
        let slept = base.map_or(wake.slept, |base| reading.asleep.saturating_sub(base));
        due = tokio::time::Instant::from_std(settled_deadline(deadline.into_std(), slept, wake.at));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::cell::Cell;
    use std::sync::Arc;

    const SEC: Duration = Duration::from_secs(1);

    fn accepted(state: &WakeState) -> Reading {
        Reading {
            asleep: state
                .asleep
                .unwrap_or_else(|| panic!("the scripted state has a baseline")),
            last: state.last(),
        }
    }

    fn counted(asleep: Duration) -> Reading {
        Reading { asleep, last: None }
    }

    async fn pending_once<F: std::future::Future>(mut future: std::pin::Pin<&mut F>) {
        std::future::poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
    }

    #[test]
    fn a_handoff_seeds_only_a_missing_deadline_baseline() {
        let at = Instant::now() + 60 * SEC;
        let receipt = Deadline::with_reading(at, Some(counted(5000 * SEC)));
        assert_eq!(receipt.with_baseline(Some(counted(5100 * SEC))), receipt);
        let untracked = Deadline::untracked(at);
        assert_eq!(untracked.with_baseline(None), untracked);
        let fresh = untracked.with_baseline(Some(counted(5100 * SEC)));
        assert_eq!(fresh.at_reading(Some(counted(5100 * SEC)), at), at);
        assert_eq!(
            fresh.at_reading(Some(counted(5110 * SEC)), at),
            at - 10 * SEC
        );
        assert_eq!(untracked.at_reading(Some(counted(5110 * SEC)), at), at);
    }

    #[tokio::test(start_paused = true)]
    async fn a_measured_cut_republishes_pacing_before_the_wait_can_finish() {
        let start = tokio::time::Instant::now();
        let receipt =
            Deadline::with_reading((start + 60 * SEC).into_std(), Some(counted(5000 * SEC)));
        let wait = ArmedWait::from_deadline_with(receipt, Some(start + 10 * SEC), true, || None);
        let published = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&published);
        let observer = crate::capacity::InFlightRetryObserver::new(move |deadline| {
            captured
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(deadline);
        });
        let guard = observer.waiting(wait.deadline().into_std());
        let latest = || {
            published
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .last()
                .copied()
                .flatten()
        };
        assert_eq!(latest(), Some((start + 60 * SEC).into_std()));
        {
            let waiting = wait.wait_with_clock_observed(
                POLL,
                || {
                    Some(Reading {
                        asleep: 5120 * SEC,
                        last: Some(Wake {
                            at: (start + POLL).into_std(),
                            slept: 120 * SEC,
                            seq: 1,
                        }),
                    })
                },
                |deadline| {
                    assert!(
                        guard.update(deadline.into_std()),
                        "the cut still owns its pacing publication"
                    );
                },
            );
            tokio::pin!(waiting);
            pending_once(waiting.as_mut()).await;
            tokio::time::advance(POLL).await;
            pending_once(waiting.as_mut()).await;
            assert_eq!(
                latest(),
                Some((start + 10 * SEC).into_std()),
                "a crash during the narrowed wait must not carry its old60s pacing"
            );
            tokio::time::advance(POLL).await;
            pending_once(waiting.as_mut()).await;
            assert_eq!(
                published
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .len(),
                2,
                "a repeated reading neither recredits sleep nor republishes the same deadline"
            );
            assert!(waiting.await);
        }
        assert_eq!(tokio::time::Instant::now(), start + 10 * SEC);
        drop(guard);
        assert_eq!(
            latest(),
            None,
            "ending the exact wait clears its publication"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn retained_advice_normalizes_and_arms_from_one_selection_read() {
        let start = tokio::time::Instant::now();
        let receipt =
            Deadline::with_reading((start + 60 * SEC).into_std(), Some(counted(5000 * SEC)));
        let reads = Cell::new(0);
        let wait = ArmedWait::from_deadline_with(receipt, None, true, || {
            reads.set(reads.get() + 1);
            Some(counted(5010 * SEC))
        });
        assert_eq!(
            reads.get(),
            1,
            "normalization and capture share one accepted count"
        );
        assert_eq!(wait.deadline(), start + 50 * SEC);
        assert!(!wait.wait_with(POLL, || Some(counted(5010 * SEC))).await);
        assert_eq!(tokio::time::Instant::now(), start + 50 * SEC);
        assert_eq!(
            receipt.at_reading(Some(counted(5010 * SEC)), start.into_std()),
            (start + 50 * SEC).into_std(),
            "the receipt and its copies retain their own original baseline"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_rejected_selection_keeps_the_known_provider_receipt_origin() {
        let start = tokio::time::Instant::now();
        let receipt =
            Deadline::with_reading((start + 60 * SEC).into_std(), Some(counted(5000 * SEC)));
        let wait = ArmedWait::from_deadline_with(receipt, None, true, || None);
        assert!(
            wait.wait_with(POLL, || Some(Reading {
                asleep: 5120 * SEC,
                last: Some(Wake {
                    at: (start + POLL).into_std(),
                    slept: 120 * SEC,
                    seq: 1
                }),
            }))
            .await
        );
        assert_eq!(tokio::time::Instant::now(), start + POLL + AFTER_WAKE);
    }

    #[tokio::test(start_paused = true)]
    async fn provider_sleep_before_selection_cannot_consume_a_fresh_local_minimum() {
        let start = tokio::time::Instant::now();
        let receipt =
            Deadline::with_reading((start + 60 * SEC).into_std(), Some(counted(5000 * SEC)));
        let wait = ArmedWait::from_deadline_with(receipt, Some(start + 10 * SEC), true, || {
            Some(counted(5120 * SEC))
        });
        assert_eq!(wait.deadline(), start + 10 * SEC);
        assert!(!wait.wait_with(POLL, || Some(counted(5120 * SEC))).await);
        assert_eq!(tokio::time::Instant::now(), start + 10 * SEC);
    }

    #[tokio::test(start_paused = true)]
    async fn a_rejected_selection_preserves_the_unknown_local_minimum_origin() {
        let start = tokio::time::Instant::now();
        let receipt =
            Deadline::with_reading((start + 60 * SEC).into_std(), Some(counted(5000 * SEC)));
        let wait = ArmedWait::from_deadline_with(receipt, Some(start + 10 * SEC), true, || None);
        assert!(
            wait.wait_with(POLL, || Some(Reading {
                asleep: 5120 * SEC,
                last: Some(Wake {
                    at: (start + POLL).into_std(),
                    slept: 120 * SEC,
                    seq: 1
                }),
            }))
            .await
        );
        assert_eq!(
            tokio::time::Instant::now(),
            start + 10 * SEC,
            "known provider sleep does not shorten a newly selected minimum with unknown origin"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn independent_wait_origins_count_later_and_missed_wakes_once() {
        let start = tokio::time::Instant::now();
        let receipt =
            Deadline::with_reading((start + 60 * SEC).into_std(), Some(counted(5000 * SEC)));
        let wait = ArmedWait::from_deadline_with(receipt, Some(start + 10 * SEC), true, || None);
        assert!(
            wait.wait_with(POLL, || {
                let later = tokio::time::Instant::now() >= start + 5 * SEC;
                Some(Reading {
                    asleep: if later { 5600 * SEC } else { 5120 * SEC },
                    last: Some(Wake {
                        at: (start + if later { 5 * SEC } else { POLL }).into_std(),
                        slept: 120 * SEC,
                        seq: if later { 3 } else { 1 },
                    }),
                })
            })
            .await
        );
        assert_eq!(
            tokio::time::Instant::now(),
            start + 5 * SEC + AFTER_WAKE,
            "both later sleeps count from the fresh local baseline; repeated observations do not move it again"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_unknown_receipt_and_selection_credit_only_sleep_after_first_valid_count() {
        for later_sleep in [false, true] {
            let start = tokio::time::Instant::now();
            let receipt = Deadline::untracked((start + 60 * SEC).into_std());
            let wait = ArmedWait::from_deadline_with(receipt, None, true, || None);
            let cut = wait
                .wait_with(POLL, || {
                    let later = later_sleep && tokio::time::Instant::now() >= start + 10 * SEC;
                    Some(Reading {
                        asleep: if later { 5240 * SEC } else { 5120 * SEC },
                        last: Some(Wake {
                            at: (start + if later { 10 * SEC } else { POLL }).into_std(),
                            slept: 120 * SEC,
                            seq: if later { 2 } else { 1 },
                        }),
                    })
                })
                .await;
            assert_eq!(cut, later_sleep);
            assert_eq!(
                tokio::time::Instant::now(),
                start
                    + if later_sleep {
                        10 * SEC + AFTER_WAKE
                    } else {
                        60 * SEC
                    }
            );
        }
    }

    #[test]
    fn a_reading_finds_a_wake_only_where_the_time_asleep_grew_by_a_second() {
        let t0 = Instant::now();
        let mut state = WakeState::default();
        // The first reading is the baseline, whatever the host slept before it.
        assert_eq!(state.observe(t0, 3600 * SEC), None);
        // The two clocks read a moment apart: no sleep.
        assert_eq!(state.observe(t0 + SEC, 3600 * SEC + SEC / 2), None);
        let wake = state.observe(t0 + 2 * SEC, 3600 * SEC + SEC / 2 + 19 * SEC);
        assert_eq!(
            wake,
            Some(Wake {
                at: t0 + 2 * SEC,
                slept: 19 * SEC,
                seq: 1,
            })
        );
        // Later readings with no sleep keep the last wake.
        assert_eq!(state.observe(t0 + 3 * SEC, 3619 * SEC + SEC / 2), wake);
        let second = state.observe(t0 + 4 * SEC, 3619 * SEC + SEC / 2 + 1500 * SEC);
        assert_eq!(second.map(|w| (w.seq, w.slept)), Some((2, 1500 * SEC)));
        assert_eq!(state.last(), second);
    }

    #[test]
    fn a_wait_counts_the_sleep_but_never_ends_sooner_than_the_networks_own_wake() {
        let t0 = Instant::now();
        let deadline = t0 + 60 * SEC;
        // A long sleep: the wait ends AFTER_WAKE past the wake.
        let long = Wake {
            at: t0 + 10 * SEC,
            slept: 2 * 3600 * SEC,
            seq: 1,
        };
        assert_eq!(deadline_after(deadline, &long), t0 + 10 * SEC + AFTER_WAKE);
        // A short sleep only shortens it by the time slept.
        let short = Wake {
            at: t0 + 10 * SEC,
            slept: 5 * SEC,
            seq: 1,
        };
        assert_eq!(deadline_after(deadline, &short), t0 + 55 * SEC);
        // A wake right before the deadline never moves it later.
        let late = Wake {
            at: deadline - SEC / 2,
            slept: 30 * SEC,
            seq: 1,
        };
        assert_eq!(deadline_after(deadline, &late), deadline);
    }

    /// A BACKOFF A SLEEP INTERRUPTED ENDS SOON AFTER THE WAKE: a 60 s wait, a wake 10 s in that
    /// slept two hours, ends at the wake plus AFTER_WAKE and says a wake cut it. POSITIVE
    /// CONTROL: the same wait with no wake runs its whole 60 s and says none did.
    #[tokio::test(start_paused = true)]
    async fn a_backoff_a_sleep_interrupted_ends_soon_after_the_wake() {
        let start = tokio::time::Instant::now();
        let deadline = start + 60 * SEC;
        let woke_at = start + 10 * SEC;
        let readings = Arc::new(Mutex::new(Vec::<Instant>::new()));
        let seen = Arc::clone(&readings);
        let armed = counted(5000 * SEC);
        let cut = sleep_until_with(deadline, POLL, Some(armed), move || {
            let now = tokio::time::Instant::now();
            seen.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(now.into_std());
            Some(if now >= woke_at {
                Reading {
                    asleep: armed.asleep + 2 * 3600 * SEC,
                    last: Some(Wake {
                        at: woke_at.into_std(),
                        slept: 2 * 3600 * SEC,
                        seq: 7,
                    }),
                }
            } else {
                armed
            })
        })
        .await;
        assert!(cut);
        assert_eq!(tokio::time::Instant::now(), woke_at + AFTER_WAKE);
        assert!(
            readings
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len()
                >= 10,
            "read every POLL"
        );

        // POSITIVE CONTROL: no wake, the whole wait.
        let start = tokio::time::Instant::now();
        let cut = sleep_until_with(start + 60 * SEC, POLL, Some(armed), || Some(armed)).await;
        assert!(!cut);
        assert_eq!(tokio::time::Instant::now(), start + 60 * SEC);
    }

    /// A WAKE FOUND BEFORE THE WAIT BEGAN IS NOT ONE THAT INTERRUPTED IT: a wait that starts with
    /// an old wake on the books runs its whole time; one found during it cuts it short.
    #[tokio::test(start_paused = true)]
    async fn only_a_wake_found_during_the_wait_cuts_it_short() {
        let start = tokio::time::Instant::now();
        let old = Wake {
            at: start.into_std(),
            slept: 3600 * SEC,
            seq: 3,
        };
        let armed = Reading {
            asleep: 5000 * SEC,
            last: Some(old),
        };
        let cut = sleep_until_with(start + 20 * SEC, POLL, Some(armed), || Some(armed)).await;
        assert!(!cut);
        assert_eq!(tokio::time::Instant::now(), start + 20 * SEC);
    }

    /// A delayed pre-sleep sample must not make one ten-second sleep count twice.
    /// Drive the actual wait with the ordering possible when a reader samples the
    /// clocks before another reader records the wake, but publishes afterward.
    #[tokio::test(start_paused = true)]
    async fn a_stale_sample_cannot_credit_one_sleep_twice() {
        let start = tokio::time::Instant::now();
        let base = 5000 * SEC;
        let mut state = WakeState::default();
        state.observe(start.into_std(), base);
        let armed = accepted(&state);
        let cut = sleep_until_with(start + 100 * SEC, POLL, Some(armed), || {
            let now = tokio::time::Instant::now();
            let asleep = if now < start + 10 * SEC || now == start + 11 * SEC {
                base
            } else {
                base + 10 * SEC
            };
            state.observe(now.into_std(), asleep);
            Some(accepted(&state))
        })
        .await;
        assert!(cut);
        assert_eq!(
            tokio::time::Instant::now(),
            start + 90 * SEC,
            "one ten-second sleep leaves ninety awake seconds of the advised wait"
        );
        assert_eq!(state.last().map(|wake| wake.seq), Some(1));
    }

    /// Rejecting a stale sample must not suppress a genuinely new sleep.
    #[tokio::test(start_paused = true)]
    async fn two_real_sleeps_each_count_toward_the_wait() {
        let start = tokio::time::Instant::now();
        let base = 5000 * SEC;
        let mut state = WakeState::default();
        state.observe(start.into_std(), base);
        let armed = accepted(&state);
        let cut = sleep_until_with(start + 100 * SEC, POLL, Some(armed), || {
            let now = tokio::time::Instant::now();
            let asleep = if now < start + 10 * SEC {
                base
            } else if now < start + 12 * SEC {
                base + 10 * SEC
            } else {
                base + 20 * SEC
            };
            state.observe(now.into_std(), asleep);
            Some(accepted(&state))
        })
        .await;
        assert!(cut);
        assert_eq!(
            tokio::time::Instant::now(),
            start + 80 * SEC,
            "two real ten-second sleeps leave eighty awake seconds"
        );
        assert_eq!(state.last().map(|wake| wake.seq), Some(2));
    }

    /// Other readers can record two wakes before this waiter runs again. Both
    /// sleeps still count, even though its next reading exposes only the last wake.
    #[tokio::test(start_paused = true)]
    async fn a_wait_counts_sleeps_another_reader_observed_between_its_polls() {
        let start = tokio::time::Instant::now();
        let base = 5000 * SEC;
        let mut state = WakeState::default();
        state.observe(start.into_std(), base);
        let armed = accepted(&state);
        let mut other_readers_ran = false;
        let cut = sleep_until_with(start + 100 * SEC, POLL, Some(armed), || {
            if tokio::time::Instant::now() >= start + 10 * SEC && !other_readers_ran {
                // This waiter missed the first wake while other readers ran.
                state.observe((start + 8 * SEC).into_std(), base + 10 * SEC);
                state.observe((start + 10 * SEC).into_std(), base + 20 * SEC);
                other_readers_ran = true;
            }
            Some(accepted(&state))
        })
        .await;
        assert!(cut);
        assert_eq!(state.last().map(|wake| wake.seq), Some(2));
        assert_eq!(
            tokio::time::Instant::now(),
            start + 80 * SEC,
            "both ten-second sleeps leave eighty awake seconds of the advised wait"
        );
    }

    #[test]
    fn a_captured_deadline_counts_elapsed_sleep_without_compounding_it() {
        let now = Instant::now();
        let at = now + 100 * SEC;
        let base = 5000 * SEC;
        let deadline = Deadline::with_reading(at, Some(counted(base)));
        assert_eq!(deadline.at_reading(None, now), at);
        assert_eq!(deadline.at_reading(Some(counted(base - SEC)), now), at);
        assert_eq!(deadline.at_reading(Some(counted(base + SEC / 2)), now), at);
        let first = Some(counted(base + 10 * SEC));
        assert_eq!(deadline.at_reading(first, now), at - 10 * SEC);
        assert_eq!(deadline.at_reading(first, now), at - 10 * SEC);
        assert_eq!(
            deadline.at_reading(Some(counted(base + 20 * SEC)), now),
            at - 20 * SEC
        );
    }

    #[test]
    fn an_untracked_deadline_keeps_its_original_instant() {
        let now = Instant::now();
        let at = now + 100 * SEC;
        let deadline = Deadline::untracked(at);
        assert_eq!(deadline.current(), at);
        assert_eq!(deadline.at_reading(Some(counted(Duration::MAX)), now), at);
    }

    #[test]
    fn sleep_that_underflows_a_deadline_leaves_it_already_due() {
        let now = Instant::now();
        for at in [now - SEC, now + 100 * SEC] {
            let deadline = Deadline::with_reading(at, Some(counted(Duration::ZERO)));
            assert_eq!(
                deadline.at_reading(Some(counted(Duration::MAX)), now),
                now.min(at)
            );
        }
    }

    /// Reporting a selected wait can yield before the waiting future is polled.
    /// Its captured baseline includes that interval; an unmeasurable host keeps
    /// the original tokio deadline.
    #[tokio::test(start_paused = true)]
    async fn a_selected_wait_counts_sleep_before_its_first_poll() {
        for tracked in [true, false] {
            let start = tokio::time::Instant::now();
            let base = 5000 * SEC;
            let wait = ArmedWait::capture_with(start + 100 * SEC, tracked, || Some(counted(base)));
            tokio::time::advance(10 * SEC).await;
            let current = Reading {
                asleep: base + 20 * SEC,
                last: Some(Wake {
                    at: tokio::time::Instant::now().into_std(),
                    slept: 20 * SEC,
                    seq: 1,
                }),
            };
            let reads = Cell::new(0);
            let cut = wait
                .wait_with(POLL, || {
                    reads.set(reads.get() + 1);
                    Some(current)
                })
                .await;
            assert_eq!(cut, tracked);
            assert_eq!(reads.get() > 0, tracked);
            assert_eq!(
                tokio::time::Instant::now(),
                start + if tracked { 80 * SEC } else { 100 * SEC }
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_wait_can_be_cancelled_without_leaving_a_poller() {
        let start = tokio::time::Instant::now();
        let reads = Cell::new(0);
        let armed = counted(5000 * SEC);
        {
            let wait = sleep_until_with(start + 100 * SEC, POLL, Some(armed), || {
                reads.set(reads.get() + 1);
                Some(armed)
            });
            tokio::pin!(wait);
            tokio::select! {
                _ = tokio::time::sleep_until(start + 3 * SEC) => {}
                _ = &mut wait => panic!("the selected wait is still pending"),
            }
        }
        let stopped = reads.get();
        assert!(stopped > 0, "the wait had started polling");
        tokio::time::advance(200 * SEC).await;
        assert_eq!(reads.get(), stopped, "dropping the wait stops its polling");
    }

    /// The actual publication seam samples under its observation lock and never
    /// exposes a rejected stale total to an outer continuation's sleep mark.
    #[test]
    fn a_read_publishes_only_the_accepted_sleep_total() {
        let state = Mutex::new(WakeState::default());
        let base = 5000 * SEC;
        let sample = |asleep| {
            read_with(
                &state,
                || {
                    assert!(matches!(
                        state.try_lock(),
                        Err(std::sync::TryLockError::WouldBlock)
                    ));
                    Some(asleep)
                },
                || None,
            )
            .unwrap_or_else(|| panic!("the scripted sleep total must be readable"))
        };
        let initial = sample(base);
        assert_eq!(initial.asleep, base);
        assert_eq!(initial.last, None);
        let first = sample(base + 10 * SEC);
        assert_eq!(first.last.map(|wake| wake.seq), Some(1));
        assert_eq!(sample(base), first, "do not publish a stale lower total");
        assert_eq!(sample(base + 10 * SEC), first, "do not count it twice");
        let second = sample(base + 20 * SEC);
        assert_eq!(second.asleep, base + 20 * SEC);
        assert_eq!(
            second.last.map(|wake| (wake.seq, wake.slept)),
            Some((2, 10 * SEC))
        );
    }

    /// A WAKE IS DATED WHEN THE HOST WOKE, NOT WHEN A READING NOTICED IT (review of 2026-10-24:
    /// nothing reads while a turn streams or the session stands idle, so the first reading after
    /// a sleep can come many minutes after the wake, and a wake dated then held every act 30 s
    /// and more, saying "the host just woke", with the host long awake). The kernel's record of
    /// the wake dates it; a record from before the reading before (the host was awake then, so
    /// it is not this wake) and no record at all date it at the reading, the latest it can be.
    #[test]
    fn a_wake_is_dated_by_the_kernels_record_of_it() {
        let t0 = Instant::now();
        let base = 5000 * SEC;
        let mut state = WakeState::default();
        assert_eq!(state.observe_woke(t0, base, || Some(SEC)), None);
        // The host slept an hour after the reading at t0, woke, and stayed awake 10 min with
        // nothing reading: the reading at t0 + 12 min (awake clock) finds the wake 10 min old.
        let now = t0 + 12 * 60 * SEC;
        let wake = state.observe_woke(now, base + 3600 * SEC, || Some(10 * 60 * SEC));
        assert_eq!(
            wake,
            Some(Wake {
                at: now - 10 * 60 * SEC,
                slept: 3600 * SEC,
                seq: 1,
            })
        );
        // The kernel is asked only for a reading that finds a sleep.
        let later = now + 60 * SEC;
        assert_eq!(
            state.observe_woke(later, base + 3600 * SEC, || panic!("asked with no sleep")),
            wake
        );
        // A record from before the reading before is not this wake: dated at the reading.
        let next = later + 60 * SEC;
        let stale = state.observe_woke(next, base + 7200 * SEC, || Some(10 * 60 * SEC));
        assert_eq!(stale.map(|w| (w.at, w.seq)), Some((next, 2)));
        // No record (Linux): dated at the reading.
        let last = next + 60 * SEC;
        let none = state.observe_woke(last, base + 7300 * SEC, || None);
        assert_eq!(none.map(|w| (w.at, w.seq)), Some((last, 3)));
        // `observe` is the same reading with no record.
        let mut plain = WakeState::default();
        plain.observe(t0, base);
        assert_eq!(
            plain.observe(now, base + 3600 * SEC).map(|w| w.at),
            Some(now)
        );
    }

    /// The publication seam asks the kernel for the wake's date under the same lock, only when
    /// the reading finds a sleep, and publishes the wake so dated.
    #[test]
    fn a_read_dates_its_wake_by_the_kernels_record() {
        const AWAKE: Duration = Duration::from_millis(30);
        let state = Mutex::new(WakeState::default());
        let base = 5000 * SEC;
        let asked = std::cell::Cell::new(0_u32);
        let read = |asleep: Duration| {
            read_with(
                &state,
                || Some(asleep),
                || {
                    asked.set(asked.get() + 1);
                    assert!(matches!(
                        state.try_lock(),
                        Err(std::sync::TryLockError::WouldBlock)
                    ));
                    Some(AWAKE)
                },
            )
            .unwrap_or_else(|| panic!("the scripted sleep total must be readable"))
        };
        assert_eq!(read(base).last, None);
        std::thread::sleep(Duration::from_millis(80));
        let found = read(base + 3600 * SEC);
        let after = Instant::now();
        assert_eq!(
            asked.get(),
            1,
            "asked once, for the reading that found the sleep"
        );
        // Woke 30 ms before the reading that found it, 50 ms after the reading before.
        let at = found.last.map(|w| w.at);
        assert!(
            at.is_some_and(|at| at <= after - AWAKE),
            "{at:?} is not the kernel's date"
        );
        assert_eq!(read(base + 3600 * SEC).last, found.last);
        assert_eq!(asked.get(), 1);
    }

    /// A host whose readings are scripted: `sleeps` are `(awake instant, time slept)`, the
    /// wake each found dated at that instant; a reading sees every sleep up to now.
    fn scripted(
        base: Duration,
        sleeps: Vec<(tokio::time::Instant, Duration)>,
    ) -> impl FnMut() -> Option<Reading> {
        move || {
            let now = tokio::time::Instant::now();
            let found: Vec<_> = sleeps.iter().filter(|(at, _)| *at <= now).collect();
            let asleep = base + found.iter().map(|(_, slept)| *slept).sum::<Duration>();
            let last = found.last().map(|(at, slept)| Wake {
                at: at.into_std(),
                slept: *slept,
                seq: found.len() as u64,
            });
            Some(Reading { asleep, last })
        }
    }

    /// A STREAM IDLE ACROSS A SLEEP LONGER THAN ITS TIMEOUT IS GIVEN UP ONCE THE HOST HAS
    /// SETTLED: a 300 s idle timeout and a wake 10 s in that slept 20 min elapse at the wake
    /// plus WAKE_SETTLE, not 290 awake seconds later. A short sleep (60 s) only shortens it by
    /// the sleep. POSITIVE CONTROL: no wake, the whole 300 s; a future that is ready wins.
    #[tokio::test(start_paused = true)]
    async fn an_idle_timeout_counts_the_sleep() {
        let pending = std::future::pending::<()>;
        let base = 5000 * SEC;
        let start = tokio::time::Instant::now();
        let woke_at = start + 10 * SEC;
        let host = scripted(base, vec![(woke_at, 20 * 60 * SEC)]);
        let out = timeout_with(300 * SEC, POLL, host, pending()).await;
        assert!(out.is_err(), "the idle stream elapses");
        assert_eq!(tokio::time::Instant::now(), woke_at + WAKE_SETTLE);

        let start = tokio::time::Instant::now();
        let host = scripted(base, vec![(start + 10 * SEC, 60 * SEC)]);
        let out = timeout_with(300 * SEC, POLL, host, pending()).await;
        assert!(out.is_err());
        assert_eq!(tokio::time::Instant::now(), start + 240 * SEC);

        // POSITIVE CONTROL: no wake, the whole timeout.
        let start = tokio::time::Instant::now();
        let out = timeout_with(300 * SEC, POLL, scripted(base, vec![]), pending()).await;
        assert!(out.is_err());
        assert_eq!(tokio::time::Instant::now(), start + 300 * SEC);

        // A future ready before its time is its output: an event 5 s in, before a wake at 10 s.
        let start = tokio::time::Instant::now();
        let host = scripted(base, vec![(start + 10 * SEC, 20 * 60 * SEC)]);
        let ready = async {
            tokio::time::sleep(5 * SEC).await;
            "event"
        };
        let out = timeout_with(300 * SEC, POLL, host, ready).await;
        assert_eq!(out.ok(), Some("event"));
        assert_eq!(tokio::time::Instant::now(), start + 5 * SEC);
    }

    /// NO STREAM IS GIVEN UP IN A DARK WAKE: a wake 10 s in after 20 min asleep, a 19 s dark
    /// wake, then 30 min asleep again and a wake at 29 s. The first wake's settle (40 s) falls
    /// 11 s into the second wake, which settles again: the stream is given up at 59 s, the
    /// second wake plus WAKE_SETTLE. A sleep found before the timeout was armed counts nothing.
    #[tokio::test(start_paused = true)]
    async fn a_dark_wake_gives_no_stream_up_and_the_next_wake_settles_again() {
        let pending = std::future::pending::<()>;
        let base = 5000 * SEC;
        let start = tokio::time::Instant::now();
        let first = start + 10 * SEC;
        let second = start + 29 * SEC;
        let host = scripted(base, vec![(first, 20 * 60 * SEC), (second, 30 * 60 * SEC)]);
        let out = timeout_with(300 * SEC, POLL, host, pending()).await;
        assert!(out.is_err());
        assert_eq!(tokio::time::Instant::now(), second + WAKE_SETTLE);

        // An hour asleep before the arming is history: the whole timeout.
        let start = tokio::time::Instant::now();
        let host = scripted(base, vec![(start, 3600 * SEC)]);
        let out = timeout_with(300 * SEC, POLL, host, pending()).await;
        assert!(out.is_err());
        assert_eq!(tokio::time::Instant::now(), start + 300 * SEC);
    }

    /// AN ARMING WHOSE OWN READING FAILED (a rejected clock sample) KNOWS NO WAKE: a wake from
    /// before the arming that a later reading reports is history and leaves the whole timeout;
    /// one after it still gives the stream up once the host has settled.
    #[tokio::test(start_paused = true)]
    async fn a_failed_arming_reading_credits_no_historical_wake() {
        let pending = std::future::pending::<()>;
        let base = 5000 * SEC;
        for (historical, elapsed) in [(true, 300 * SEC), (false, 40 * SEC)] {
            let start = tokio::time::Instant::now();
            let woke_at = if historical {
                start - 10 * SEC
            } else {
                start + 10 * SEC
            };
            let mut host = scripted(base, vec![(woke_at, 20 * 60 * SEC)]);
            let mut first = true;
            let read = move || {
                if std::mem::take(&mut first) {
                    return None;
                }
                host()
            };
            let out = timeout_with(300 * SEC, POLL, read, pending()).await;
            assert!(out.is_err());
            assert_eq!(
                tokio::time::Instant::now() - start,
                elapsed,
                "historical {historical}"
            );
        }
    }

    /// Where ALab is not active (every upstream test) the timeout is tokio's own, whatever the
    /// platform counts.
    #[tokio::test(start_paused = true)]
    async fn an_inactive_timeout_is_tokios() {
        assert!(!crate::active());
        let start = tokio::time::Instant::now();
        let out = timeout(30 * SEC, std::future::pending::<()>()).await;
        assert!(out.is_err());
        assert_eq!(tokio::time::Instant::now(), start + 30 * SEC);
        assert_eq!(timeout(30 * SEC, async { 7 }).await.ok(), Some(7));
    }

    /// The kernel's record of the last wake is on the awake clock: never more awake time than
    /// the host has been up.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_kernels_wake_record_is_on_the_awake_clock() {
        if let Some(awake) = awake_since_wake() {
            let up = clock_ns(libc::CLOCK_UPTIME_RAW).map(Duration::from_nanos);
            assert!(up.is_some_and(|up| awake <= up), "{awake:?} > {up:?}");
        }
        assert!(sysctl_u64(c"hw.tbfrequency").is_some_and(|hz| hz > 0));
        assert_eq!(sysctl_u64(c"no.such.sysctl"), None);
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
    fn sample_clock_gap(gap_ns: u64, asleep_ns: u64) -> Option<Duration> {
        let mut elapsed = 0;
        let mut calls = 0;
        sample_asleep_with(|clock| {
            if calls == 1 {
                elapsed = gap_ns;
            }
            calls += 1;
            let awake = 1_000_000_000_000 + elapsed;
            Some(match clock {
                SleepClock::Awake => awake,
                SleepClock::Through => awake + asleep_ns,
            })
        })
    }

    /// Awake descheduling between Linux's original reads is not host suspend.
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
    #[test]
    fn a_slow_linux_clock_pair_cannot_create_a_false_wake() {
        let state = Mutex::new(WakeState::default());
        let before = read_with(&state, || Some(5000 * SEC), || None)
            .unwrap_or_else(|| panic!("the initial sleep total must be readable"));
        assert_eq!(before.last, None);
        let delayed = read_with(
            &state,
            || sample_clock_gap(3_000_000_000, 5_000_000_000_000),
            || None,
        );
        let last = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .last();
        assert_eq!(last, None, "three awake seconds are not a sleep");
        assert_eq!(delayed, None, "reject the uncertain clock pair");
    }

    /// A slow first macOS pair must not seed a low baseline whose recovery looks like sleep.
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
    #[test]
    fn a_slow_first_clock_pair_cannot_seed_a_false_wake() {
        let state = Mutex::new(WakeState::default());
        let delayed = read_with(
            &state,
            || sample_clock_gap(3_000_000_000, 5_000_000_000_000),
            || None,
        );
        let fresh = read_with(&state, || sample_clock_gap(0, 5_000_000_000_000), || None)
            .unwrap_or_else(|| panic!("the fast pair must be readable"));
        assert_eq!(fresh.last, None, "baseline recovery is not host sleep");
        assert_eq!(delayed, None);
        assert_eq!(fresh.asleep, 5000 * SEC);
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
    #[test]
    fn fast_clock_pairs_still_measure_a_real_sleep() {
        let state = Mutex::new(WakeState::default());
        let before = read_with(&state, || sample_clock_gap(0, 5_000_000_000_000), || None)
            .unwrap_or_else(|| panic!("the initial fast pair must be readable"));
        assert_eq!(before.last, None);
        let after = read_with(&state, || sample_clock_gap(0, 5_010_000_000_000), || None)
            .unwrap_or_else(|| panic!("the next fast pair must be readable"));
        assert_eq!(after.asleep, 5010 * SEC);
        assert_eq!(
            after.last.map(|wake| (wake.seq, wake.slept)),
            Some((1, 10 * SEC))
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
    #[test]
    fn failed_backwards_and_slow_clock_pairs_are_rejected() {
        for (awake, through) in [
            ([None, Some(1_000_000_000_000)], Some(6_000_000_000_000)),
            ([Some(1_000_000_000_000), None], Some(6_000_000_000_000)),
            ([Some(1_000_000_000_000), Some(1_000_000_000_000)], None),
            (
                [Some(1_000_000_000_000), Some(999_999_999_999)],
                Some(6_000_000_000_000),
            ),
            (
                [Some(1_000_000_000_000), Some(1_000_001_000_000)],
                Some(6_000_000_000_000),
            ),
            (
                [Some(1_000_000_000_000), Some(1_003_000_000_000)],
                Some(6_000_000_000_000),
            ),
        ] {
            let mut awake = awake.into_iter();
            let result = sample_asleep_with(|clock| match clock {
                SleepClock::Awake => awake.next().flatten(),
                SleepClock::Through => through,
            });
            assert_eq!(result, None);
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
    #[test]
    fn a_fast_bracket_uses_the_conservative_awake_endpoint() {
        let mut awake = [1_000_000_000_000, 1_000_000_999_999].into_iter();
        let result = sample_asleep_with(|clock| match clock {
            SleepClock::Awake => awake.next(),
            SleepClock::Through => Some(6_000_000_000_000),
        });
        assert_eq!(result, Some(Duration::from_nanos(4_999_999_000_001)));
    }

    async fn core_wait_after_an_initial_unread_sample(historical: bool) -> (bool, Duration) {
        let start = tokio::time::Instant::now();
        let woke_at = if historical {
            start - 10 * SEC
        } else {
            start + 10 * SEC
        };
        let mut first = true;
        let first_valid = Cell::new(None);
        let mut read = || {
            if std::mem::take(&mut first) {
                return None;
            }
            let woke = historical || tokio::time::Instant::now() >= woke_at;
            let asleep = if woke { 5120 * SEC } else { 5000 * SEC };
            if first_valid.get().is_none() {
                first_valid.set(Some(asleep));
            }
            Some(Reading {
                asleep,
                last: woke.then_some(Wake {
                    at: woke_at.into_std(),
                    slept: 120 * SEC,
                    seq: 1,
                }),
            })
        };
        let wait = ArmedWait::capture_with(start + 60 * SEC, true, &mut read);
        let cut = wait.wait_with(POLL, read).await;
        assert_eq!(
            first_valid.get(),
            Some(if historical { 5120 * SEC } else { 5000 * SEC }),
            "the first valid sample establishes the measured cumulative baseline"
        );
        (cut, tokio::time::Instant::now() - start)
    }

    #[tokio::test(start_paused = true)]
    async fn a_supported_wait_polls_after_a_transient_initial_failure() {
        assert_eq!(
            core_wait_after_an_initial_unread_sample(false).await,
            (true, 12 * SEC)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_initial_failure_does_not_credit_a_historical_wake() {
        assert_eq!(
            core_wait_after_an_initial_unread_sample(true).await,
            (false, 60 * SEC)
        );
    }

    /// The first successful sample may discover an older sleep only after arming.
    /// Its observation timestamp cannot prove when that unmeasured sleep happened.
    #[tokio::test(start_paused = true)]
    async fn a_first_valid_sample_cannot_credit_sleep_of_unknown_origin() {
        let start = tokio::time::Instant::now();
        let wait = ArmedWait::capture_with(start + 60 * SEC, true, || None);
        let cut = wait
            .wait_with(POLL, || {
                Some(Reading {
                    asleep: 5120 * SEC,
                    last: Some(Wake {
                        at: (start + POLL).into_std(),
                        slept: 120 * SEC,
                        seq: 1,
                    }),
                })
            })
            .await;
        assert!(
            !cut,
            "an observation after arming is not a measured sleep interval"
        );
        assert_eq!(tokio::time::Instant::now(), start + 60 * SEC);
    }

    #[tokio::test(start_paused = true)]
    async fn unsupported_waits_keep_the_original_deadline_without_sampling() {
        let start = tokio::time::Instant::now();
        let wait = ArmedWait::capture_with(start + 60 * SEC, false, || {
            panic!("an unsupported platform must not sample")
        });
        let cut = wait
            .wait_with(POLL, || panic!("an unsupported wait must not poll"))
            .await;
        assert!(!cut);
        assert_eq!(tokio::time::Instant::now(), start + 60 * SEC);
    }

    /// The host supports both clocks the measurement needs. Quantitative admission
    /// and elapsed-sleep accounting use the deterministic controls above.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_platform_supports_its_sleep_clocks() {
        assert!(supported(), "this platform has the clock pair");
        #[cfg(target_os = "macos")]
        let clocks = [libc::CLOCK_MONOTONIC_RAW, libc::CLOCK_UPTIME_RAW];
        #[cfg(target_os = "linux")]
        let clocks = [libc::CLOCK_BOOTTIME, libc::CLOCK_MONOTONIC];
        for clock in clocks {
            assert!(clock_ns(clock).is_some(), "the platform clock is readable");
        }
    }

    // The process knew the old total before the wait, but did not observe the
    // host's intervening sleep. Only the first post-arm read discovers it.
    async fn core_wait_after_unobserved_pre_arm_sleep(
        later_sleep: bool,
    ) -> (bool, Duration, Option<u64>) {
        let start = tokio::time::Instant::now();
        let mut state = WakeState::default();
        assert_eq!(
            state.observe((start - 10 * SEC).into_std(), 5000 * SEC),
            None
        );
        let mut first = true;
        let mut read = || {
            if std::mem::take(&mut first) {
                return None;
            }
            let now = tokio::time::Instant::now();
            let asleep = if later_sleep && now >= start + 10 * SEC {
                5240 * SEC
            } else {
                5120 * SEC
            };
            Some(Reading {
                asleep,
                last: state.observe(now.into_std(), asleep),
            })
        };
        let wait = ArmedWait::capture_with(start + 60 * SEC, true, &mut read);
        let cut = wait.wait_with(POLL, read).await;
        (
            cut,
            tokio::time::Instant::now() - start,
            state.last().map(|wake| wake.seq),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn an_unread_arm_does_not_credit_sleep_first_discovered_after_arm() {
        assert_eq!(
            core_wait_after_unobserved_pre_arm_sleep(false).await,
            (false, 60 * SEC, Some(1))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_unread_arm_still_counts_sleep_after_its_first_valid_baseline() {
        assert_eq!(
            core_wait_after_unobserved_pre_arm_sleep(true).await,
            (true, 12 * SEC, Some(2))
        );
    }
}
