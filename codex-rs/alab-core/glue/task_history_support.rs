// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Existing history support predicate shared by actual local/backend and remote TUI routes.
use codex_app_server_protocol::JSONRPCErrorError;

pub(crate) fn is_history_pagination_unsupported(source: &JSONRPCErrorError) -> bool {
    if source.code == -32601 {
        return true;
    }

    if !matches!(source.code, -32600 | -32602) {
        return false;
    }

    let message = source.message.to_ascii_lowercase();
    [
        "historymode",
        "history mode",
        "excludeturns",
        "exclude turns",
        "thread/turns/list",
        "thread/items/list",
    ]
    .into_iter()
    .any(|field| message.contains(field))
        || (message.contains("paginated")
            && ["unknown variant", "unsupported variant", "invalid enum"]
                .into_iter()
                .any(|error| message.contains(error)))
}
