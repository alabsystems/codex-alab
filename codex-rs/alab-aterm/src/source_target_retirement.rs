// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Release on a fresh authenticated channel after a lost OPEN acknowledgement.

use super::*;

frame!(RetiredFrame, serde_json::Value,
    state: String, open_operation: String, assignment: String, context: String);

impl Target {
    pub async fn retire_open_attempt(
        &self,
        operation: &str,
        open_operation: &str,
        member: Option<&str>,
    ) -> io::Result<native_target::OpenRetirement> {
        if !valid_hex(operation, 32)
            || !valid_hex(open_operation, 32)
            || member.is_some_and(|member| !valid_hex(member, 32))
        {
            return Err(unavailable());
        }
        let parent = &self.role.transport;
        let transport = connect_transport(&parent.endpoint, &parent.birth).await?;
        transport.bind_epoch(&parent.epoch()?.ok_or_else(unavailable)?)?;
        let result = async {
            let fields = format!(
                "{} open_operation={}{}",
                self.role.fields(),
                open_operation,
                member
                    .map(|member| format!(" member={member}"))
                    .unwrap_or_default(),
            );
            let body = transport
                .call("member-target-open-close", operation, &fields, member)
                .await?;
            let frame: RetiredFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
            frame.validate(
                &frame.request,
                operation,
                &transport.sid,
                &transport.birth,
                transport.epoch()?.as_deref(),
            )?;
            let member_matches = match (member, &frame.member) {
                (None, serde_json::Value::Null) => true,
                (Some(expected), serde_json::Value::String(actual)) => expected == actual,
                _ => false,
            };
            if frame.state != "closed"
                || frame.open_operation != open_operation
                || frame.assignment != self.role.context.assignment()
                || frame.context != self.role.context.private_capability()
                || !member_matches
            {
                return Err(unavailable());
            }
            native_target::OpenRetirement::from_native(
                &self.role.context,
                operation.into(),
                open_operation.into(),
                member.map(str::to_owned),
            )
        }
        .await;
        transport.conn.close();
        result
    }
}
