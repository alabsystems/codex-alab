// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A reported Cruise deadline at the precision the TUI displays.

use chrono::Datelike;
use chrono::Offset;
use chrono::Timelike;
use std::fmt;

/// The local minute of a scheduled landing and its weekday when it falls on another date.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Landing {
    pub(crate) time: chrono::NaiveTime,
    pub(crate) day: Option<chrono::Weekday>,
}

impl Landing {
    pub fn in_zone<Tz: chrono::TimeZone>(
        unix_seconds: u64,
        zone: &Tz,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<Self> {
        let at = zone
            .timestamp_opt(i64::try_from(unix_seconds).ok()?, 0)
            .single()?;
        let local = local_datetime(&at)?;
        let today = local_datetime(&now.with_timezone(zone))?.date();
        Some(Self {
            time: local.time().with_second(0)?,
            day: (local.date() != today).then(|| local.weekday()),
        })
    }

    /// Recheck the weekday on the next civil date while this landing is today or later.
    pub(crate) fn next_date_change<Tz: chrono::TimeZone>(
        unix_seconds: u64,
        now: &chrono::DateTime<Tz>,
    ) -> Option<chrono::DateTime<Tz>> {
        let zone = now.timezone();
        let timestamp = |seconds| zone.timestamp_opt(seconds, 0).single();
        let landing = timestamp(i64::try_from(unix_seconds).ok()?)?;
        let today = local_datetime(now)?.date();
        let landing_day = local_datetime(&landing)?.date();
        // Between two UTC readings that straddle a skipped or receding date,
        // find the first elapsed second with a different local date.
        let first_date_change = |mut after| {
            let mut before = now.timestamp();
            while after - before > 1 {
                let middle = before + (after - before) / 2;
                if local_datetime(&timestamp(middle)?)?.date() == today {
                    before = middle;
                } else {
                    after = middle;
                }
            }
            timestamp(after)
        };
        if landing_day <= today {
            // Some clocks roll from 00:01 back into yesterday. While inside
            // that first minute, the date changes before the second midnight.
            let start = today.and_time(chrono::NaiveTime::MIN);
            if let chrono::MappedLocalTime::Ambiguous(one, two) = zone.from_local_datetime(&start) {
                let before_second = one.max(two).timestamp() - 1;
                if before_second >= now.timestamp() {
                    let receded = local_datetime(&timestamp(before_second)?)?.date();
                    if receded < today && (landing_day == today || landing_day == receded) {
                        return first_date_change(before_second);
                    }
                }
            }
            if landing_day < today {
                return None;
            }
        }
        let midnight = today.succ_opt()?.and_time(chrono::NaiveTime::MIN);
        // Native zone databases can return the two offsets in either order or
        // normalize a missing midnight. Compare and verify the actual instants.
        let candidates = zone.from_local_datetime(&midnight);
        let first = candidates
            .clone()
            .earliest()
            .into_iter()
            .chain(candidates.latest())
            .filter(|candidate| candidate > now)
            .min();
        if let Some(first) = first
            && let Some(actual) = timestamp(first.timestamp())
            && local_datetime(&actual)?.date() > today
            && local_datetime(&timestamp(first.timestamp() - 1)?)?.date() == today
        {
            return Some(actual);
        }

        // A clock change can skip midnight or an entire local date. UTC offsets
        // are less than a day, so the following UTC midnight brackets the change.
        first_date_change(midnight.and_utc().timestamp() + 86_400)
    }

    pub(crate) fn clock(self) -> String {
        format!("{:02}:{:02}", self.time.hour(), self.time.minute())
    }

    /// Distinguish each displayed minute and weekday, including the same-day form.
    pub(crate) fn minute_key(self) -> u64 {
        let day = self.day.map_or(0, |day| 1 + day.num_days_from_monday());
        u64::from((day * 24 + self.time.hour()) * 60 + self.time.minute())
    }
}

fn local_datetime<Tz: chrono::TimeZone>(
    time: &chrono::DateTime<Tz>,
) -> Option<chrono::NaiveDateTime> {
    time.naive_utc().checked_add_offset(time.offset().fix())
}

impl fmt::Display for Landing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(day) = self.day {
            write!(f, "{day} ")?;
        }
        f.write_str(&self.clock())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use chrono::MappedLocalTime;
    use chrono::NaiveDate;
    use chrono::NaiveDateTime;
    use chrono::TimeZone;

