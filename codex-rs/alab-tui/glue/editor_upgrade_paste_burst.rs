// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Paste data only. No event is flushed, reclassified or replayed by this codec.
use super::*;

crate::alab_editor_upgrade::native_state!(
    super::PasteBurst,
    PasteBurstWire,
    #[serde(remote = "super::PasteBurst", deny_unknown_fields)]
    struct PasteBurstWire {
        #[serde(with = "codex_alab_tui::upgrade_editor_clock::optional")]
        last_plain_char_time: Option<Instant>,
        consecutive_plain_char_burst: u16,
        #[serde(with = "codex_alab_tui::upgrade_editor_clock::optional")]
        burst_window_until: Option<Instant>,
        buffer: String,
        active: bool,
        #[serde(with = "codex_alab_tui::upgrade_editor_clock::pending_char")]
        pending_first_char: Option<(char, Instant)>,
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(source: &PasteBurst) -> PasteBurst {
        serde_json::from_slice(&serde_json::to_vec(source).unwrap()).unwrap()
    }

    #[test]
    fn native_held_character_keeps_its_paste_detection_window() {
        let at = Instant::now() - Duration::from_secs(1);
        let mut source = PasteBurst::default();
        assert!(matches!(
            source.on_plain_char('a', at),
            CharDecision::RetainFirstChar
        ));
        let mut restored = round_trip(&source);
        let next = at + Duration::from_millis(2);
        assert!(matches!(
            source.on_plain_char('b', next),
            CharDecision::BeginBufferFromPending
        ));
        assert!(matches!(
            restored.on_plain_char('b', next),
            CharDecision::BeginBufferFromPending
        ));
        source.append_char_to_buffer('b', next);
        restored.append_char_to_buffer('b', next);
        for burst in [&mut source, &mut restored] {
            assert!(matches!(
                burst.flush_if_due(next + Duration::from_millis(2)),
                FlushResult::None
            ));
            let FlushResult::Paste(text) = burst.flush_if_due(next + Duration::from_millis(20))
            else {
                panic!("restored burst lost its original paste classification");
            };
            assert_eq!(text, "ab");
        }
    }

    #[test]
    fn expired_held_character_and_active_paste_do_not_get_new_timeouts() {
        let at = Instant::now() - Duration::from_secs(1);
        let mut held = PasteBurst::default();
        held.on_plain_char('x', at);
        let mut restored = round_trip(&held);
        assert!(matches!(
            restored.flush_if_due(Instant::now()),
            FlushResult::Typed('x')
        ));
        assert!(matches!(
            restored.flush_if_due(Instant::now()),
            FlushResult::None
        ));

        let mut active = PasteBurst::default();
        // The real caller records the triggering character before retro-grabbing.
        active.on_plain_char_no_hold(at);
        active.begin_with_retro_grabbed("hello".into(), at);
        let mut restored = round_trip(&active);
        assert!(restored.newline_should_insert_instead_of_submit(at + Duration::from_millis(100)));
        let FlushResult::Paste(text) = restored.flush_if_due(Instant::now()) else {
            panic!("active paste deadline was restarted or data was lost");
        };
        assert_eq!(text, "hello");
        assert!(!restored.newline_should_insert_instead_of_submit(Instant::now()));
    }
}
