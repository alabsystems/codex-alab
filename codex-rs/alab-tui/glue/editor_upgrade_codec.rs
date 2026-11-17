// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native editor data projections. These codecs grant no runtime admission.
//! Remote derives keep the wire schema here and native state in its own module.
macro_rules! native_state {
    ($native:path, $view:ident, $item:item) => {
        #[derive(serde::Serialize, serde::Deserialize)]
        $item

        impl serde::Serialize for $native {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                $view::serialize(self, serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $native {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                $view::deserialize(deserializer)
            }
        }
    };
}
pub(crate) use native_state;
