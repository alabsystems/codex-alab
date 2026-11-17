// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Optional original Core-injection seam. Ordinary validation, image policy,
//! direct-input policy and original request parsing remain in TurnProcessor.
use super::*;
#[derive(Clone)]
pub(crate) struct OriginalInjection {
    actual: Arc<CodexThread>,
    prepared: codex_core::PreparedSideBoundary,
}
tokio::task_local! { static ORIGINAL: OriginalInjection; }
impl OriginalInjection {
    pub(crate) fn new(
        actual: Arc<CodexThread>,
        prepared: codex_core::PreparedSideBoundary,
    ) -> Self {
        Self { actual, prepared }
    }
    pub(crate) async fn run<T>(&self, original: impl std::future::Future<Output = T>) -> T {
        ORIGINAL.scope(self.clone(), original).await
    }
}
pub(super) async fn inject_original(
    actual: &Arc<CodexThread>,
    items: Vec<ResponseItem>,
) -> codex_protocol::error::Result<()> {
    match ORIGINAL.try_with(Clone::clone).ok() {
        None => actual.inject_response_items(items).await,
        Some(owner) => {
            // The original load_thread may race a logical-ID replacement. Its
            // actual resolved Arc must match BEFORE any original Core effects.
            if !Arc::ptr_eq(actual, &owner.actual) {
                return Err(codex_protocol::error::CodexErr::InvalidRequest(
                    "Side injection resolved another actual Core".into(),
                ));
            }
            owner.prepared.inject(actual, items).await
        }
    }
}
