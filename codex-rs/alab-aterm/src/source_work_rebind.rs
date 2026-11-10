// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Existing-member work-channel recovery. A bound channel is neither a READY
//! receipt nor authority to allocate, begin work, or obtain credentials.

use super::*;

#[derive(Clone)]
pub(super) enum OriginalBody {
    Ordinary,
    Derived {
        use_member: String,
        use_operation: String,
        capability: String,
    },
    Target {
        assignment: String,
        context: String,
    },
}

impl OriginalBody {
    fn fields(&self) -> io::Result<String> {
        match self {
            Self::Ordinary => Ok(" open_kind=ordinary".into()),
            Self::Derived {
                use_member,
                use_operation,
                capability,
            } if valid_hex(use_member, 32)
                && valid_hex(use_operation, 32)
                && valid_hex(capability, 64) =>
            {
                Ok(format!(
                    " open_kind=derived use_member={use_member} use_operation={use_operation} capability={capability}"
                ))
            }
            Self::Target {
                assignment,
                context,
            } if valid_hex(assignment, 32) && valid_hex(context, 64) => Ok(format!(
                " open_kind=target assignment={assignment} context={context}"
            )),
            _ => Err(unavailable()),
        }
    }
}

frame!(OrdinaryBound, String, state: String, open_kind: String);
frame!(DerivedBound, String, state: String, open_kind: String,
    use_member: String, use_operation: String, capability: String);
frame!(TargetBound, String, state: String, open_kind: String,
    assignment: String, context: String);
frame!(PeerFrame, serde_json::Value);

/// Actual peer metadata, available before OPEN only after a validated exchange
/// on this same connection. An unbound transport cannot manufacture an epoch.
pub(super) struct PeerIdentity {
    endpoint: Endpoint,
    birth: String,
    epoch: String,
}

impl std::fmt::Debug for PeerIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativePeerIdentity(<private>)")
    }
}

impl PeerIdentity {
    /// Discover identity on the same persistent channel before an ordinary
    /// OPEN. Transport must allow only this metadata action's null member.
    pub(super) async fn establish(original: &Transport, operation: &str) -> io::Result<Self> {
        let result = async {
            let body = original.call("member-peer", operation, "", None).await?;
            let frame: PeerFrame = serde_json::from_str(&body).map_err(|_| unavailable())?;
            frame.validate(
                &frame.request,
                operation,
                &original.sid,
                &original.birth,
                original.epoch()?.as_deref(),
            )?;
            if !frame.member.is_null() {
                return Err(unavailable());
            }
            Self::capture(original)
        }
        .await;
        if result.is_err() {
            original.conn.close();
        }
        result
    }

    pub(super) fn capture(original: &Transport) -> io::Result<Self> {
        let epoch = original.epoch()?.ok_or_else(unavailable)?;
        if epoch.is_empty() || epoch.len() > 128 || epoch.chars().any(char::is_control) {
            return Err(unavailable());
        }
        Ok(Self {
            endpoint: original.endpoint.clone(),
            birth: original.birth.clone(),
            epoch,
        })
    }

    /// A replacement channel must prove the retained peer before any OPEN
    /// mutation. A local epoch pin validated only after OPEN is too late.
    pub(super) async fn reconnect_peer(&self, operation: &str) -> io::Result<Transport> {
        let transport = connect_transport(&self.endpoint, &self.birth).await?;
        transport.bind_epoch(&self.epoch)?;
        Self::establish(&transport, operation).await?;
        Ok(transport)
    }

    pub(super) fn epoch(&self) -> &str {
        &self.epoch
    }
}

/// Correlation captured immediately after the original OPEN was validated.
/// This owns no native Scope, parent Use, source, or replacement member.
pub(super) struct Binding {
    peer: PeerIdentity,
    operation: String,
    member: String,
    body: OriginalBody,
}

impl std::fmt::Debug for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeWorkBinding(<private>)")
    }
}

impl Binding {
    pub(super) fn capture(
        original: &Transport,
        operation: &str,
        member: &str,
        body: OriginalBody,
    ) -> io::Result<Self> {
        if !valid_hex(operation, 32) || !valid_hex(member, 32) {
            return Err(unavailable());
        }
        body.fields()?;
        Ok(Self {
            peer: PeerIdentity::capture(original)?,
            operation: operation.into(),
            member: member.into(),
            body,
        })
    }

    fn check_reply(&self, body: &str, transport: &Transport) -> io::Result<()> {
        // Transport.call has already joined the actual request nonce and the
        // pinned native epoch. Parse the exact role; optional role fields would
        // let one OPEN body masquerade as another.
        macro_rules! common {
            ($frame:expr, $kind:literal) => {{
                let frame = $frame;
                frame.validate(
                    &frame.request,
                    &self.operation,
                    &transport.sid,
                    &self.peer.birth,
                    Some(self.peer.epoch()),
                )?;
                if frame.member != self.member || frame.state != "bound" || frame.open_kind != $kind
                {
                    return Err(unavailable());
                }
                frame
            }};
        }
        match &self.body {
            OriginalBody::Ordinary => {
                common!(
                    serde_json::from_str::<OrdinaryBound>(body).map_err(|_| unavailable())?,
                    "ordinary"
                );
            }
            OriginalBody::Derived {
                use_member,
                use_operation,
                capability,
            } => {
                let frame = common!(
                    serde_json::from_str::<DerivedBound>(body).map_err(|_| unavailable())?,
                    "derived"
                );
                if &frame.use_member != use_member
                    || &frame.use_operation != use_operation
                    || &frame.capability != capability
                {
                    return Err(unavailable());
                }
            }
            OriginalBody::Target {
                assignment,
                context,
            } => {
                let frame = common!(
                    serde_json::from_str::<TargetBound>(body).map_err(|_| unavailable())?,
                    "target"
                );
                if &frame.assignment != assignment || &frame.context != context {
                    return Err(unavailable());
                }
            }
        }
        Ok(())
    }

    /// Return only the authenticated replacement channel. The existing member
    /// actor installs it after serializing against its old work; it must retain
    /// that same member's state and identity.
    pub(super) async fn reconnect(&self) -> io::Result<Arc<Transport>> {
        let hello = format!("{:016x}{:016x}", core::connection_request_prefix(), core::connection_request_prefix());
        let transport = self.peer.reconnect_peer(&hello).await?;
        let result = async {
            let fields = format!(" member={}{}", self.member, self.body.fields()?);
            let reply = transport
                .call(
                    "member-work-rebind",
                    &self.operation,
                    &fields,
                    Some(&self.member),
                )
                .await?;
            self.check_reply(&reply, &transport)
        }
        .await;
        if let Err(error) = result {
            transport.conn.close();
            return Err(error);
        }
        Ok(Arc::new(transport))
    }
}

#[cfg(test)]
#[path = "source_work_rebind_tests.rs"]
mod tests;
