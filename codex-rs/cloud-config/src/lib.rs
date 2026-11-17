//! Cloud-hosted configuration data for Codex.
//!
//! This crate owns transport, caching, and refresh behavior for cloud-delivered
//! config data. Parsing and composition remain in `codex-config`.

mod backend;
mod bundle_loader;
mod cache;
mod chatgpt;
mod config_server;
mod config_source;
mod metrics;
mod service;
mod validation;

pub use bundle_loader::chatgpt_config_bundle_loader;
pub use bundle_loader::chatgpt_config_bundle_loader_for_storage;
pub use bundle_loader::chatgpt_config_bundle_loader_for_storage_without_cache;
pub use bundle_loader::ensure_config_server_login;
pub use bundle_loader::managed_config_auth_manager;
pub use bundle_loader::managed_config_bundle_loader;
pub use bundle_loader::managed_config_bundle_loader_for_auth;
pub use bundle_loader::managed_config_bundle_loader_for_storage;
pub use config_server::config_server_auth_manager;
pub use config_server::config_server_bundle_loader;

pub use bundle_loader::alab_destination::loader as alab_destination_config_loader; // ALAB(accounts): exact B manager, without detached credential prewarm.

pub use bundle_loader::alab_bootstrap::PreparedAuth as PreparedBootstrapAuth; // ALAB(owned-local): actual partial constructor, never SDK admission from metadata.
pub use bundle_loader::alab_bootstrap::OwnedLoader as OwnedBootstrapCloudLoader; // ALAB(owned-local): custody of the original refresh task through explicit phase shutdown.

pub use bundle_loader::alab_managed_bootstrap::ManagedBootstrapOwner; // ALAB(owned-local): actual backend cloud owners, without a second manager.
