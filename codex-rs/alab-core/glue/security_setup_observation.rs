// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Record verified absence only after successful HTTP decoding and identity validation.

use codex_app_server_protocol::AccountSecuritySetupNotice;
use codex_app_server_protocol::AlabAccountSecuritySetupIdentity;

pub(super) fn verified(
    observed: &mut Option<AlabAccountSecuritySetupIdentity>,
    account_id: String,
    user_id: String,
    notice: Option<super::Notice>,
) -> Option<AccountSecuritySetupNotice> {
    *observed = notice.is_none().then(|| AlabAccountSecuritySetupIdentity {
        account_id: account_id.clone(),
        user_id: user_id.clone(),
    });
    notice.map(|notice| AccountSecuritySetupNotice {
        account_id,
        user_id,
        title: notice.title,
        description: notice.description,
        action: notice.action,
    })
}
