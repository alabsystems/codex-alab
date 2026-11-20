// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::POLL;
use super::Reading;
use super::WaitDeadline;
use super::read;
use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::ptr;
use std::time::Duration;
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;
use tokio::time::Instant;

unsafe extern "C" {
    fn mach_continuous_time() -> u64;
}

pub(super) async fn park(
    deadline: WaitDeadline,
    read_selected: impl FnMut() -> Option<Reading>,
) -> WaitDeadline {
    park_with(deadline, read_selected, register).await
}

async fn park_with(
    mut deadline: WaitDeadline,
    mut read_selected: impl FnMut() -> Option<Reading>,
    mut arm: impl FnMut(u64, Duration) -> io::Result<AsyncFd<OwnedFd>>,
) -> WaitDeadline {
    while let Some((anchor, remaining)) = prepare(&mut deadline, &mut read_selected, || {
        // Anchor before the current sample so a suspend during setup cannot delay the hint.
        // SAFETY: the system clock has no arguments or preconditions.
        let anchor = unsafe { mach_continuous_time() };
        read().map(|reading| (anchor, reading))
    })
    .await
    {
        let Ok(timer) = arm(anchor, remaining) else {
            break;
        };
        let rearm_after = Instant::now() + POLL;
        // Tokio's deadline also preserves a runtime clock advanced by tests.
        let kernel_hint = tokio::select! {
            ready = timer.readable() => ready.is_ok(),
            () = tokio::time::sleep_until(deadline.at) => false,
        };
        drop(timer);
        if let Some(sample) = read() {
            deadline.observe(sample, Instant::now());
        }
        if !kernel_hint || deadline.at.saturating_duration_since(Instant::now()) <= POLL {
            break;
        }
        // A suspend between the Mach anchor and the sample can make the hint
        // arrive early. Recheck and park for the remaining wait, arming at
        // most once per sample interval if notifications keep arriving early.
        if Instant::now() < rearm_after {
            tokio::time::sleep_until(rearm_after).await;
        }
    }
    deadline
}

async fn prepare(
    deadline: &mut WaitDeadline,
    mut read_selected: impl FnMut() -> Option<Reading>,
    mut read_current: impl FnMut() -> Option<(u64, Reading)>,
) -> Option<(u64, Duration)> {
    loop {
        let now = Instant::now();
        if deadline.at.saturating_duration_since(now) <= POLL {
            return None;
        }
        // Record publication can have captured the first valid baseline while we
        // waited. Keep that origin across rejected current samples and credit only
        // the sleep a subsequent accepted reading measures from it.
        if deadline.asleep.is_none() {
            deadline.asleep = read_selected().map(|reading| reading.asleep);
        }
        if deadline.asleep.is_some()
            && let Some((anchor, sample)) = read_current()
        {
            let now = Instant::now();
            deadline.observe(sample, now);
            let remaining = deadline.at.saturating_duration_since(now);
            return (remaining > POLL).then_some((anchor, remaining));
        }
        tokio::time::sleep_until(now + POLL).await;
    }
}

