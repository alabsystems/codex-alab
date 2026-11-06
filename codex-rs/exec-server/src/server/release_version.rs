//! Include the startup-cached package release version in executor metadata.

use codex_build_info::BuildInfo;

use crate::protocol::EnvironmentInfo;

#[path = "../../../alab-core/glue/executor_version.rs"]
mod alab_executor_version; // ALAB(version): keep the product display separate from executor compatibility

pub(super) fn local_environment_info() -> EnvironmentInfo {
    EnvironmentInfo {
        executor_version: alab_executor_version::version(BuildInfo::get().version().to_string()), // ALAB(version): the client line describes the upstream protocol
        ..EnvironmentInfo::local()
    }
}
