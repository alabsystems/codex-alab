// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
//! Portable cache metadata; upstream's request enum is serialize-only.
use super::PromptCacheTtl;
use serde::Deserialize;
use serde::Serialize;
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub(super) enum CacheLifetime {
    ThirtyMinutes,
    TwelveHours,
}
impl From<PromptCacheTtl> for CacheLifetime {
    fn from(value: PromptCacheTtl) -> Self {
        match value {
            PromptCacheTtl::ThirtyMinutes => Self::ThirtyMinutes,
            PromptCacheTtl::TwelveHours => Self::TwelveHours,
        }
    }
}
impl From<CacheLifetime> for PromptCacheTtl {
    fn from(value: CacheLifetime) -> Self {
        match value {
            CacheLifetime::ThirtyMinutes => Self::ThirtyMinutes,
            CacheLifetime::TwelveHours => Self::TwelveHours,
        }
    }
}
