// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn alab_da1_supported() -> bool {
    DA1_SUPPORTED.load(Ordering::Relaxed)
}
pub(crate) fn alab_set_da1_supported(supported: bool) {
    DA1_SUPPORTED.store(supported, Ordering::Relaxed);
}
