// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Distinct same-original-A live cleanup and real configured/NoPrimary route.
//! Neither path opens the Goal Hold or establishes Core/effect/App authority.
use super::*;
fn same_original<T>(original: &Arc<T>, actual: &T) -> bool {
    std::ptr::eq(original.as_ref(), actual)
}
impl Binding {
    pub(super) async fn acknowledge_live_callbacks(
        &self,
        ticket: Option<&Arc<Ticket>>,
        context: &RequestContext,
        origin: &AcceptedOrigin,
        descriptor: &data::Descriptor,
        ids: &[RequestId],
    ) -> Result<data::Reply, JSONRPCErrorError> {
        let Some(ticket) = ticket else {
            return Ok(data::Reply::LiveAcknowledged {
                descriptor: descriptor.clone(),
                ids: ids.to_vec(),
                retired: true,
            });
        };
        // Current C2 cannot use a live C1 assertion to bypass projection/activation.
        if !same_original(&ticket.original, origin) {
            return Err(invalid_request(
                "live callback ACK changed actual original A",
            ));
        }
        let _serial = ticket
            .serial
            .acquire()
            .await
            .map_err(|_| internal_error("live callback lease unavailable"))?;
        let capture = ticket
            .prepared
            .observe_current_for(&self.processor.outgoing, &self.backend, context)
            .await
            .map_err(native_error)?;
        capture
            .acknowledge_for(&self.processor.outgoing, &self.backend, context, ids)
            .await
            .map_err(native_error)?;
        self.open()?;
        let retired = self.retire_callback_ids(ticket, descriptor, ids)?;
        Ok(data::Reply::LiveAcknowledged {
            descriptor: descriptor.clone(),
            ids: ids.to_vec(),
            retired,
        })
    }
    fn retire_callback_ids(
        &self,
        ticket: &Arc<Ticket>,
        descriptor: &data::Descriptor,
        ids: &[RequestId],
    ) -> Result<bool, JSONRPCErrorError> {
        let complete = {
            let mut cut = ticket.cut.lock().unwrap_or_else(PoisonError::into_inner);
            if cut.uncertain {
                return Err(invalid_request("callback owner remains unknown"));
            }
            for id in ids {
                if !cut.acknowledged.contains(id) {
                    cut.acknowledged.push(id.clone());
                }
            }
            cut.acknowledged.len() == descriptor.ids.len()
        };
        let removed = if complete {
            self.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .callbacks
                .book
                .retire(descriptor.ordinal, &descriptor.operation)
        } else {
            None
        };
        drop(removed);
        Ok(complete)
    }
    fn configured_cut(
        &self,
        d: &data::Descriptor,
        p: &data::ConfiguredProjection,
        content: data::Content,
    ) -> Result<data::ConfiguredObservation, JSONRPCErrorError> {
        let auth = {
            self.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .callbacks
                .auth
                .clone()
        };
        let proof = auth
            .sign("configured-observation", &(d, p, &content))
            .map_err(native_error)?;
        Ok(data::ConfiguredObservation {
            descriptor: d.clone(),
            projection: p.clone(),
            content,
            proof,
        })
    }
    pub(super) async fn observe_configured_callbacks(
        &self,
        ticket: Option<&Arc<Ticket>>,
        context: &RequestContext,
        origin: &AcceptedOrigin,
        descriptor: &data::Descriptor,
        projection: &data::ConfiguredProjection,
    ) -> Result<data::Reply, JSONRPCErrorError> {
        self.callback_configured_projection(projection, origin)
            .await?;
        let content = if let Some(ticket) = ticket {
            let capture = ticket
                .prepared
                .observe_current_for(&self.processor.outgoing, &self.backend, context)
                .await
                .map_err(native_error)?;
            let facts = endpoint_facts(
                capture
                    .facts_for(&self.processor.outgoing)
                    .await
                    .map_err(native_error)?,
            );
            if facts.iter().any(|fact| !closed_outcome(&fact.outcome)) {
                return Err(invalid_request(
                    "NoPrimary permits only OWN actually closed callback facts",
                ));
            }
            self.callback_configured_projection(projection, origin)
                .await?;
            data::Content::Current { facts }
        } else {
            data::Content::Retired {}
        };
        Ok(data::Reply::ObservedConfigured {
            observation: self.configured_cut(descriptor, projection, content)?,
        })
    }
    pub(super) async fn acknowledge_configured_callbacks(
        &self,
        ticket: Option<&Arc<Ticket>>,
        context: &RequestContext,
        origin: &AcceptedOrigin,
        descriptor: &data::Descriptor,
        observation: &data::ConfiguredObservation,
        ids: &[RequestId],
    ) -> Result<data::Reply, JSONRPCErrorError> {
        let auth = {
            self.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .callbacks
                .auth
                .clone()
        };
        if !observation.valid()
            || !auth
                .verify(
                    "configured-observation",
                    &(
                        &observation.descriptor,
                        &observation.projection,
                        &observation.content,
                    ),
                    &observation.proof,
                )
                .map_err(native_error)?
        {
            return Err(invalid_request(
                "configured callback cut is not genuine Source bookkeeping",
            ));
        }
        let Some(ticket) = ticket else {
            return Ok(data::Reply::LiveAcknowledged {
                descriptor: descriptor.clone(),
                ids: ids.to_vec(),
                retired: true,
            });
        };
        let _serial = ticket
            .serial
            .acquire()
            .await
            .map_err(|_| internal_error("configured callback lease unavailable"))?;
        let actual = self
            .callback_configured_projection(&observation.projection, origin)
            .await?;
        self.callback_configured_committed(&actual)?;
        let data::Content::Current { facts } = &observation.content else {
            return Err(invalid_request(
                "active NoPrimary callback needs its real closed facts",
            ));
        };
        if !ids.iter().all(|id| {
            facts
                .iter()
                .any(|fact| &fact.request_id == id && closed_outcome(&fact.outcome))
        }) {
            return Err(invalid_request("NoPrimary ACK lacks own closed facts"));
        }
        let capture = ticket
            .prepared
            .observe_current_for(&self.processor.outgoing, &self.backend, context)
            .await
            .map_err(native_error)?;
        capture
            .acknowledge_for(&self.processor.outgoing, &self.backend, context, ids)
            .await
            .map_err(native_error)?;
        let current = self
            .callback_configured_projection(&observation.projection, origin)
            .await?;
        self.callback_configured_committed(&current)?;
        let retired = self.retire_callback_ids(ticket, descriptor, ids)?;
        Ok(data::Reply::LiveAcknowledged {
            descriptor: descriptor.clone(),
            ids: ids.to_vec(),
            retired,
        })
    }
    pub(super) async fn observe_closed_callbacks(
        &self,
        ticket: Option<&Arc<Ticket>>,
        context: &RequestContext,
        origin: &AcceptedOrigin,
        descriptor: &data::Descriptor,
        projection: &data::Projection,
        ids: &[RequestId],
    ) -> Result<data::Reply, JSONRPCErrorError> {
        self.callback_projection(projection, origin).await?;
        let content = if let Some(ticket) = ticket {
            let capture = ticket
                .prepared
                .observe_current_for(&self.processor.outgoing, &self.backend, context)
                .await
                .map_err(native_error)?;
            let facts = endpoint_facts(
                capture
                    .facts_for(&self.processor.outgoing)
                    .await
                    .map_err(native_error)?,
            );
            let selected = select_closed(&facts, ids).map_err(invalid_request)?;
            // The current real Core proves this App frontier only. OWN endpoint thread
            // IDs remain their original DATA; this never claims that old Core is live.
            self.callback_projection(projection, origin).await?;
            data::Content::Current { facts: selected }
        } else {
            data::Content::Retired {}
        };
        let auth = {
            self.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .callbacks
                .auth
                .clone()
        };
        let proof = auth
            .sign(
                "closed-observation",
                &(descriptor, projection, ids, &content),
            )
            .map_err(native_error)?;
        Ok(data::Reply::ObservedClosed {
            observation: data::ClosedObservation {
                descriptor: descriptor.clone(),
                projection: projection.clone(),
                ids: ids.to_vec(),
                content,
                proof,
            },
        })
    }
    pub(super) async fn acknowledge_closed_callbacks(
        &self,
        ticket: Option<&Arc<Ticket>>,
        context: &RequestContext,
        origin: &AcceptedOrigin,
        descriptor: &data::Descriptor,
        observation: &data::ClosedObservation,
        ids: &[RequestId],
    ) -> Result<data::Reply, JSONRPCErrorError> {
        let auth = {
            self.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .callbacks
                .auth
                .clone()
        };
        if !observation.valid()
            || !auth
                .verify(
                    "closed-observation",
                    &(
                        &observation.descriptor,
                        &observation.projection,
                        &observation.ids,
                        &observation.content,
                    ),
                    &observation.proof,
                )
                .map_err(native_error)?
        {
            return Err(invalid_request(
                "closed callback cut is not genuine Source bookkeeping",
            ));
        }
        let Some(ticket) = ticket else {
            return Ok(data::Reply::LiveAcknowledged {
                descriptor: descriptor.clone(),
                ids: ids.to_vec(),
                retired: true,
            });
        };
        let _serial = ticket
            .serial
            .acquire()
            .await
            .map_err(|_| internal_error("closed callback lease unavailable"))?;
        let actual = self
            .callback_projection(&observation.projection, origin)
            .await?;
        self.callback_committed(&actual)?;
        let data::Content::Current { facts } = &observation.content else {
            return Err(invalid_request("active callback needs OWN closed facts"));
        };
        let _selected = select_closed(facts, ids).map_err(invalid_request)?;
        let capture = ticket
            .prepared
            .observe_current_for(&self.processor.outgoing, &self.backend, context)
            .await
            .map_err(native_error)?;
        capture
            .acknowledge_for(&self.processor.outgoing, &self.backend, context, ids)
            .await
            .map_err(native_error)?;
        let current = self
            .callback_projection(&observation.projection, origin)
            .await?;
        self.callback_committed(&current)?;
        let retired = self.retire_callback_ids(ticket, descriptor, ids)?;
        Ok(data::Reply::LiveAcknowledged {
            descriptor: descriptor.clone(),
            ids: ids.to_vec(),
            retired,
        })
    }
}
#[cfg(test)]
#[path = "retained_callback_ack_route_tests.rs"]
mod tests;

fn select_closed(facts: &[data::Fact], ids: &[RequestId]) -> Result<Vec<data::Fact>, &'static str> {
    ids.iter()
        .map(|id| {
            facts
                .iter()
                .find(|fact| &fact.request_id == id && closed_outcome(&fact.outcome))
                .cloned()
                .ok_or("requested callback has no OWN actual closed endpoint fact")
        })
        .collect()
}
