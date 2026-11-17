// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Exact installed theme observation. No theme/path/config is created on decode;
//! only equal actual semantic resources can rebind a process-local cache number.
use super::*;
use serde::Deserialize;
use serde::Serialize;
use std::io;
const MAX_THEME_BYTES: usize = 4 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ThemeStamp {
    schema: u8,
    revision: u64,
    #[serde(deserialize_with = "required")]
    theme: Option<serde_json::Value>,
}
fn required<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d)
}
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
pub(crate) enum CacheGeneration {
    Equivalent(u64),
    PendingInvalidation,
}
impl ThemeStamp {
    pub(crate) fn capture() -> io::Result<Self> {
        // get(), deliberately not get_or_init/theme_lock/current_syntax_theme.
        // The original final Config/theme installation must already own it.
        #[cfg(not(test))]
        let actual = THEME.get();
        #[cfg(test)]
        let actual = TEST_THEME.with(|actual| actual.get().cloned());
        let (revision, theme) = if let Some(actual) = actual {
            let actual = actual
                .read()
                .map_err(|_| invalid("actual syntax theme interrupted"))?;
            (
                syntax_theme_revision(),
                Some(serde_json::to_value(&*actual).map_err(invalid)?),
            )
        } else {
            (syntax_theme_revision(), None)
        };
        let result = Self {
            schema: 1,
            revision,
            theme,
        };
        result.validate_data()?;
        Ok(result)
    }
    pub(crate) fn validate_data(&self) -> io::Result<()> {
        if self.schema != 1
            || serde_json::to_vec(&self.theme).map_err(invalid)?.len() > MAX_THEME_BYTES
        {
            return Err(invalid("native theme observation schema/budget"));
        }
        if let Some(value) = &self.theme {
            let actual: Theme = serde_json::from_value(value.clone()).map_err(invalid)?;
            if serde_json::to_value(&actual).map_err(invalid)? != *value {
                return Err(invalid("noncanonical exact native theme DATA"));
            }
        } else if self.revision != 0 {
            return Err(invalid("absent native theme has an impossible revision"));
        }
        Ok(())
    }
    pub(crate) fn revision_data(&self) -> u64 {
        self.revision
    }
    pub(crate) fn semantic_eq(&self, other: &Self) -> bool {
        self.schema == other.schema && self.theme == other.theme
    }
    pub(crate) fn has_theme(&self) -> bool {
        self.theme.is_some()
    }
    fn prepare_runtime(&self) -> io::Result<Option<Theme>> {
        self.validate_data()?;
        let Some(value) = &self.theme else {
            return Ok(None);
        };
        let actual: Theme = serde_json::from_value(value.clone()).map_err(invalid)?;
        if serde_json::to_value(&actual).map_err(invalid)? != *value {
            return Err(invalid("noncanonical exact native theme DATA"));
        }
        Ok(Some(actual))
    }
    pub(crate) fn install_runtime(
        &self,
        tui: &mut crate::tui::Tui,
        renderer: &crate::retained_image_renderer::Binding,
    ) -> io::Result<()> {
        renderer.install_runtime_theme(tui, self, self.prepare_runtime()?)
    }
    fn match_installed(&self, old_cache_revision: u64) -> io::Result<u64> {
        self.validate_data()?;
        let actual = Self::capture()?;
        if old_cache_revision != self.revision || self.theme != actual.theme {
            return Err(invalid(
                "retained cache does not match the actual installed semantic theme",
            ));
        }
        Ok(actual.revision)
    }
    pub(crate) fn cache_generation(
        &self,
        old_cache_revision: u64,
        renderer: &crate::retained_image_renderer::Binding,
    ) -> io::Result<CacheGeneration> {
        renderer.check()?;
        self.validate_data()?;
        let actual = Self::capture()?;
        Ok(
            if self.revision == old_cache_revision && self.theme == actual.theme {
                CacheGeneration::Equivalent(actual.revision)
            } else {
                CacheGeneration::PendingInvalidation
            },
        )
    }
    pub(crate) fn rebind_revision(
        &self,
        old_cache_revision: u64,
        renderer: &crate::retained_image_renderer::Binding,
    ) -> io::Result<u64> {
        renderer.check()?;
        self.match_installed(old_cache_revision)
    }
}
/// The actual native write exclusion covers BOTH generation validation and
/// assignment/revision. Calling original set_syntax_theme while holding it would
/// relock; this owned descendant performs the same two original mutations once.
pub(crate) fn conditional_install(expected: &ThemeStamp, theme: Theme) -> io::Result<u64> {
    // Creating a Theme occurs only from the genuine captured runtime resource,
    // never build_default_theme/config/path. If another original initializer
    // wins, this route refuses instead of replacing its new actual state.
    let actual = if expected.has_theme() {
        #[cfg(not(test))]
        let actual = THEME
            .get()
            .ok_or_else(|| invalid("actual installed theme disappeared"))?;
        #[cfg(test)]
        let actual = TEST_THEME
            .with(|actual| actual.get().cloned())
            .ok_or_else(|| invalid("actual installed theme disappeared"))?;
        actual
    } else {
        let inserted = std::cell::Cell::new(false);
        #[cfg(not(test))]
        let actual = THEME.get_or_init(|| {
            inserted.set(true);
            RwLock::new(theme.clone())
        });
        #[cfg(test)]
        let actual = TEST_THEME.with(|actual| {
            actual
                .get_or_init(|| {
                    inserted.set(true);
                    std::sync::Arc::new(RwLock::new(theme.clone()))
                })
                .clone()
        });
        if !inserted.get() {
            return Err(invalid("another actual native theme initialization won"));
        }
        actual
    };
    #[cfg(test)]
    let actual = actual.as_ref();
    conditional_swap(
        actual,
        expected.revision_data(),
        theme,
        syntax_theme_revision,
        || {
            #[cfg(not(test))]
            {
                THEME_REVISION
                    .fetch_add(1, Ordering::Release)
                    .wrapping_add(1)
            }
            #[cfg(test)]
            {
                TEST_THEME_REVISION.with(|revision| {
                    let value = revision.get().wrapping_add(1);
                    revision.set(value);
                    value
                })
            }
        },
    )
}
fn conditional_swap(
    actual: &RwLock<Theme>,
    expected: u64,
    theme: Theme,
    current: impl FnOnce() -> u64,
    bump: impl FnOnce() -> u64,
) -> io::Result<u64> {
    let mut actual = actual
        .write()
        .map_err(|_| invalid("actual native theme write interrupted"))?;
    if current() != expected {
        return Err(invalid("newer actual C2 theme editing must be preserved"));
    }
    *actual = theme;
    Ok(bump())
}
#[cfg(test)]
#[path = "retained_image_theme_tests.rs"]
mod tests;
