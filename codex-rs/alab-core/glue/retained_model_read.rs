// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Original model/list read DATA recorded before the unchanged native decoder.
//! This is not Source currentness, input acceptance, Core effects or Native authority.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Mutex;
tokio::task_local! {static MODEL_READ:Option<Arc<Capture>>;}
fn invalid(message: impl fmt::Display) -> IoError {
    IoError::new(ErrorKind::InvalidData, message.to_string())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IoData {
    kind: String,
    #[serde(deserialize_with = "required")]
    raw: Option<i32>,
    message: String,
}
fn required<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d)
}
impl IoData {
    fn capture(error: &IoError) -> IoResult<Self> {
        // io::Error::new may own an arbitrary Error/source/downcast payload.
        // Display/kind equality cannot preserve that actual representation.
        if error.raw_os_error().is_none() && error.get_ref().is_some() {
            return Err(invalid(
                "actual model transport owns an unsupported custom error payload",
            ));
        }
        let data = Self {
            kind: format!("{:?}", error.kind()),
            raw: error.raw_os_error(),
            message: error.to_string(),
        };
        let _same_representation = data.restore()?;
        Ok(data)
    }
    fn restore(&self) -> IoResult<IoError> {
        if let Some(raw) = self.raw {
            let error = IoError::from_raw_os_error(raw);
            if error.to_string() != self.message || format!("{:?}", error.kind()) != self.kind {
                return Err(invalid("original native IO error representation changed"));
            }
            return Ok(error);
        }
        let kind = match self.kind.as_str() {
            "NotFound" => ErrorKind::NotFound,
            "PermissionDenied" => ErrorKind::PermissionDenied,
            "ConnectionRefused" => ErrorKind::ConnectionRefused,
            "ConnectionReset" => ErrorKind::ConnectionReset,
            "ConnectionAborted" => ErrorKind::ConnectionAborted,
            "NotConnected" => ErrorKind::NotConnected,
            "AddrInUse" => ErrorKind::AddrInUse,
            "AddrNotAvailable" => ErrorKind::AddrNotAvailable,
            "BrokenPipe" => ErrorKind::BrokenPipe,
            "AlreadyExists" => ErrorKind::AlreadyExists,
            "WouldBlock" => ErrorKind::WouldBlock,
            "InvalidInput" => ErrorKind::InvalidInput,
            "InvalidData" => ErrorKind::InvalidData,
            "TimedOut" => ErrorKind::TimedOut,
            "WriteZero" => ErrorKind::WriteZero,
            "Interrupted" => ErrorKind::Interrupted,
            "UnexpectedEof" => ErrorKind::UnexpectedEof,
            "Unsupported" => ErrorKind::Unsupported,
            "OutOfMemory" => ErrorKind::OutOfMemory,
            "Other" => ErrorKind::Other,
            _ => {
                return Err(invalid(
                    "actual model read error needs its exact native representation",
                ));
            }
        };
        let error = IoError::from(kind);
        if error.to_string() != self.message {
            return Err(invalid("simple native IO error representation changed"));
        }
        Ok(error)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Reply {
    Received { result: RequestResult },
    Transport { error: IoData },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recorded {
    schema: u8,
    reply: Reply,
}
impl Recorded {
    pub fn validate_data(&self) -> IoResult<()> {
        if self.schema != 1 || serde_json::to_vec(self).map_err(invalid)?.len() > 16 * 1024 * 1024 {
            return Err(invalid("model read record schema/budget"));
        }
        if let Reply::Transport { error } = &self.reply {
            let _native = error.restore()?;
        }
        Ok(())
    }
    /// Decode the SAME received DATA through the original converter, with the
    /// observation tap suppressed: historical DATA is never a new original RPC.
    pub fn decode<T: DeserializeOwned>(&self) -> std::result::Result<T, TypedRequestError> {
        if let Err(source) = self.validate_data() {
            return Err(TypedRequestError::Transport {
                method: "model/list".into(),
                source,
            });
        }
        let response = match &self.reply {
            Reply::Received { result } => Ok(result.clone()),
            Reply::Transport { error } => Err(match error.restore() {
                Ok(error) => error,
                Err(error) => {
                    return Err(TypedRequestError::Transport {
                        method: "model/list".into(),
                        source: error,
                    });
                }
            }),
        };
        MODEL_READ.sync_scope(None, || {
            crate::decode_typed_response("model/list", response)
        })
    }
}
#[derive(Default)]
struct State {
    record: Option<Recorded>,
    uncertain: bool,
}
pub struct Capture {
    state: Mutex<State>,
}
impl Capture {
    pub fn prepare() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
        })
    }
    pub async fn scope<F: std::future::Future>(self: &Arc<Self>, original: F) -> F::Output {
        if let Ok(Some(outer)) = MODEL_READ.try_with(Clone::clone) {
            // Both owners have ambiguous output identity, including an outer
            // record already captured before the nested original operation.
            outer
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .uncertain = true;
            self.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .uncertain = true;
        }
        MODEL_READ.scope(Some(self.clone()), original).await
    }
    pub fn recorded(&self) -> IoResult<Recorded> {
        let state = self
            .state
            .lock()
            .map_err(|_| invalid("actual original model read interrupted"))?;
        if state.uncertain {
            return Err(invalid("original model read identity was ambiguous"));
        }
        let record = state
            .record
            .clone()
            .ok_or_else(|| invalid("original model read has no observed response"))?;
        record.validate_data()?;
        Ok(record)
    }
}
pub(crate) fn before_decode(method: &str, response: &IoResult<RequestResult>) {
    if method != "model/list" {
        return;
    }
    let _observed = MODEL_READ.try_with(|active| {
        let Some(actual) = active else {
            return;
        };
        let mut state = actual
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.record.is_some() {
            state.uncertain = true;
            return;
        }
        let reply = match response {
            Ok(result) => Reply::Received {
                result: result.clone(),
            },
            Err(error) => match IoData::capture(error) {
                Ok(error) => Reply::Transport { error },
                Err(_) => {
                    state.uncertain = true;
                    return;
                }
            },
        };
        state.record = Some(Recorded { schema: 1, reply });
    });
}
#[cfg(test)]
#[path = "retained_model_read_tests.rs"]
mod tests;
