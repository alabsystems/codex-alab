//! Extension crate for the `/goal` feature.

mod accounting;
mod analytics;
mod api;
mod events;
mod extension;
mod metrics;
mod runtime;
mod spec;
mod steering;
mod tool;

pub use api::GoalClearOutcome; // ALAB(goal-recovery): expose exact clear outcomes to the durable ALab coordinator.
pub use api::GoalObjectiveUpdate;
pub use api::GoalService;
pub use api::GoalServiceError;
pub use api::GoalSetOutcome;
pub use api::GoalSetRequest;
pub use api::GoalTokenBudgetUpdate;
pub use extension::GoalExtension;
pub use extension::GoalExtensionConfig;
pub use extension::install_with_backend;
pub use runtime::GoalRuntimeHandle;
pub use runtime::PreviousGoalSnapshot;
#[doc(hidden)]
pub use runtime::UpgradeRetirementCoordinator; // ALAB(goal-recovery): publish a committed runtime inventory through its exact registry.
pub use spec::CREATE_GOAL_TOOL_NAME;
pub use spec::GET_GOAL_TOOL_NAME;
pub use spec::UPDATE_GOAL_TOOL_NAME;
pub use tool::CreateGoalRequest;
