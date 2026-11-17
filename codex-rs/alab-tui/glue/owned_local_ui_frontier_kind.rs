// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Distinct actual Native/Source routes share the original UI replay consumer.
//! Matching identifiers are correlation DATA. Every UI ACK, backend observation
//! and App promotion below remains a real operation on the original resources.
use super::*;
use codex_alab_tui::frontend_control::Controller;
use codex_alab_tui::frontend_control::FrontendSelectionFrontier;
use codex_app_server_client::source_selection;

#[derive(Clone)]
pub(crate) enum Native {
    Runtime(FrontendFrontier),
    Source(FrontendSelectionFrontier),
}

/// The genuine result object returned by Source's final committed operation.
/// Arc retains its private construction; neither status nor receipt DATA can
/// recreate one. This object authorizes Source endpoint bookkeeping only.
#[derive(Clone)]
pub(crate) enum Activation {
    Runtime(Arc<retained_frontier::Activation>),
    Source(Arc<source_selection::Activation>),
}
impl Activation {
    pub(crate) fn side_view_data(&self) -> Option<codex_app_server_client::SideView> {
        match self {
            Self::Source(actual) => actual.side_view_data(),
            Self::Runtime(_) => None,
        }
    }
    pub(crate) fn matches(&self, receipt: &Receipt) -> bool {
        let actual = match (self, receipt) {
            (Self::Runtime(actual), Receipt::Runtime(_)) => {
                Receipt::Runtime(actual.receipt().clone())
            }
            (Self::Source(actual), Receipt::Source(_)) => Receipt::Source(actual.receipt().clone()),
            _ => return false,
        };
        actual.frontier_data() == receipt.frontier_data()
            && actual.snapshot_data() == receipt.snapshot_data()
            && actual.thread_id_data() == receipt.thread_id_data()
    }
    pub(crate) async fn acknowledge_closed_callbacks(
        &self,
        capture: &codex_app_server_client::retained_callbacks::CallbackCapture,
        handle: &AppServerRequestHandle,
        original: codex_app_server_protocol::ClientRequest,
        observation: &codex_app_server_client::retained_callbacks::ClosedCallbackObservation,
        ids: Vec<codex_app_server_protocol::RequestId>,
    ) -> Result<codex_app_server_client::retained_callbacks::CallbackRetirement> {
        Ok(match self {
            Self::Runtime(actual) => {
                capture
                    .acknowledge_closed_runtime(handle, original, observation, actual, ids)
                    .await?
            }
            Self::Source(actual) => {
                capture
                    .acknowledge_closed_source(handle, original, observation, actual, ids)
                    .await?
            }
        })
    }
    pub(crate) async fn acknowledge_callbacks(
        &self,
        capture: &codex_app_server_client::retained_callbacks::CallbackCapture,
        handle: &AppServerRequestHandle,
        original: codex_app_server_protocol::ClientRequest,
        observation: &codex_app_server_client::retained_callbacks::CallbackObservation,
        ids: Vec<codex_app_server_protocol::RequestId>,
    ) -> Result<codex_app_server_client::retained_callbacks::CallbackRetirement> {
        Ok(match self {
            Self::Runtime(actual) => {
                capture
                    .acknowledge_runtime(handle, original, observation, actual, ids)
                    .await?
            }
            Self::Source(actual) => {
                capture
                    .acknowledge_source(handle, original, observation, actual, ids)
                    .await?
            }
        })
    }
}

#[derive(Clone)]
pub(crate) enum Receipt {
    Runtime(retained_frontier::Receipt),
    Source(source_selection::Receipt),
}

impl Native {
    pub(crate) fn frontier_data(&self) -> &str {
        match self {
            Self::Runtime(actual) => actual.frontier_data(),
            Self::Source(actual) => actual.frontier_data(),
        }
    }

    pub(crate) fn service_data(&self) -> &str {
        match self {
            Self::Runtime(actual) => actual.service_data(),
            Self::Source(actual) => actual.service_data(),
        }
    }

    pub(crate) fn is_for_controller(&self, controller: &Controller) -> bool {
        match self {
            Self::Runtime(actual) => actual.is_for_controller(controller),
            Self::Source(actual) => actual.is_for_controller(controller),
        }
    }

    pub(crate) fn matches(&self, receipt: &Receipt) -> bool {
        matches!(
            (self, receipt),
            (Self::Runtime(_), Receipt::Runtime(_)) | (Self::Source(_), Receipt::Source(_))
        ) && self.frontier_data() == receipt.frontier_data()
    }

    /// Called only while the original App/Tui owns its completed replay, real
    /// successful draw and actual stopped/joined input capture. This function
    /// supplies none of those witnesses and returns no reusable grant.
    pub(crate) async fn commit(
        &self,
        receipt: &Receipt,
        tenure: &crate::owned_local_frontend::FrontendTenure,
        handle: &AppServerRequestHandle,
    ) -> Result<Activation> {
        if !self.matches(receipt)
            || !self.is_for_controller(tenure.controller())
            || self.service_data() != tenure.target.service_data()
        {
            bail!("UI commit changed its actual route or physical delivery");
        }
        match (self, receipt) {
            (Self::Runtime(native), Receipt::Runtime(receipt)) => {
                let ui = native.acknowledge_ui().await?;
                let observed = retained_frontier::observe(handle, receipt).await?;
                if native.go_data().is_some() {
                    let owner = native.promote_in_app(&ui).await?;
                    let retained = tenure.retain_app_owner(native, owner)?;
                    retained.check_current().await?;
                }
                let completed = retained_frontier::activate(handle, observed.receipt()).await?;
                Ok(Activation::Runtime(Arc::new(completed)))
            }
            (Self::Source(native), Receipt::Source(receipt)) => {
                let ui = native.acknowledge_ui().await?;
                let observed = source_selection::observe(handle, receipt).await?;
                if native.go_data().is_some() {
                    let owner = native.promote_in_app(&ui).await?;
                    // The same physical tenure owns the actual new App keeper
                    // BEFORE any async current or final backend observation.
                    let retained = tenure.retain_source_app_owner(native, owner)?;
                    retained.check_current().await?;
                }
                let completed = source_selection::activate(handle, observed.receipt()).await?;
                Ok(Activation::Source(Arc::new(completed)))
            }
            _ => bail!("Runtime and Source selection receipts cannot substitute"),
        }
    }
}

impl Receipt {
    pub(crate) fn side_display_boundary(&self) -> Option<&source_selection::SideDisplayBoundary> {
        match self {
            Self::Runtime(_) => None,
            Self::Source(actual) => actual.side_display_boundary(),
        }
    }
    pub(crate) fn frontier_data(&self) -> &str {
        match self {
            Self::Runtime(actual) => actual.frontier_data(),
            Self::Source(actual) => actual.frontier_data(),
        }
    }

    pub(crate) fn snapshot_data(&self) -> &str {
        match self {
            Self::Runtime(actual) => actual.snapshot_data(),
            Self::Source(actual) => actual.snapshot_data(),
        }
    }

    pub(crate) fn thread_id_data(&self) -> &str {
        match self {
            Self::Runtime(actual) => actual.thread_id_data(),
            Self::Source(actual) => actual.thread_id_data(),
        }
    }

    pub(crate) fn task_tools_available(&self) -> bool {
        match self {
            Self::Runtime(actual) => actual.task_tools_available(),
            Self::Source(actual) => actual.task_tools_available(),
        }
    }

    pub(crate) fn is_primary_presentation(&self) -> bool {
        match self {
            Self::Runtime(_) => true,
            Self::Source(actual) => actual.is_primary_presentation(),
        }
    }
}
