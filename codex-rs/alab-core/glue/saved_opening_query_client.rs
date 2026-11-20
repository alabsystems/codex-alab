// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A replacement may inspect the backend's outcome for one original saved
//! chooser. This data owns neither a construction operation nor a subscription.
use super::*;
use codex_app_server_protocol::JSONRPCErrorError;
use metadata::preparation::SavedResumeOutcome;

impl SavedResumeOutcome {
    fn valid(&self) -> bool {
        !matches!(self, Self::Rejected { had_data: false, value, .. } if !value.is_null())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SavedResumeObservation {
    Resumed,
    Rejected(JSONRPCErrorError),
    Pending,
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
#[error("saved resume lookup was refused by the app-server overload queue")]
struct SavedResumeReadOverloaded;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Request", into = "Request")]
pub struct SavedResumeQuery {
    original: Arc<Prepare>,
    offer: Offer,
    child: String,
}

impl TryFrom<Request> for SavedResumeQuery {
    type Error = std::io::Error;
    fn try_from(request: Request) -> IoResult<Self> {
        if let Request::ReadSavedResume { original, offer } = request
            && let Some(child) = original.saved_resume_child(&offer)
        {
            let child = child.to_owned();
            Ok(Self {
                original,
                offer,
                child,
            })
        } else {
            Err(invalid("invalid saved resume lookup identity"))
        }
    }
}
impl From<SavedResumeQuery> for Request {
    fn from(query: SavedResumeQuery) -> Self {
        Self::ReadSavedResume {
            original: query.original,
            offer: query.offer,
        }
    }
}

impl std::fmt::Debug for SavedResumeQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SavedResumeQuery")
            .field("opening", &self.opening())
            .field("child", &self.child())
            .finish()
    }
}

impl ClientConstructionOperation {
    /// The actual saved chooser uses its operation UUID, in simple form, as
    /// the preparation identity before the full original request is available.
    pub fn for_saved_resume(
        selection: ClientConstructionSelection,
        opening: &str,
        child: &str,
    ) -> IoResult<Self> {
        let operation = Self::with_intent(
            ConstructionIntent::SavedResume {
                opening: opening.into(),
                child: child.into(),
                selection: Box::new(selection),
            },
            opening.into(),
        );
        if !operation
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .valid_saved_binding()
        {
            return Err(invalid("invalid original saved chooser identity"));
        }
        Ok(operation)
    }
    pub fn saved_resume_query(&self, child: &str) -> IoResult<SavedResumeQuery> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let query = state
            .saved_resume_query()?
            .ok_or_else(|| invalid("saved resume has no attempted original"))?;
        query.validate_child(child)?;
        Ok(query)
    }
}

impl ClientConstructionCheckpoint {
    pub fn saved_opening(&self) -> Option<(&str, &str)> {
        match &self.intent {
            ConstructionIntent::SavedResume { opening, child, .. } => Some((opening, child)),
            _ => None,
        }
    }
    pub(super) fn valid_saved_binding(&self) -> bool {
        let ConstructionIntent::SavedResume {
            opening,
            child,
            selection,
        } = &self.intent
        else {
            return true;
        };
        opening == &self.preparation
            && metadata::preparation::token(opening)
            && codex_protocol::ThreadId::from_string(child).is_ok_and(|id| id.to_string() == *child)
            && selection.presentation == ClientPresentation::SideConversation
            && selection.prepared.is_none()
            && serde_json::to_vec(selection)
                .is_ok_and(|bytes| bytes.len() <= metadata::preparation::MAX_ARGUMENT_BYTES)
            && self
                .result
                .as_ref()
                .is_none_or(|result| result.thread_id == *child)
    }
    /// Extract immutable lookup data without restoring a mutation owner.
    pub fn saved_resume_query(&self) -> IoResult<Option<SavedResumeQuery>> {
        ClientConstructionOperation::validate_checkpoint(self)?;
        let Some((_, child)) = self.saved_opening() else {
            return Ok(None);
        };
        if !self.mutation_attempted
            || self.retired
            || self.saved_reply_received
            || self.result.is_some()
        {
            return Ok(None);
        }
        let original = self
            .frozen
            .as_ref()
            .ok_or_else(|| invalid("saved resume lost its frozen original"))?;
        let offer = self
            .offer
            .as_ref()
            .ok_or_else(|| invalid("saved resume lost its original offer"))?;
        Ok(Some(SavedResumeQuery {
            original: Arc::clone(original),
            offer: offer.clone(),
            child: child.into(),
        }))
    }
}

impl SavedResumeQuery {
    /// Only the app-server transport's explicit overload refusal permits a
    /// later read to continue a previously observed pending opening.
    pub fn is_read_overloaded(error: &std::io::Error) -> bool {
        matches!(error.get_ref(), Some(cause) if cause.is::<SavedResumeReadOverloaded>())
    }

    pub fn opening(&self) -> &str {
        &self.offer.preparation
    }
    pub fn child(&self) -> &str {
        &self.child
    }
    pub fn validate_child(&self, child: &str) -> IoResult<()> {
        if self.child != child {
            return Err(invalid("saved resume lookup changed its child"));
        }
        Ok(())
    }
    /// Observe the same prepared claim on the actual successor. An ordinary
    /// config result, rejected lookup, or transport failure carries no outcome.
    pub async fn read(
        &self,
        handle: &AppServerRequestHandle,
        child: &str,
    ) -> IoResult<SavedResumeObservation> {
        self.validate_child(child)?;
        let AppServerRequestHandle::Remote(remote) = handle else {
            return Err(invalid(
                "saved resume lookup requires actual retained frontend IO",
            ));
        };
        let wire = codex_app_server_protocol::JSONRPCRequest {
            id: RequestId::String(format!("alab-saved-resume-read-{}", fresh())),
            method: "config/read".into(),
            params: Some(serde_json::json!({"includeLayers":false, "_meta":{
                (metadata::preparation::KEY):serde_json::to_value(self).map_err(std::io::Error::other)?
            }})),
            trace: None,
        };
        let value = remote.request_json_rpc(wire).await?.map_err(|error| {
            if error.code == -32001
                && error.message == "Server overloaded; retry later."
                && error.data.is_none()
            {
                std::io::Error::new(std::io::ErrorKind::WouldBlock, SavedResumeReadOverloaded)
            } else {
                invalid("saved resume lookup was refused")
            }
        })?;
        let observed =
            side_cleanup_reply::observation_value(value, metadata::preparation::RESULT_KEY)?;
        let completion: Completion = serde_json::from_value(observed)
            .map_err(|_| invalid("invalid saved resume outcome"))?;
        let Completion::SavedResume {
            offer,
            thread_id,
            outcome,
        } = completion
        else {
            return Err(invalid("response was not a saved resume outcome"));
        };
        if offer != self.offer || thread_id != child || !outcome.valid() {
            return Err(invalid(
                "saved resume outcome changed the original operation",
            ));
        }
        Ok(match outcome {
            SavedResumeOutcome::Resumed {} => SavedResumeObservation::Resumed,
            SavedResumeOutcome::Pending {} => SavedResumeObservation::Pending,
            SavedResumeOutcome::Unavailable {} => SavedResumeObservation::Unavailable,
            SavedResumeOutcome::Rejected {
                code,
                message,
                had_data,
                value,
            } => SavedResumeObservation::Rejected(JSONRPCErrorError {
                code,
                message,
                data: had_data.then_some(value),
            }),
        })
    }
}
