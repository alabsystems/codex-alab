//! Split older items at turn boundaries so each terminal row is rendered on the page that owns it.

pub(super) use crate::app_server_session::history_page::group_turn_items; // ALAB(history): group by scoped item identity in page order, returning bounded slices.

#[cfg(test)]
#[path = "history_completion_tests.rs"]
mod tests;
