// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::RefreshedModelCatalog;
use super::RejectedModel;
use super::StepContext;
use crate::guardian::ParentResponse;
use codex_api::ResponseEvent;
use codex_api::ResponseId;
use codex_extension_api::ExtensionData;
use codex_extension_api::ModelResponseInterceptor;
use codex_extension_api::ModelResponseStream;
use futures::StreamExt;
use std::sync::Arc;

// On this release line the model client constructs its stream before returning it.
// Only the first stream opened by the owning request can consume this observer.
tokio::task_local! { static OPENING: std::cell::RefCell<Option<Observer>>; }

pub(super) async fn open<T>(
    send: impl Future<Output = T>,
    step: &StepContext,
    owner: Arc<RefreshedModelCatalog>,
    replay: bool,
) -> T {
    let observer = Observer {
        step: Arc::clone(&step.extension_data),
        turn: Arc::clone(&step.turn.extension_data),
        owner,
        model: step.settings.model_info.slug.clone(),
        replay,
    };
    OPENING
        .scope(std::cell::RefCell::new(Some(observer)), send)
        .await
}

impl StepContext {
    pub(crate) fn alab_observe_original_stream(
        stream: ModelResponseStream,
        original: bool,
    ) -> ModelResponseStream {
        if let Ok(Some(mut observer)) = OPENING.try_with(|opening| opening.take()) {
            observer.replay &= original;
            Box::new(observer).intercept(stream)
        } else {
            stream
        }
    }
}

struct Observer {
    step: Arc<ExtensionData>,
    turn: Arc<ExtensionData>,
    owner: Arc<RefreshedModelCatalog>,
    model: String,
    replay: bool,
}

struct Created {
    id: String,
    previous_id: Option<Arc<ResponseId>>,
    previous_parent: Option<Arc<ParentResponse>>,
}

impl ModelResponseInterceptor for Observer {
    fn intercept(mut self: Box<Self>, stream: ModelResponseStream) -> ModelResponseStream {
        let mut created = None;
        stream
            .inspect(move |event| match event {
                Ok(ResponseEvent::Created {
                    response_id: Some(id),
                }) if self.replay => {
                    created = Some(Created {
                        id: id.clone(),
                        previous_id: self.turn.get(),
                        previous_parent: self.turn.get(),
                    });
                }
                Ok(ResponseEvent::Created { .. } | ResponseEvent::RateLimits(_)) => {}
                Ok(ResponseEvent::ServerModel(model)) if model == &self.model => {}
                Ok(_) => self.replay = false,
                Err(error) => {
                    let Some(before_output) = error.alab_streamed_model_retirement() else {
                        return;
                    };
                    let replay = before_output && self.replay;
                    if replay && let Some(created) = created.take() {
                        self.step.insert(created);
                    }
                    self.step.insert(RejectedModel {
                        owner: Arc::clone(&self.owner),
                        replay,
                    });
                }
            })
            .boxed()
    }
}

pub(super) fn restore_speculative_response(step: &StepContext) {
    let Some(created) = step.extension_data.remove::<Created>() else {
        return;
    };
    let turn = &step.turn.extension_data;
    // response.created updates the ordinary turn's review routing before its first item.
    // Restore the original route only if this rejected request actually installed its own ID.
    if turn
        .get::<ResponseId>()
        .is_some_and(|id| id.0 == created.id)
    {
        if let Some(id) = &created.previous_id {
            turn.insert_arc(Arc::clone(id));
        } else {
            turn.remove::<ResponseId>();
        }
    }
    if turn
        .get::<ParentResponse>()
        .is_some_and(|parent| parent.id == created.id)
    {
        if let Some(parent) = &created.previous_parent {
            turn.insert_arc(Arc::clone(parent));
        } else {
            turn.remove::<ParentResponse>();
        }
    }
}

#[cfg(test)]
#[path = "model_retirement_stream_tests.rs"]
pub(super) mod tests;
