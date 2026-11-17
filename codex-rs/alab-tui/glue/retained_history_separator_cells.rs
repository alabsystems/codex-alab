// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Preserve historical completion presentation without sampling a new clock,
//! changing its original date/offset or reconstructing runtime execution facts.
use super::*;
use crate::history_cell::alab_retained_cells as cells;
use crate::terminal_hyperlinks::alab_retained_text as native;
use codex_otel::RuntimeMetricTotals;
use serde::Deserialize;
use serde::Serialize;
use std::io;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DateData {
    year: i32,
    month: u32,
    day: u32,
}
impl DateData {
    fn capture(actual: NaiveDate) -> Self {
        Self {
            year: actual.year(),
            month: actual.month(),
            day: actual.day(),
        }
    }
    fn restore(self) -> io::Result<NaiveDate> {
        NaiveDate::from_ymd_opt(self.year, self.month, self.day)
            .ok_or_else(|| native::invalid("invalid historical display date"))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompletionData {
    seconds: i64,
    nanos: u32,
    offset: i32,
}
impl CompletionData {
    fn capture(actual: DateTime<Local>) -> Self {
        Self {
            seconds: actual.timestamp(),
            nanos: actual.timestamp_subsec_nanos(),
            offset: actual.offset().local_minus_utc(),
        }
    }
    fn restore(self) -> io::Result<DateTime<Local>> {
        let utc = DateTime::from_timestamp(self.seconds, self.nanos)
            .ok_or_else(|| native::invalid("invalid original completion instant"))?;
        let offset = chrono::FixedOffset::east_opt(self.offset)
            .ok_or_else(|| native::invalid("invalid original completion offset"))?;
        Ok(DateTime::<Local>::from_naive_utc_and_offset(
            utc.naive_utc(),
            offset,
        ))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricsData {
    tool_calls: [u64; 2],
    api_calls: [u64; 2],
    streaming_events: [u64; 2],
    websocket_calls: [u64; 2],
    websocket_events: [u64; 2],
    overhead: u64,
    inference: u64,
    iapi_ttft: u64,
    service_ttft: u64,
    iapi_tbt_bits: u64,
    service_tbt_bits: u64,
    turn_ttft: u64,
    turn_ttfm: u64,
}
fn totals(actual: RuntimeMetricTotals) -> [u64; 2] {
    [actual.count, actual.duration_ms]
}
fn restore_totals([count, duration_ms]: [u64; 2]) -> RuntimeMetricTotals {
    RuntimeMetricTotals { count, duration_ms }
}
impl MetricsData {
    fn capture(actual: RuntimeMetricsSummary) -> Self {
        Self {
            tool_calls: totals(actual.tool_calls),
            api_calls: totals(actual.api_calls),
            streaming_events: totals(actual.streaming_events),
            websocket_calls: totals(actual.websocket_calls),
            websocket_events: totals(actual.websocket_events),
            overhead: actual.responses_api_overhead_ms,
            inference: actual.responses_api_inference_time_ms,
            iapi_ttft: actual.responses_api_engine_iapi_ttft_ms,
            service_ttft: actual.responses_api_engine_service_ttft_ms,
            iapi_tbt_bits: actual.responses_api_engine_iapi_tbt_ms.to_bits(),
            service_tbt_bits: actual.responses_api_engine_service_tbt_ms.to_bits(),
            turn_ttft: actual.turn_ttft_ms,
            turn_ttfm: actual.turn_ttfm_ms,
        }
    }
    fn restore(self) -> RuntimeMetricsSummary {
        RuntimeMetricsSummary {
            tool_calls: restore_totals(self.tool_calls),
            api_calls: restore_totals(self.api_calls),
            streaming_events: restore_totals(self.streaming_events),
            websocket_calls: restore_totals(self.websocket_calls),
            websocket_events: restore_totals(self.websocket_events),
            responses_api_overhead_ms: self.overhead,
            responses_api_inference_time_ms: self.inference,
            responses_api_engine_iapi_ttft_ms: self.iapi_ttft,
            responses_api_engine_service_ttft_ms: self.service_ttft,
            responses_api_engine_iapi_tbt_ms: f64::from_bits(self.iapi_tbt_bits),
            responses_api_engine_service_tbt_ms: f64::from_bits(self.service_tbt_bits),
            turn_ttft_ms: self.turn_ttft,
            turn_ttfm_ms: self.turn_ttfm,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::history_cell) struct Data {
    elapsed_seconds: Option<u64>,
    metrics: Option<MetricsData>,
    completed_at: Option<CompletionData>,
    display_date: DateData,
    clock: u8,
}
impl Data {
    pub(in crate::history_cell) fn capture(
        actual: &dyn HistoryCell,
        _context: &mut cells::CaptureContext<'_>,
        nesting: usize,
    ) -> io::Result<Option<Self>> {
        cells::depth(nesting)?;
        Ok(actual
            .as_any()
            .downcast_ref::<FinalMessageSeparator>()
            .map(Self::capture_separator))
    }
    fn capture_separator(actual: &FinalMessageSeparator) -> Self {
        Self {
            elapsed_seconds: actual.elapsed_seconds,
            metrics: actual.runtime_metrics.map(MetricsData::capture),
            completed_at: actual.completed_at.map(CompletionData::capture),
            display_date: DateData::capture(actual.display_date),
            clock: match actual.clock_format {
                ClockFormat::TwelveHour => 0,
                ClockFormat::TwentyFourHour => 1,
            },
        }
    }
    fn restore_separator(self) -> io::Result<FinalMessageSeparator> {
        let clock_format = match self.clock {
            0 => ClockFormat::TwelveHour,
            1 => ClockFormat::TwentyFourHour,
            _ => return Err(native::invalid("unknown original clock format")),
        };
        Ok(FinalMessageSeparator {
            elapsed_seconds: self.elapsed_seconds,
            runtime_metrics: self.metrics.map(MetricsData::restore),
            completed_at: self.completed_at.map(CompletionData::restore).transpose()?,
            display_date: self.display_date.restore()?,
            clock_format,
        })
    }
    pub(in crate::history_cell) fn restore(
        self,
        _shared: &mut native::Restored,
        _resources: &mut cells::RestoreContext,
        nesting: usize,
    ) -> io::Result<Box<dyn HistoryCell>> {
        cells::depth(nesting)?;
        Ok(Box::new(self.restore_separator()?))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn original() -> FinalMessageSeparator {
        let offset = chrono::FixedOffset::east_opt(19_800).unwrap();
        let utc = DateTime::from_timestamp(1_704_067_200, 123_456_789).unwrap();
        FinalMessageSeparator {
            elapsed_seconds: Some(3661),
            runtime_metrics: Some(RuntimeMetricsSummary {
                tool_calls: RuntimeMetricTotals {
                    count: 7,
                    duration_ms: 31,
                },
                responses_api_engine_iapi_tbt_ms: -0.0,
                responses_api_engine_service_tbt_ms: f64::from_bits(0x7ff8_0000_0000_0123),
                ..Default::default()
            }),
            completed_at: Some(DateTime::<Local>::from_naive_utc_and_offset(
                utc.naive_utc(),
                offset,
            )),
            display_date: NaiveDate::from_ymd_opt(2020, 2, 29).unwrap(),
            clock_format: ClockFormat::TwentyFourHour,
        }
    }
    #[test]
    fn native_separator_keeps_original_fixed_offset_display_date_and_labels() -> io::Result<()> {
        let original = original();
        let raw =
            serde_json::to_vec(&Data::capture_separator(&original)).map_err(native::invalid)?;
        let data: Data = serde_json::from_slice(&raw).map_err(native::invalid)?;
        let restored = data.restore_separator()?;
        assert_eq!(restored.completed_at, original.completed_at);
        assert_eq!(
            restored.completed_at.unwrap().offset(),
            original.completed_at.unwrap().offset()
        );
        assert_eq!(restored.display_date, original.display_date);
        assert_eq!(restored.clock_format, original.clock_format);
        assert_eq!(restored.display_label(), original.display_label());
        for width in [0, 10, 80] {
            assert_eq!(restored.display_lines(width), original.display_lines(width));
        }
        Ok(())
    }
    #[test]
    fn separator_metric_data_preserves_all_floating_bits_including_signed_zero_and_nan()
    -> io::Result<()> {
        let actual = original().runtime_metrics.unwrap();
        let saved = MetricsData::capture(actual);
        let restored = serde_json::from_slice::<MetricsData>(
            &serde_json::to_vec(&saved).map_err(native::invalid)?,
        )
        .map_err(native::invalid)?
        .restore();
        assert_eq!(
            restored.responses_api_engine_iapi_tbt_ms.to_bits(),
            actual.responses_api_engine_iapi_tbt_ms.to_bits()
        );
        assert_eq!(
            restored.responses_api_engine_service_tbt_ms.to_bits(),
            actual.responses_api_engine_service_tbt_ms.to_bits()
        );
        assert_eq!(restored.tool_calls, actual.tool_calls);
        Ok(())
    }
    #[test]
    fn malformed_native_completion_date_offset_and_clock_are_refused() -> io::Result<()> {
        let mut data = Data::capture_separator(&original());
        data.clock = 255;
        assert!(data.restore_separator().is_err());
        let mut data = Data::capture_separator(&original());
        data.display_date.month = 13;
        assert!(data.restore_separator().is_err());
        let mut data = Data::capture_separator(&original());
        data.completed_at.as_mut().unwrap().offset = 86400;
        assert!(data.restore_separator().is_err());
        Ok(())
    }
}
