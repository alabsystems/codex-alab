// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn alab_cached_colors() -> Option<[[u8; 3]; 2]> {
    #[cfg(not(test))]
    {
        let cache = default_colors_cache().lock().ok()?;
        cache.value.map(|colors| {
            [
                [colors.fg.0, colors.fg.1, colors.fg.2],
                [colors.bg.0, colors.bg.1, colors.bg.2],
            ]
        })
    }
    #[cfg(test)]
    {
        TEST_DEFAULT_COLORS
            .with(std::cell::Cell::get)
            .flatten()
            .map(|colors| {
                [
                    [colors.fg.0, colors.fg.1, colors.fg.2],
                    [colors.bg.0, colors.bg.1, colors.bg.2],
                ]
            })
    }
}
