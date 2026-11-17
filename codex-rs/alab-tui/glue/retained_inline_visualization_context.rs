// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Cache aliases are UI DATA. Paths never construct visualization permission:
//! each restored context must match a genuine context supplied by original Config policy.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;

const MAX_CONTEXTS: usize = 1_000_000;
type Cache = Mutex<HashMap<PathBuf, String>>;

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct PathData(Vec<u8>);
impl PathData {
    fn capture(actual: &Path) -> Self {
        Self(actual.as_os_str().as_bytes().to_vec())
    }
    fn restore(&self) -> io::Result<PathBuf> {
        if self.0.contains(&0) {
            return Err(invalid("visualization path contains NUL"));
        }
        Ok(PathBuf::from(std::ffi::OsString::from_vec(self.0.clone())))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ContextData {
    visualizations_dir: PathData,
    thread_dir: PathData,
    viewer_dir: PathData,
    cache: u32,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GraphData {
    schema: u8,
    caches: Vec<Vec<Entry>>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    path: PathData,
    document: String,
}

#[derive(Default)]
pub(crate) struct Capture {
    indices: HashMap<usize, u32>,
    held: Vec<Arc<Cache>>,
}
impl Capture {
    pub(crate) fn context(
        &mut self,
        actual: &InlineVisualizationContext,
    ) -> io::Result<ContextData> {
        let pointer = Arc::as_ptr(&actual.materialized_viewers) as usize;
        let cache = if let Some(index) = self.indices.get(&pointer) {
            *index
        } else {
            if self.held.len() >= MAX_CONTEXTS {
                return Err(invalid("visualization cache graph capacity"));
            }
            let index = u32::try_from(self.held.len()).map_err(invalid)?;
            self.held.push(actual.materialized_viewers.clone());
            self.indices.insert(pointer, index);
            index
        };
        Ok(ContextData {
            visualizations_dir: PathData::capture(&actual.visualizations_dir),
            thread_dir: PathData::capture(&actual.thread_dir),
            viewer_dir: PathData::capture(&actual.viewer_dir),
            cache,
        })
    }
    pub(crate) fn export(&self) -> io::Result<GraphData> {
        let caches = self
            .held
            .iter()
            .map(|actual| {
                let actual = actual
                    .lock()
                    .map_err(|_| invalid("visualization cache is poisoned"))?;
                if actual.len() > MAX_CONTEXTS {
                    return Err(invalid("visualization cache entry capacity"));
                }
                let mut entries = actual
                    .iter()
                    .map(|(path, document)| Entry {
                        path: PathData::capture(path),
                        document: document.clone(),
                    })
                    .collect::<Vec<_>>();
                entries.sort_by(|a, b| a.path.0.cmp(&b.path.0));
                Ok(entries)
            })
            .collect::<io::Result<_>>()?;
        Ok(GraphData { schema: 1, caches })
    }
}

#[derive(Default)]
pub(crate) struct Bindings {
    actual: Vec<InlineVisualizationContext>,
    caches: Option<Vec<Arc<Cache>>>,
}
impl Bindings {
    /// The argument can only originate in the original native context constructor;
    /// no serialized path, thread ID or cache entry is an alternative constructor.
    pub(crate) fn bind(&mut self, actual: &InlineVisualizationContext) -> io::Result<()> {
        if self.actual.len() >= MAX_CONTEXTS {
            return Err(invalid("visualization binding capacity"));
        }
        self.actual.push(actual.clone());
        Ok(())
    }
    pub(crate) fn install(&mut self, data: GraphData) -> io::Result<()> {
        if self.caches.is_some() || data.schema != 1 || data.caches.len() > MAX_CONTEXTS {
            return Err(invalid(
                "visualization graph version/capacity/already installed",
            ));
        }
        let caches = data
            .caches
            .into_iter()
            .map(|entries| {
                if entries.len() > MAX_CONTEXTS {
                    return Err(invalid("visualization entry capacity"));
                }
                let mut cache = HashMap::new();
                for entry in entries {
                    if cache
                        .insert(entry.path.restore()?, entry.document)
                        .is_some()
                    {
                        return Err(invalid("duplicate native visualization cache path"));
                    }
                }
                Ok(Arc::new(Mutex::new(cache)))
            })
            .collect::<io::Result<_>>()?;
        self.caches = Some(caches);
        Ok(())
    }
    pub(crate) fn context(&self, data: &ContextData) -> io::Result<InlineVisualizationContext> {
        let visualizations_dir = data.visualizations_dir.restore()?;
        let thread_dir = data.thread_dir.restore()?;
        let viewer_dir = data.viewer_dir.restore()?;
        let actual = self
            .actual
            .iter()
            .find(|actual| {
                actual.visualizations_dir == visualizations_dir
                    && actual.thread_dir == thread_dir
                    && actual.viewer_dir == viewer_dir
            })
            .ok_or_else(|| {
                invalid(
                    "restored visualization context has no matching original Config-policy binding",
                )
            })?;
        let cache = self
            .caches
            .as_ref()
            .and_then(|caches| caches.get(data.cache as usize))
            .ok_or_else(|| invalid("visualization cache has no actual graph entry"))?;
        // Only the native materialization function writes this private cache. Verify
        // those original key coordinates without opening a file or granting a root.
        let directory = actual.viewer_dir.join(".codex-viewers");
        if cache
            .lock()
            .map_err(|_| invalid("restored visualization cache poisoned"))?
            .keys()
            .any(|path| path.parent() != Some(directory.as_path()) || path.file_name().is_none())
        {
            return Err(invalid(
                "visualization cache entry escapes its bound native viewer directory",
            ));
        }
        let mut restored = actual.clone();
        restored.materialized_viewers = cache.clone();
        Ok(restored)
    }
}

#[cfg(test)]
#[path = "retained_inline_visualization_tests.rs"]
mod tests;
