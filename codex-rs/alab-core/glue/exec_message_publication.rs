// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Preserve delivery identity without replaying publication callbacks.
use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", deny_unknown_fields)]
pub(crate) enum ExecPublication {
    Consumed,
    Pending {
        sender: ThreadId,
        scope_id: uuid::Uuid,
    },
}

impl MessageDelivery {
    pub(crate) fn capture_exec(&self) -> Result<(usize, ExecPublication), String> {
        let publication = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let saved = match publication.as_ref() {
            None => ExecPublication::Consumed,
            Some(publication) => {
                // Contributors may retain arbitrary ExtensionData at publication.
                // A host must provide a restoration protocol before that data can
                // cross exec; never turn a live delivery into historical mail.
                if !publication.observers.is_empty() {
                    return Err("message publication has nonportable lifecycle observers".into());
                }
                ExecPublication::Pending {
                    sender: publication.input.sender,
                    scope_id: uuid::Uuid::parse_str(publication.input.data.level_id())
                        .map_err(|_| "message publication has a nonportable scope identity")?,
                }
            }
        };
        Ok((Arc::as_ptr(&self.0) as usize, saved))
    }

    pub(crate) fn restore_exec(saved: ExecPublication) -> Self {
        let publication = match saved {
            ExecPublication::Consumed => None,
            ExecPublication::Pending { sender, scope_id } => Some(MessagePublication {
                input: MessageLifecycleInput {
                    sender,
                    data: ExtensionData::new(scope_id.to_string()),
                },
                observers: Box::default(),
            }),
        };
        Self(Arc::new(Mutex::new(publication)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    #[derive(Default)]
    struct Observer(AtomicUsize);
    impl MessageLifecycleContributor for Observer {
        fn on_publish(&self, _: &MessageLifecycleInput) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn on_consumed(&self, _: &MessageLifecycleInput, _: ThreadId) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn observer_state_is_not_silently_discarded_or_republished() {
        let observer = Arc::new(Observer::default());
        let observers: Vec<Arc<dyn MessageLifecycleContributor>> = vec![observer.clone()];
        let delivery = MessageDelivery::publish(ThreadId::new(), &observers);
        assert_eq!(observer.0.load(Ordering::SeqCst), 1);
        assert!(delivery.capture_exec().is_err());
        assert_eq!(observer.0.load(Ordering::SeqCst), 1);
        delivery.consumed(ThreadId::new());
        assert_eq!(observer.0.load(Ordering::SeqCst), 2);
        assert!(matches!(
            delivery.capture_exec().unwrap().1,
            ExecPublication::Consumed
        ));
    }
    #[test]
    fn restored_delivery_keeps_its_scope_identity_and_consumes_all_its_clones_once() {
        let original = MessageDelivery::publish(ThreadId::new(), &[]);
        let (_, saved) = original.capture_exec().unwrap();
        let before = serde_json::to_value(&saved).unwrap();
        let restored =
            MessageDelivery::restore_exec(serde_json::from_value(before.clone()).unwrap());
        assert_eq!(
            serde_json::to_value(restored.capture_exec().unwrap().1).unwrap(),
            before
        );
        let shared = restored.clone();
        restored.consumed(ThreadId::new());
        assert!(matches!(
            shared.capture_exec().unwrap().1,
            ExecPublication::Consumed
        ));
        assert!(matches!(
            original.capture_exec().unwrap().1,
            ExecPublication::Pending { .. }
        ));
    }
}
