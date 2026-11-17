// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Frontend observations on the exact original Native controller connection.
//! A receipt is historical correlation, not a UI witness, SDK/model permission
//! or proof that the current input frontier still matches. The caller owns the
//! genuine primary replay/editor/input observation before acknowledge_ui.
use super::Controller;
use crate::accounts::Session;
use crate::source_member::Transport;
use codex_alab_core::managed_auth::unavailable;
use codex_alab_core::managed_auth::valid_hex;
use serde::Deserialize;
use std::io;
use std::sync::Arc;

struct Context {
    transport: Arc<Transport>,
    member: String,
    controller_operation: String,
    session: Session,
}
impl Context {
    fn capture(controller: &Controller) -> Self {
        Self {
            transport: controller.transport.clone(),
            member: controller.member.clone(),
            controller_operation: controller.operation.clone(),
            session: controller.session.clone(),
        }
    }
    fn fields(&self, service: &str, go: Option<&str>, frontier: Option<&str>) -> String {
        let mut fields = format!(
            " member={} controller_operation={} service={service}",
            self.member, self.controller_operation
        );
        if let Some(go) = go {
            fields.push_str(&format!(" go={go}"));
        }
        if let Some(frontier) = frontier {
            fields.push_str(&format!(" frontier={frontier}"));
        }
        fields
    }
}

// Separate closed envelopes deliberately reject go:null (and every go key) on
// the initial route, and require a real string go on the replacement route.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitialResponse {
    schema: u32,
    request: String,
    operation: String,
    member: String,
    session: Session,
    service: String,
    state: String,
    frontier: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplacementResponse {
    schema: u32,
    request: String,
    operation: String,
    member: String,
    session: Session,
    service: String,
    go: String,
    state: String,
    frontier: String,
}
enum Response {
    Initial(InitialResponse),
    Replacement(ReplacementResponse),
}
struct Fields<'a> {
    schema: u32,
    request: &'a str,
    operation: &'a str,
    member: &'a str,
    session: &'a Session,
    service: &'a str,
    go: Option<&'a str>,
    state: &'a str,
    frontier: &'a str,
}
struct Expected<'a> {
    operation: &'a str,
    service: &'a str,
    go: Option<&'a str>,
    state: &'a str,
    frontier: Option<&'a str>,
}
impl Response {
    fn decode(body: &str, replacement: bool) -> io::Result<Self> {
        if replacement {
            serde_json::from_str(body)
                .map(Self::Replacement)
                .map_err(|_| unavailable())
        } else {
            serde_json::from_str(body)
                .map(Self::Initial)
                .map_err(|_| unavailable())
        }
    }
    fn checked(body: &str, context: &Context, expected: Expected<'_>) -> io::Result<Self> {
        let decoded = Self::decode(body, expected.go.is_some()).and_then(|native| {
            native.validate(context, expected)?;
            Ok(native)
        });
        // Transport.call checks the generic authenticated envelope. A closed
        // action-specific decode/correlation failure is equally uncertain.
        if decoded.is_err() {
            context.transport.release();
        }
        decoded
    }
    fn fields(&self) -> Fields<'_> {
        match self {
            Self::Initial(frame) => Fields {
                schema: frame.schema,
                request: &frame.request,
                operation: &frame.operation,
                member: &frame.member,
                session: &frame.session,
                service: &frame.service,
                go: None,
                state: &frame.state,
                frontier: &frame.frontier,
            },
            Self::Replacement(frame) => Fields {
                schema: frame.schema,
                request: &frame.request,
                operation: &frame.operation,
                member: &frame.member,
                session: &frame.session,
                service: &frame.service,
                go: Some(&frame.go),
                state: &frame.state,
                frontier: &frame.frontier,
            },
        }
    }
    fn validate(&self, context: &Context, expected: Expected<'_>) -> io::Result<()> {
        let frame = self.fields();
        // Transport.call already validates the exact generated request, host
        // epoch and real process identity. Preserve the original controller's
        // exact session/member and the action-specific closed schema here too.
        if frame.schema != 1
            || !valid_hex(frame.request, 32)
            || frame.operation != expected.operation
            || frame.member != context.member
            || frame.session != &context.session
            || frame.service != expected.service
            || !valid_hex(frame.service, 32)
            || frame.go != expected.go
            || frame.go.is_some_and(|go| !valid_hex(go, 32))
            || frame.state != expected.state
            || !valid_hex(frame.frontier, 32)
            || expected
                .frontier
                .is_some_and(|frontier| frame.frontier != frontier)
        {
            return Err(unavailable());
        }
        Ok(())
    }
}
fn operation() -> String {
    format!(
        "{:016x}{:016x}",
        codex_alab_core::native_source::connection_request_prefix(),
        codex_alab_core::native_source::connection_request_prefix()
    )
}
struct Receipt {
    context: Context,
    native: Response,
    ui_operation: String,
    app_operation: String,
}

