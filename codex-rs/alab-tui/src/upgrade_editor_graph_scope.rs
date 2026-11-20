// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Synchronous, thread-scoped Serde adapters for one editor generation type.
//! No scope may cross an await, nest, or transfer a live runtime resource.

use super::upgrade_editor_graph::Capture;
use super::upgrade_editor_graph::GenerationId;
use super::upgrade_editor_graph::Restore;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::any::Any;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::Weak;

thread_local! {
    static SCOPE: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
}

enum Scope<T> {
    Collect(Capture<T>),
    Encode(Capture<T>),
    Decode(Restore<T>),
    Nodes,
}

struct Guard;
impl Guard {
    fn enter<T: 'static>(scope: Scope<T>) -> Result<Self, String> {
        SCOPE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some() {
                return Err("nested editor generation scope".into());
            }
            *slot = Some(Box::new(scope));
            Ok(Self)
        })
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        SCOPE.with(|slot| *slot.borrow_mut() = None);
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame<T> {
    generations: Vec<T>,
    body: Box<serde_json::value::RawValue>,
}

fn bounded_json(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, String> {
    crate::upgrade_frame::encode(&[], value, limit, "editor checkpoint exceeds byte budget")
        .map_err(|error| error.to_string())
}

/// Two immutable traversals: first collect all strong roots, then resolve weak
/// keys. Thus a weak field may precede its root in the source struct or map.
/// The caller supplies a byte budget shared with its enclosing runtime frame.
pub fn encode<T: Serialize + 'static>(
    body: &impl Serialize,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let _guard = Guard::enter(Scope::<T>::Collect(Capture::default()))?;
    serde_json::to_writer(std::io::sink(), body).map_err(|error| error.to_string())?;
    SCOPE
        .with(|slot| {
            let mut slot = slot.borrow_mut();
            let scope = slot
                .as_mut()
                .and_then(|scope| scope.downcast_mut::<Scope<T>>())
                .ok_or("missing editor generation scope")?;
            let Scope::Collect(capture) = scope else {
                return Err("invalid editor generation phase");
            };
            *scope = Scope::Encode(std::mem::take(capture));
            Ok(())
        })
        .map_err(str::to_owned)?;
    let body = bounded_json(body, max_bytes)?;
    let body = serde_json::value::RawValue::from_string(
        String::from_utf8(body).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let capture = SCOPE
        .with(|slot| {
            let mut slot = slot.borrow_mut();
            let scope = slot
                .as_mut()
                .and_then(|scope| scope.downcast_mut::<Scope<T>>())
                .ok_or("missing editor generation encoder")?;
            match std::mem::replace(scope, Scope::Nodes) {
                Scope::Encode(capture) => Ok(capture),
                _ => Err("invalid editor generation phase"),
            }
        })
        .map_err(str::to_owned)?;
    // No RefCell borrow crosses user serialization. Nodes must be leaf values;
    // adapters inside a node refuse rather than recursively acquire graph roots.
    bounded_json(
        &Frame {
            generations: capture.values().collect::<Vec<_>>(),
            body,
        },
        max_bytes,
    )
}

pub fn decode<T: DeserializeOwned + 'static, B: DeserializeOwned>(
    bytes: &[u8],
    max_bytes: usize,
    max_nodes: usize,
) -> Result<B, String> {
    if bytes.len() > max_bytes {
        return Err("editor checkpoint exceeds byte budget".into());
    }
    let frame: Frame<Box<serde_json::value::RawValue>> =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    if frame.generations.len() > max_nodes {
        return Err("editor checkpoint exceeds generation budget".into());
    }
    let generations = frame
        .generations
        .into_iter()
        .map(|value| serde_json::from_str::<T>(value.get()).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let _guard = Guard::enter(Scope::Decode(Restore::new(generations)))?;
    let body = serde_json::from_str(frame.body.get()).map_err(|error| error.to_string())?;
    SCOPE
        .with(|slot| {
            let slot = slot.borrow();
            match slot
                .as_ref()
                .and_then(|scope| scope.downcast_ref::<Scope<T>>())
            {
                Some(Scope::Decode(restore)) => restore.validate_roots(),
                _ => Err("missing editor generation decoder"),
            }
        })
        .map_err(str::to_owned)?;
    Ok(body)
}

/// Inline values are only for standalone leaf codecs. A graph scope always
/// emits references and never accepts inline data that could break sharing.
#[derive(Serialize, Deserialize)]
enum Strong<T> {
    Inline(T),
    Generation(GenerationId),
}

pub mod strong {
    use super::*;

    pub fn serialize<T: Serialize + 'static, S: serde::Serializer>(
        value: &Arc<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let id = SCOPE
            .with(|slot| {
                let mut slot = slot.borrow_mut();
                match slot.as_mut() {
                    None => Ok(None),
                    Some(scope) => match scope.downcast_mut::<Scope<T>>() {
                        Some(Scope::Collect(capture)) => Ok(Some(capture.strong(value))),
                        Some(Scope::Encode(capture)) => capture
                            .weak(&Arc::downgrade(value))?
                            .ok_or("editor generation changed during capture")
                            .map(Some),
                        _ => Err("wrong editor generation scope"),
                    },
                }
            })
            .map_err(serde::ser::Error::custom)?;
        match id {
            Some(id) => Strong::<&T>::Generation(id).serialize(serializer),
            None => Strong::Inline(value.as_ref()).serialize(serializer),
        }
    }

    pub fn deserialize<'de, T: Deserialize<'de> + 'static, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Arc<T>, D::Error> {
        let value = Strong::<T>::deserialize(deserializer)?;
        SCOPE
            .with(|slot| {
                let slot = slot.borrow();
                match (slot.as_ref(), value) {
                    (None, Strong::Inline(value)) => Ok(Arc::new(value)),
                    (Some(scope), Strong::Generation(id)) => match scope.downcast_ref::<Scope<T>>()
                    {
                        Some(Scope::Decode(restore)) => restore.strong(id),
                        _ => Err("wrong editor generation scope"),
                    },
                    _ => Err("editor generation reference outside its graph"),
                }
            })
            .map_err(serde::de::Error::custom)
    }
}

