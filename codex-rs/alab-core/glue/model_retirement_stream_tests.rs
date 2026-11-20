// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;
use codex_api::ApiError;

struct ReplaceError;

impl ModelResponseInterceptor for ReplaceError {
    fn intercept(self: Box<Self>, stream: ModelResponseStream) -> ModelResponseStream {
        stream.map(|event| event.map_err(|_| retired())).boxed()
    }
}

fn retired() -> ApiError {
    ApiError::ModelNotFound {
        message: "model is retired".into(),
        status: None,
        alab_empty_stream: true,
    }
}

pub(in super::super) async fn exercise(session: &super::super::Session, step: &StepContext) {
    let prior = Arc::new(ResponseId("prior".into()));
    let parent = Arc::new(ParentResponse {
        id: "prior".into(),
        routing_affinity: None,
    });
    let source = &step.turn.extension_data;
    source.insert_arc(Arc::clone(&prior));
    source.insert_arc(Arc::clone(&parent));
    for (partial, transformed, raw_retirement, expected) in [
        (false, false, true, Some(true)),
        (true, false, true, Some(false)),
        (false, true, true, Some(false)),
        (false, true, false, None),
    ] {
        let mut interceptors: Vec<Box<dyn ModelResponseInterceptor>> = Vec::new();
        if transformed {
            interceptors.push(Box::new(ReplaceError));
        }
        let mut events = vec![Ok(ResponseEvent::Created {
            response_id: Some("rejected".into()),
        })];
        if partial {
            events.push(Ok(ResponseEvent::OutputTextDelta("shown".into())));
        }
        events.push(Err(if raw_retirement {
            retired()
        } else {
            ApiError::Stream("unrelated".into())
        }));
        let open = async {
            Ok(crate::model_request::intercept_stream(
                futures::stream::iter(events).boxed(),
                interceptors,
            ))
        };
        let mut stream = step
            .alab_observe_model_open(session, open, true, true)
            .await
            .unwrap();
        while let Some(event) = stream.next().await {
            match event {
                Ok(ResponseEvent::Created {
                    response_id: Some(id),
                }) => {
                    source.insert(ResponseId(id.clone()));
                    source.insert(ParentResponse {
                        id,
                        routing_affinity: None,
                    });
                }
                Ok(_) => {}
                Err(error) => assert!(matches!(error, ApiError::ModelNotFound { .. })),
            }
        }
        let observed = step.extension_data.remove::<RejectedModel>();
        assert_eq!(observed.map(|value| value.replay), expected);
        restore_speculative_response(step);
        if expected == Some(true) {
            assert!(Arc::ptr_eq(&source.get::<ResponseId>().unwrap(), &prior));
            assert!(Arc::ptr_eq(
                &source.get::<ParentResponse>().unwrap(),
                &parent
            ));
        } else {
            assert_eq!(source.get::<ResponseId>().unwrap().0, "rejected");
            assert_eq!(source.get::<ParentResponse>().unwrap().id, "rejected");
        }
    }
}