/// Actual Native begin reply retained with its same controller transport.
/// Its identifiers only select the original operation; they cannot grant Go.
#[derive(Clone)]
pub struct FrontendFrontier(Arc<Receipt>);
impl std::fmt::Debug for FrontendFrontier {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("FrontendFrontier(<native receipt; selection data only>)")
    }
}
impl Controller {
    /// Ask the actual current foreground/F1 owner. None selects only the real
    /// original-birth route; Some(go) selects the genuine replacement route.
    /// An unanswered call closes the original transport and remains uncertain.
    /// Never reconnect, resend, create a new controller, or fall back here.
    pub async fn begin_retained_frontier(
        &self,
        service: &str,
        go: Option<&str>,
    ) -> io::Result<FrontendFrontier> {
        if !valid_hex(service, 32) || go.is_some_and(|go| !valid_hex(go, 32)) {
            return Err(unavailable());
        }
        let context = Context::capture(self);
        let operation = operation();
        let action = if go.is_some() {
            "controller-retained-frontier-begin"
        } else {
            "controller-retained-initial-begin"
        };
        let body = context
            .transport
            .call(
                action,
                &operation,
                &context.fields(service, go, None),
                Some(&context.member),
            )
            .await?;
        let native = Response::checked(
            &body,
            &context,
            Expected {
                operation: &operation,
                service,
                go,
                state: if go.is_some() {
                    "frontier"
                } else {
                    "initial-frontier"
                },
                frontier: None,
            },
        )?;
        Ok(FrontendFrontier(Arc::new(Receipt {
            context,
            native,
            ui_operation: self::operation(),
            app_operation: self::operation(),
        })))
    }
}
impl FrontendFrontier {
    /// Exact resource correlation only; Native still decides every ACK/currentness check.
    pub fn is_for_controller(&self, controller: &Controller) -> bool {
        Arc::ptr_eq(&self.0.context.transport, &controller.transport)
            && self.0.context.member == controller.member
            && self.0.context.controller_operation == controller.operation
            && self.0.context.session == controller.session
    }
    pub fn service_data(&self) -> &str {
        self.0.native.fields().service
    }
    pub fn go_data(&self) -> Option<&str> {
        self.0.native.fields().go
    }
    pub fn frontier_data(&self) -> &str {
        self.0.native.fields().frontier
    }

    /// Call only after the genuine TUI primary replay/editor/input state has
    /// been observed by its actual owner. This method does not manufacture that
    /// observation. Native independently checks the current physical/runtime
    /// frontier. An unanswered call closes the same original transport; its
    /// uncertain result never permits reconnect/resend or implies no ACK.
    pub async fn acknowledge_ui(&self) -> io::Result<FrontendUiAck> {
        let context = &self.0.context;
        let original = self.0.native.fields();
        let action = if original.go.is_some() {
            "controller-retained-ui-ack"
        } else {
            "controller-retained-initial-ui-ack"
        };
        let fields = context.fields(original.service, original.go, Some(original.frontier));
        let body = context
            .transport
            .call(action, &self.0.ui_operation, &fields, Some(&context.member))
            .await?;
        let native = Response::checked(
            &body,
            context,
            Expected {
                operation: &self.0.ui_operation,
                service: original.service,
                go: original.go,
                state: if original.go.is_some() {
                    "activated"
                } else {
                    "initial-activated"
                },
                frontier: Some(original.frontier),
            },
        )?;
        Ok(FrontendUiAck {
            frontier: self.clone(),
            native: Arc::new(native),
        })
    }
}

/// Opaque historical Native UI-ACK reply. No public constructor, serializer,
/// boolean conversion or model-admission API exists on this receipt.
#[derive(Clone)]
pub struct FrontendUiAck {
    frontier: FrontendFrontier,
    native: Arc<Response>,
}
impl std::fmt::Debug for FrontendUiAck {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("FrontendUiAck(<native receipt; requires current activation>)")
    }
}
impl FrontendUiAck {
    pub fn service_data(&self) -> &str {
        self.native.fields().service
    }
    pub fn go_data(&self) -> Option<&str> {
        self.native.fields().go
    }
    pub fn frontier_data(&self) -> &str {
        self.native.fields().frontier
    }
    pub fn same_frontier(&self, frontier: &FrontendFrontier) -> bool {
        Arc::ptr_eq(&self.frontier.0, &frontier.0)
    }
}

#[cfg(test)]
#[path = "frontend_retained_frontend_frontier_tests.rs"]
mod tests;

#[path = "frontend_retained_app_owner.rs"]
mod app_owner;
pub use app_owner::AppFrontendOwner;

#[path = "frontend_source_selection_ui.rs"]
mod source_selection;
pub use source_selection::FrontendSelectionAppOwner;
pub use source_selection::FrontendSelectionFrontier;
pub use source_selection::FrontendSelectionUiAck;
