// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Genuine frontend S2 begin/UI/App responses on the original controller.
//! Prior FrontendFrontier carries only the actual constructor selection/context;
//! every new action independently checks Native's current constructor/A/runtime.
//! Actual Source UI replay/draw/input ownership must precede acknowledge_ui.
use super::*;
macro_rules! selection_reply {
    ($name:ident { $($key:ident : $ty:ty),* $(,)? }) => {
        #[derive(Deserialize)] #[serde(deny_unknown_fields)]
        struct $name { schema:u32, request:String, operation:String, member:String, session:Session,
            service:String, source_operation:String, selection:String, frontier:String, state:String,
            $($key:$ty),* }
    };
}
selection_reply!(Initial {});
selection_reply!(Replacement { go: String });
selection_reply!(Promotion {
    go: String,
    app_owner: String
});
enum Reply {
    Initial(Initial),
    Replacement(Replacement),
    Promotion(Promotion),
}
struct Fields<'a> {
    schema: u32,
    request: &'a str,
    operation: &'a str,
    member: &'a str,
    session: &'a Session,
    service: &'a str,
    source: &'a str,
    selection: &'a str,
    frontier: &'a str,
    state: &'a str,
    go: Option<&'a str>,
    owner: Option<&'a str>,
}
impl Reply {
    fn decode(body: &str, go: Option<&str>, promotion: bool) -> io::Result<Self> {
        match (go.is_some(), promotion) {
            (false, false) => serde_json::from_str(body).map(Self::Initial),
            (true, false) => serde_json::from_str(body).map(Self::Replacement),
            (true, true) => serde_json::from_str(body).map(Self::Promotion),
            (false, true) => return Err(unavailable()),
        }
        .map_err(|_| unavailable())
    }
    fn fields(&self) -> Fields<'_> {
        macro_rules! fields {
            ($value:ident,$go:expr,$owner:expr) => {
                Fields {
                    schema: $value.schema,
                    request: &$value.request,
                    operation: &$value.operation,
                    member: &$value.member,
                    session: &$value.session,
                    service: &$value.service,
                    source: &$value.source_operation,
                    selection: &$value.selection,
                    frontier: &$value.frontier,
                    state: &$value.state,
                    go: $go,
                    owner: $owner,
                }
            };
        }
        match self {
            Self::Initial(value) => fields!(value, None, None),
            Self::Replacement(value) => fields!(value, Some(&value.go), None),
            Self::Promotion(value) => fields!(value, Some(&value.go), Some(&value.app_owner)),
        }
    }
}
struct Receipt {
    base: FrontendFrontier,
    native: Reply,
    ui_operation: String,
    app_operation: String,
}
#[derive(Clone)]
pub struct FrontendSelectionFrontier(Arc<Receipt>);
#[derive(Clone)]
pub struct FrontendSelectionUiAck {
    frontier: FrontendSelectionFrontier,
    native: Arc<Reply>,
}
#[derive(Clone)]
pub struct FrontendSelectionAppOwner {
    frontier: FrontendSelectionFrontier,
    native: Arc<Reply>,
}
impl FrontendFrontier {
    /// The genuine prior Native response supplies the original controller and
    /// constructor branch. IDs select the real accepted Source reservation;
    /// none of them or this historical base receipt supplies current admission.
    pub async fn begin_source_selection(
        &self,
        source: &str,
        selection: &str,
    ) -> io::Result<FrontendSelectionFrontier> {
        if !valid_hex(source, 32) || !valid_hex(selection, 32) {
            return Err(unavailable());
        }
        let op = operation();
        let original = self.0.native.fields();
        let context = &self.0.context;
        let fields = format!(
            "{} source_operation={source} selection={selection}",
            context.fields(original.service, original.go, None)
        );
        let body = context
            .transport
            .call(
                "controller-source-selection-begin",
                &op,
                &fields,
                Some(&context.member),
            )
            .await?;
        let checked: io::Result<Reply> = (|| {
            let native = Reply::decode(&body, original.go, false)?;
            validate(
                &native,
                context,
                Expected {
                    operation: &op,
                    service: original.service,
                    go: original.go,
                    source,
                    selection,
                    frontier: None,
                    state: "selection-frontier",
                },
            )?;
            Ok(native)
        })();
        if checked.is_err() {
            context.transport.release();
        }
        Ok(FrontendSelectionFrontier(Arc::new(Receipt {
            base: self.clone(),
            native: checked?,
            ui_operation: operation(),
            app_operation: operation(),
        })))
    }
}
struct Expected<'a> {
    operation: &'a str,
    service: &'a str,
    go: Option<&'a str>,
    source: &'a str,
    selection: &'a str,
    frontier: Option<&'a str>,
    state: &'a str,
}
fn validate(native: &Reply, context: &Context, expected: Expected<'_>) -> io::Result<()> {
    let Expected {
        operation,
        service,
        go,
        source,
        selection,
        frontier,
        state,
    } = expected;
    let actual = native.fields();
    if actual.schema != 1
        || !valid_hex(actual.request, 32)
        || actual.operation != operation
        || actual.member != context.member
        || actual.session != &context.session
        || actual.service != service
        || actual.go != go
        || actual.source != source
        || actual.selection != selection
        || !valid_hex(actual.frontier, 32)
        || frontier.is_some_and(|id| actual.frontier != id)
        || actual.state != state
        || actual.owner.is_some_and(|id| !valid_hex(id, 32))
    {
        return Err(unavailable());
    }
    Ok(())
}
impl FrontendSelectionFrontier {
    pub fn service_data(&self) -> &str {
        self.0.native.fields().service
    }
    pub fn go_data(&self) -> Option<&str> {
        self.0.native.fields().go
    }
    pub fn source_operation_data(&self) -> &str {
        self.0.native.fields().source
    }
    pub fn selection_data(&self) -> &str {
        self.0.native.fields().selection
    }
    pub fn frontier_data(&self) -> &str {
        self.0.native.fields().frontier
    }
    pub fn is_for_controller(&self, controller: &Controller) -> bool {
        self.0.base.is_for_controller(controller)
    }
    fn fields(&self) -> String {
        let native = self.0.native.fields();
        format!(
            "{} source_operation={} selection={}",
            self.0
                .base
                .0
                .context
                .fields(native.service, native.go, Some(native.frontier)),
            native.source,
            native.selection
        )
    }
    /// Actual Source App/Tui ownership, ordered replay and genuine draw/input
    /// fence must be retained across this call. No idle/composer bool suffices.
    pub async fn acknowledge_ui(&self) -> io::Result<FrontendSelectionUiAck> {
        let original = self.0.native.fields();
        let context = &self.0.base.0.context;
        let body = context
            .transport
            .call(
                "controller-source-selection-ui-ack",
                &self.0.ui_operation,
                &self.fields(),
                Some(&context.member),
            )
            .await?;
        let checked: io::Result<Reply> = (|| {
            let native = Reply::decode(&body, original.go, false)?;
            validate(
                &native,
                context,
                Expected {
                    operation: &self.0.ui_operation,
                    service: original.service,
                    go: original.go,
                    source: original.source,
                    selection: original.selection,
                    frontier: Some(original.frontier),
                    state: "selection-activated",
                },
            )?;
            Ok(native)
        })();
        if checked.is_err() {
            context.transport.release();
        }
        Ok(FrontendSelectionUiAck {
            frontier: self.clone(),
            native: Arc::new(checked?),
        })
    }
    /// The backend must first observe the genuine Native S2 activation while
    /// its Source barriers remain closed. Native itself denies promotion until
    /// that actual observation and same-current-UI conjunction exist.
    pub async fn promote_in_app(
        &self,
        ui: &FrontendSelectionUiAck,
    ) -> io::Result<FrontendSelectionAppOwner> {
        if !ui.same_frontier(self) {
            return Err(unavailable());
        }
        let original = self.0.native.fields();
        if original.go.is_none() {
            return Err(unavailable());
        }
        let context = &self.0.base.0.context;
        let body = context
            .transport
            .call(
                "controller-source-selection-promote",
                &self.0.app_operation,
                &self.fields(),
                Some(&context.member),
            )
            .await?;
        let checked: io::Result<Reply> = (|| {
            let native = Reply::decode(&body, original.go, true)?;
            validate(
                &native,
                context,
                Expected {
                    operation: &self.0.app_operation,
                    service: original.service,
                    go: original.go,
                    source: original.source,
                    selection: original.selection,
                    frontier: Some(original.frontier),
                    state: "selection-app-current",
                },
            )?;
            Ok(native)
        })();
        if checked.is_err() {
            context.transport.release();
        }
        Ok(FrontendSelectionAppOwner {
            frontier: self.clone(),
            native: Arc::new(checked?),
        })
    }
}
impl FrontendSelectionUiAck {
    pub fn same_frontier(&self, actual: &FrontendSelectionFrontier) -> bool {
        Arc::ptr_eq(&self.frontier.0, &actual.0)
    }
    pub fn frontier_data(&self) -> &str {
        self.native.fields().frontier
    }
}
impl FrontendSelectionAppOwner {
    pub fn same_frontier(&self, actual: &FrontendSelectionFrontier) -> bool {
        Arc::ptr_eq(&self.frontier.0, &actual.0)
    }
    pub async fn check_current(&self) -> io::Result<()> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Current {
            schema: u32,
            request: String,
            operation: String,
            member: String,
            session: Session,
            state: String,
            app_owner: String,
        }
        let context = &self.frontier.0.base.0.context;
        let op = operation();
        let owner = self.native.fields().owner.ok_or_else(unavailable)?;
        let fields = format!(
            " member={} controller_operation={} app_owner={owner}",
            context.member, context.controller_operation
        );
        let body = context
            .transport
            .observe_retained(
                crate::source_member::RetainedQuery::AppCurrent,
                &op,
                &fields,
                &context.member,
            )
            .await
            .map_err(crate::source_member::ObservationError::into_io)?;
        let checked = (|| {
            let native: Current = serde_json::from_str(&body).map_err(|_| unavailable())?;
            if native.schema != 1
                || !valid_hex(&native.request, 32)
                || native.operation != op
                || native.member != context.member
                || native.session != context.session
                || native.state != "app-current"
                || native.app_owner != owner
            {
                return Err(unavailable());
            }
            Ok(())
        })();
        if checked.is_err() {
            context.transport.release();
        }
        checked
    }
}

impl std::fmt::Debug for FrontendSelectionFrontier {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("FrontendSelectionFrontier(<same actual resource; no Native grant>)")
    }
}

impl std::fmt::Debug for FrontendSelectionUiAck {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("FrontendSelectionUiAck(<same actual resource; no Native grant>)")
    }
}

impl std::fmt::Debug for FrontendSelectionAppOwner {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("FrontendSelectionAppOwner(<same actual resource; no Native grant>)")
    }
}