pub mod weak {
    use super::*;

    pub fn serialize<T: 'static, S: serde::Serializer>(
        value: &Weak<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let id = SCOPE
            .with(|slot| {
                let slot = slot.borrow();
                match slot
                    .as_ref()
                    .and_then(|scope| scope.downcast_ref::<Scope<T>>())
                {
                    Some(Scope::Collect(_)) => Ok(None),
                    Some(Scope::Encode(capture)) => capture.weak(value),
                    _ => Err("weak editor generation key outside capture"),
                }
            })
            .map_err(serde::ser::Error::custom)?;
        id.serialize(serializer)
    }

    pub fn deserialize<'de, T: 'static, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Weak<T>, D::Error> {
        let id = Option::<GenerationId>::deserialize(deserializer)?;
        SCOPE
            .with(|slot| {
                let slot = slot.borrow();
                match slot
                    .as_ref()
                    .and_then(|scope| scope.downcast_ref::<Scope<T>>())
                {
                    Some(Scope::Decode(restore)) => match id {
                        Some(id) => restore.weak(id),
                        None => Ok(Weak::new()),
                    },
                    _ => Err("weak editor generation key outside restore"),
                }
            })
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize)]
    struct Editor {
        // Intentionally before its root to exercise the collection pass.
        #[serde(with = "weak")]
        payload: Weak<Vec<String>>,
        #[serde(with = "strong")]
        live: Arc<Vec<String>>,
        #[serde(with = "strong")]
        recorded: Arc<Vec<String>>,
    }

    #[test]
    fn weak_before_strong_and_distinct_equal_generations_restore() {
        let live = Arc::new(vec!["image".to_owned()]);
        let recorded = Arc::new(live.as_ref().clone());
        let source = Editor {
            payload: Arc::downgrade(&recorded),
            live,
            recorded,
        };
        let bytes = encode::<Vec<String>>(&source, 4096).unwrap();
        let restored: Editor = decode::<Vec<String>, _>(&bytes, 4096, 2).unwrap();
        assert!(Weak::ptr_eq(
            &restored.payload,
            &Arc::downgrade(&restored.recorded)
        ));
        assert!(!Weak::ptr_eq(
            &restored.payload,
            &Arc::downgrade(&restored.live)
        ));
        assert_eq!(source.live, restored.live);
        assert!(decode::<Vec<String>, Editor>(&bytes, 4096, 1).is_err());
        assert!(decode::<Vec<String>, Editor>(&bytes, 1, 2).is_err());
        assert!(encode::<Vec<String>>(&source, 1).is_err());
        assert!(
            serde_json::from_str::<Editor>(
                r#"{"payload":0,"live":{"Generation":0},"recorded":{"Generation":0}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn inline_injection_and_nested_capture_refuse_without_leaking_scope() {
        let inline = br#"{"generations":[],"body":{"payload":null,"live":{"Inline":[]},"recorded":{"Inline":[]}}}"#;
        assert!(decode::<Vec<String>, Editor>(inline, 4096, 2).is_err());
        let _guard = Guard::enter(Scope::<Vec<String>>::Collect(Capture::default())).unwrap();
        assert!(
            encode::<Vec<String>>(&(), 4096)
                .unwrap_err()
                .contains("nested")
        );
        // A rejected nested scope must not clear the outer capture.
        let generation = Arc::new(vec!["outer".to_owned()]);
        #[derive(Serialize)]
        struct Root<'a>(#[serde(with = "strong")] &'a Arc<Vec<String>>);
        assert!(serde_json::to_vec(&Root(&generation)).is_ok());
    }

    #[test]
    fn table_ownership_cannot_turn_an_unrooted_payload_into_a_live_generation() {
        let wire = br#"{"generations":[["unrooted"]],"body":{"Generation":0}}"#;
        #[derive(Deserialize)]
        struct WeakOnly(#[serde(with = "weak")] Weak<Vec<String>>);
        let weak_only = br#"{"generations":[["unrooted"]],"body":0}"#;
        assert!(decode::<Vec<String>, WeakOnly>(weak_only, 4096, 1).is_err());
        #[derive(Deserialize)]
        struct StrongRoot(#[serde(with = "strong")] Arc<Vec<String>>);
        let restored = decode::<Vec<String>, StrongRoot>(wire, 4096, 1).unwrap();
        assert_eq!(restored.0.as_ref(), &["unrooted"]);
        // Exercise the field to keep diagnostics meaningful in this fixture.
        assert!(WeakOnly(Weak::new()).0.upgrade().is_none());
    }

    #[test]
    fn capture_refuses_unrooted_live_payload_and_scope_is_cleared_after_error() {
        let unrelated = Arc::new(vec!["attachment".to_owned()]);
        let source = Editor {
            payload: Arc::downgrade(&unrelated),
            live: Arc::new(Vec::new()),
            recorded: Arc::new(Vec::new()),
        };
        assert!(
            encode::<Vec<String>>(&source, 4096)
                .unwrap_err()
                .contains("missing from strong roots")
        );
        let valid = Editor {
            payload: Arc::downgrade(&source.live),
            ..source
        };
        let bytes = encode::<Vec<String>>(&valid, 4096).unwrap();
        let malformed = String::from_utf8(bytes.clone())
            .unwrap()
            .replace("\"Generation\":0", "\"Generation\":99");
        assert!(decode::<Vec<String>, Editor>(malformed.as_bytes(), 4096, 2).is_err());
        assert!(decode::<Vec<String>, Editor>(&bytes, 4096, 2).is_ok());
    }
}
