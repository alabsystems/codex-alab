// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact native paging state. Cursor DATA creates neither a reader nor a loaded page.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::io;
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn required<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Data {
    history_mode: u8,
    #[serde(deserialize_with = "required")]
    next_turn_cursor: Option<String>,
    #[serde(deserialize_with = "required")]
    next_item_cursor: Option<String>,
    seen_turn_cursors: Vec<String>,
    seen_item_cursors: Vec<String>,
    loading_older: bool,
}
impl Data {
    pub(crate) fn capture(actual: &ThreadHistoryPagination) -> Self {
        let mut turns = actual.seen_turn_cursors.iter().cloned().collect::<Vec<_>>();
        turns.sort();
        let mut items = actual.seen_item_cursors.iter().cloned().collect::<Vec<_>>();
        items.sort();
        Self {
            history_mode: match actual.history_mode {
                ThreadHistoryMode::Legacy => 0,
                ThreadHistoryMode::Paginated => 1,
            },
            next_turn_cursor: actual.next_turn_cursor.clone(),
            next_item_cursor: actual.next_item_cursor.clone(),
            seen_turn_cursors: turns,
            seen_item_cursors: items,
            loading_older: actual.loading_older,
        }
    }
    pub(crate) fn prepare(&self) -> io::Result<ThreadHistoryPagination> {
        if self
            .seen_turn_cursors
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
            || self
                .seen_item_cursors
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(invalid(
                "native paging cursor sets are duplicated or noncanonical",
            ));
        }
        let history_mode = match self.history_mode {
            0 => ThreadHistoryMode::Legacy,
            1 => ThreadHistoryMode::Paginated,
            _ => return Err(invalid("native paging history mode")),
        };
        Ok(ThreadHistoryPagination {
            history_mode,
            next_turn_cursor: self.next_turn_cursor.clone(),
            next_item_cursor: self.next_item_cursor.clone(),
            seen_turn_cursors: self.seen_turn_cursors.iter().cloned().collect(),
            seen_item_cursors: self.seen_item_cursors.iter().cloned().collect(),
            loading_older: self.loading_older,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_native_cursor_sets_and_pending_loading_survive_data_roundtrip() -> io::Result<()> {
        let original = ThreadHistoryPagination {
            history_mode: ThreadHistoryMode::Paginated,
            next_turn_cursor: Some("actual opaque turn cursor".into()),
            next_item_cursor: Some("actual opaque item cursor".into()),
            seen_turn_cursors: HashSet::from(["turn-old".into(), "turn-new".into()]),
            seen_item_cursors: HashSet::from(["item-old".into()]),
            loading_older: true,
        };
        let data = Data::capture(&original);
        let wire = serde_json::to_vec(&data).map_err(invalid)?;
        let decoded: Data = serde_json::from_slice(&wire).map_err(invalid)?;
        let native = decoded.prepare()?;
        if native.history_mode != original.history_mode
            || native.next_turn_cursor != original.next_turn_cursor
            || native.next_item_cursor != original.next_item_cursor
            || native.seen_turn_cursors != original.seen_turn_cursors
            || native.seen_item_cursors != original.seen_item_cursors
            || !native.loading_older
        {
            return Err(invalid("native paging state was changed or defaulted"));
        }
        Ok(())
    }
    #[test]
    fn missing_nullable_cursor_and_invalid_mode_are_not_fresh_empty_history() -> io::Result<()> {
        let data = Data::capture(&ThreadHistoryPagination::default());
        let mut wire = serde_json::to_value(&data).map_err(invalid)?;
        wire.as_object_mut()
            .ok_or_else(|| invalid("paging DATA is not object"))?
            .remove("next_item_cursor");
        if serde_json::from_value::<Data>(wire).is_ok() {
            return Err(invalid("missing original cursor defaulted to absence"));
        }
        let mut malformed = data;
        malformed.history_mode = 2;
        if malformed.prepare().is_ok() {
            return Err(invalid("unknown native history mode became a default"));
        }
        Ok(())
    }
}
