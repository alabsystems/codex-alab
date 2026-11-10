// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Private destination transport. This module installs no production Host.

use super::*;
use codex_alab_core::native_target;

#[path = "source_target_resource.rs"]
mod resource;
#[path = "source_target_retirement.rs"]
mod retirement;

frame!(ContextFrame, serde_json::Value, context: String, assignment_generation: u64, target_ref: String);

pub(super) struct Role {
    pub(super) context: native_target::Context,
    transport: Arc<Transport>,
}
impl Role {
    pub(super) fn fields(&self) -> String {
        format!(
            " assignment={} context={}",
            self.context.assignment(),
            self.context.private_capability()
        )
    }
}

/// Owns the authenticated native context channel across initialization and ACK.
/// Members share its actual role internally; callers cannot clone this owner.
pub struct Target {
    role: Arc<Role>,
}
impl native_target::Destination for Target {
    fn context(&self) -> &native_target::Context {
        Target::context(self)
    }

    fn open_member<'a>(&'a self, operation: &'a str) -> core::MemberFuture<'a> {
        Box::pin(self.open(operation))
    }

    fn retire_open<'a>(
        &'a self,
        operation: &'a str,
        open_operation: &'a str,
        member: Option<&'a str>,
    ) -> native_target::OpenRetirementFuture<'a> {
        Box::pin(self.retire_open_attempt(operation, open_operation, member))
    }

    fn acknowledge<'a>(
        &'a self,
        receipt: &'a native_target::ResourceReceipt,
    ) -> native_target::AppliedFuture<'a> {
        Box::pin(self.acknowledge_receipt(receipt))
    }
}
impl std::fmt::Debug for Target {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeTarget(<private>)")
    }
}
impl Target {
    pub fn context(&self) -> &native_target::Context {
        &self.role.context
    }

    /// The caller retains this exact OPEN attempt before polling the future.
    /// A failed or cancelled request is Unknown until native owned retirement.
    pub async fn open(&self, operation: &str) -> io::Result<Arc<dyn core::Member>> {
        if !valid_hex(operation, 32) || !self.role.transport.conn.is_open() {
            return Err(unavailable());
        }
        let parent = &self.role.transport;
        // Verify the retained native instance before sending the OPEN mutation.
        let peer = work_rebind::PeerIdentity::capture(parent)?;
        let hello = format!("{:016x}{:016x}", core::connection_request_prefix(), core::connection_request_prefix());
        let transport = peer.reconnect_peer(&hello).await?;
        let result = async {
            let body = transport
                .call("member-open-target", operation, &self.role.fields(), None)
                .await?;
            let frame: CommonFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
            frame.validate(
                &frame.request,
                operation,
                &transport.sid,
                &transport.birth,
                transport.epoch()?.as_deref(),
            )?;
            if !valid_hex(&frame.member, 32) {
                return Err(unavailable());
            }
            Ok(frame.member)
        }
        .await;
        match result {
            Ok(id) => Ok(Arc::new(NativeMember::new(transport, id, operation.into(), Some(Arc::clone(&self.role)), work_rebind::OriginalBody::Target {
                assignment: self.role.context.assignment().into(), context: self.role.context.private_capability().into(),
            })?)),
            Err(error) => {
                transport.conn.close();
                Err(error)
            }
        }
    }
}

/// Resolve only an actual native Installed entry on an authenticated channel.
/// This function does not prepare/install a target or grant migration admission.
pub async fn context_at(endpoint: &Endpoint, assignment: &str, birth: &str) -> io::Result<Target> {
    if !crate::locate::valid_sid(&endpoint.sid)
        || !valid_hex(assignment, 32)
        || birth.is_empty()
        || birth.len() > 128
        || birth.chars().any(char::is_control)
    {
        return Err(unavailable());
    }
    let transport = connect_transport(endpoint, birth).await?;
    let result = async {
        let body = transport
            .call("member-target-context", assignment, "", None)
            .await?;
        let frame: ContextFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
        frame.validate(
            &frame.request,
            assignment,
            &transport.sid,
            &transport.birth,
            transport.epoch()?.as_deref(),
        )?;
        if !frame.member.is_null() {
            return Err(unavailable());
        }
        native_target::Context::from_native(
            assignment.into(),
            frame.context,
            frame.assignment_generation,
            frame.target_ref,
        )
    }
    .await;
    match result {
        Ok(context) => Ok(Target {
            role: Arc::new(Role {
                context,
                transport: Arc::new(transport),
            }),
        }),
        Err(error) => {
            transport.conn.close();
            Err(error)
        }
    }
}