    #[test]
    fn landing_date_timer_stops_after_the_landing_date() {
        let zone = FixedOffset::east_opt(5 * 3_600 + 45 * 60).unwrap();
        let midnight = zone.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap();
        let now = midnight - chrono::Duration::milliseconds(1);
        let seconds = |t: chrono::DateTime<FixedOffset>| t.timestamp().try_into().unwrap();
        for at in [now, midnight, midnight + chrono::Duration::days(2)] {
            assert_eq!(Landing::next_date_change(seconds(at), &now), Some(midnight));
        }
        assert_eq!(Landing::next_date_change(seconds(now), &midnight), None);
        assert_eq!(
            Landing::next_date_change(seconds(midnight), &midnight),
            Some(midnight + chrono::Duration::days(1))
        );
        assert_eq!(Landing::next_date_change(u64::MAX, &now), None);
        let last_utc = u64::try_from(chrono::DateTime::<chrono::Utc>::MAX_UTC.timestamp()).unwrap();
        assert_eq!(Landing::next_date_change(last_utc, &now), None);
        assert!(Landing::in_zone(last_utc, &zone, now.to_utc()).is_none());
    }

    // A single UTC transition lets these tests exercise real TimeZone resolution
    // independently of the machine's zone database or process-wide TZ setting.
    #[derive(Clone)]
    struct Shift<const BEFORE: i32, const AFTER: i32, const AT: i64>;

    impl<const BEFORE: i32, const AFTER: i32, const AT: i64> TimeZone for Shift<BEFORE, AFTER, AT> {
        type Offset = FixedOffset;

        fn from_offset(_: &Self::Offset) -> Self {
            Self
        }
        fn offset_from_local_date(&self, date: &NaiveDate) -> MappedLocalTime<Self::Offset> {
            self.offset_from_local_datetime(&date.and_time(chrono::NaiveTime::MIN))
        }
        fn offset_from_local_datetime(
            &self,
            local: &NaiveDateTime,
        ) -> MappedLocalTime<Self::Offset> {
            let before = FixedOffset::east_opt(BEFORE).unwrap();
            let after = FixedOffset::east_opt(AFTER).unwrap();
            let early = local
                .checked_sub_offset(before)
                .unwrap()
                .and_utc()
                .timestamp()
                < AT;
            let late = local
                .checked_sub_offset(after)
                .unwrap()
                .and_utc()
                .timestamp()
                >= AT;
            match (early, late) {
                // Match native resolvers that give the later UTC instant first.
                (true, true) => MappedLocalTime::Ambiguous(after, before),
                (true, false) => MappedLocalTime::Single(before),
                (false, true) => MappedLocalTime::Single(after),
                (false, false) => MappedLocalTime::None,
            }
        }
        fn offset_from_utc_date(&self, date: &NaiveDate) -> Self::Offset {
            self.offset_from_utc_datetime(&date.and_time(chrono::NaiveTime::MIN))
        }
        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
            FixedOffset::east_opt(if utc.and_utc().timestamp() < AT {
                BEFORE
            } else {
                AFTER
            })
            .unwrap()
        }
    }

    #[test]
    fn date_timer_finds_the_first_real_second_of_a_changed_date() {
        fn check<Tz: TimeZone>(zone: Tz, boundary: i64) {
            let now = zone.timestamp_opt(boundary - 60, 0).unwrap();
            let at = u64::try_from(boundary + 3_600).unwrap();
            assert_eq!(
                Landing::next_date_change(at, &now).unwrap().timestamp(),
                boundary
            );
        }
        const MIDNIGHT: i64 = 1_795_132_800; // 2026-11-20 00:00 UTC
        const NOON: i64 = MIDNIGHT + 43_200;
        check(Shift::<0, 3_600, MIDNIGHT>, MIDNIGHT); // 00:00 skipped
        check(Shift::<3_600, 0, MIDNIGHT>, MIDNIGHT - 3_600); // first of two 00:00s
        check(Shift::<-43_200, 43_200, NOON>, NOON); // a whole date skipped
    }

    #[test]
    fn short_first_date_recedes_and_returns_at_the_second_midnight() {
        const MIDNIGHT: i64 = 1_795_132_800;
        const ROLLBACK: i64 = MIDNIGHT + 60;
        let zone = Shift::<0, -3_600, ROLLBACK>;
        let due = u64::try_from(MIDNIGHT + 7_200).unwrap();
        let next = |now, at| {
            Landing::next_date_change(at, &zone.timestamp_opt(now, 0).unwrap())
                .map(|change| change.timestamp())
        };
        assert_eq!(next(MIDNIGHT - 1, due), Some(MIDNIGHT));
        assert_eq!(next(MIDNIGHT + 30, due), Some(ROLLBACK));
        assert_eq!(next(ROLLBACK + 1, due), Some(MIDNIGHT + 3_600));
        let receded_day = u64::try_from(ROLLBACK + 120).unwrap();
        assert_eq!(next(MIDNIGHT + 30, receded_day), Some(ROLLBACK));
    }
}