fn register(anchor: u64, remaining: Duration) -> io::Result<AsyncFd<OwnedFd>> {
    // The kernel reports the number of Mach timebase ticks per second. Use
    // the same scale as the host wake reading in the parent module.
    let hz = super::sysctl_u64(c"hw.tbfrequency")
        .filter(|hz| *hz > 0)
        .ok_or_else(|| io::Error::other("Mach timebase unavailable"))?;
    let ticks = remaining
        .as_nanos()
        .checked_mul(u128::from(hz))
        .ok_or_else(|| io::Error::other("Mach timer interval out of range"))?
        / 1_000_000_000;
    let at = i64::try_from(u128::from(anchor) + ticks)
        .map_err(|_| io::Error::other("Mach timer deadline out of range"))?;
    // SAFETY: kqueue has no arguments.
    let raw = unsafe { libc::kqueue() };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful call returned a fresh descriptor; take its sole ownership.
    let queue = unsafe { OwnedFd::from_raw_fd(raw) };
    // SAFETY: raw remains open through both registration calls.
    if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let event = libc::kevent64_s {
        ident: 1,
        filter: libc::EVFILT_TIMER,
        flags: libc::EV_ADD | libc::EV_ONESHOT,
        fflags: libc::NOTE_MACHTIME | libc::NOTE_ABSOLUTE | libc::NOTE_MACH_CONTINUOUS_TIME,
        data: at,
        udata: 0,
        ext: [0; 2],
    };
    // SAFETY: one initialized change, no output records, no blocking wait.
    if unsafe {
        libc::kevent64(
            queue.as_raw_fd(),
            &event,
            1,
            ptr::null_mut(),
            0,
            0,
            ptr::null(),
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    AsyncFd::with_interest(queue, Interest::READABLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_recovered_sample_parks_from_the_recorded_baseline() {
        let now = Instant::now();
        let hour = Duration::from_secs(3600);
        let mut deadline = WaitDeadline {
            at: now + hour,
            asleep: None,
        };
        let reading = |seconds| Reading {
            asleep: Duration::from_secs(seconds),
            last: None,
        };
        // The first valid selection came from the record; a later sample is
        // rejected before we measure the subsequent thirty seconds of sleep.
        let mut selected = [None, Some(reading(100))].into_iter();
        let mut current = [None, Some((42, reading(130)))].into_iter();
        let result = prepare(
            &mut deadline,
            || selected.next().expect("selection is retained"),
            || current.next().expect("latest sample"),
        )
        .await;
        assert_eq!(now.elapsed(), POLL * 2);
        assert_eq!(
            result,
            Some((42, hour - Duration::from_secs(30) - POLL * 2))
        );
        assert_eq!(deadline.asleep, Some(Duration::from_secs(130)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_wait_that_remains_unread_ends_at_its_original_deadline() {
        let start = Instant::now();
        let at = start + POLL * 3;
        let mut deadline = WaitDeadline { at, asleep: None };
        assert_eq!(
            prepare(&mut deadline, || None, || panic!("no accepted baseline")).await,
            None
        );
        assert_eq!(start.elapsed(), POLL * 2);
        super::super::sleep_tracks_until(deadline, None, POLL, || None, |_| {}).await;
        assert_eq!(Instant::now(), at);
    }

    #[tokio::test]
    async fn an_early_kernel_hint_rearms_the_rest_of_a_dated_wait() {
        let week = Duration::from_secs(7 * 24 * 60 * 60);
        let deadline = WaitDeadline {
            at: Instant::now() + week,
            asleep: None,
        };
        let mut arms = Vec::new();
        let wait = park_with(deadline, read, |anchor, remaining| {
            arms.push(remaining);
            if arms.len() == 1 {
                // Deliver a real kernel hint well before the selected deadline.
                register(anchor, Duration::from_millis(1))
            } else {
                // Stop at the second arm without spending a week in this test.
                Err(io::Error::other("test registration failure"))
            }
        });
        let still_waiting = tokio::time::timeout(Duration::from_secs(15), wait)
            .await
            .expect("a premature hint must promptly rearm the remaining wait");
        assert_eq!(
            arms.len(),
            2,
            "a week must not fall back to one-second polling"
        );
        assert!(arms.iter().all(|remaining| *remaining > week / 2));
        assert!(still_waiting.at > Instant::now() + week / 2);
    }

    #[tokio::test]
    async fn the_continuous_timer_delivers_a_single_deadline_to_tokios_io_driver() {
        let now = Instant::now();
        // SAFETY: reading the system clock requires no resources.
        let start = unsafe { mach_continuous_time() };
        let duration = Duration::from_millis(80);
        let queue = register(start, duration).expect("arm continuous timer");
        let _ready = tokio::time::timeout(Duration::from_secs(5), queue.readable())
            .await
            .expect("kernel timer fired")
            .expect("readable kqueue");
        assert!(now.elapsed() >= duration.saturating_sub(Duration::from_millis(8)));
    }
}
