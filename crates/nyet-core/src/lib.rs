#![forbid(unsafe_code)]

pub mod apply;
pub mod cache;
pub mod error;
pub mod eval;
pub mod intent;
pub mod option;
pub mod path;
pub mod plan;
pub mod render;
pub mod store;
pub mod transaction;
pub mod validate;
pub mod value;

pub use apply::{
    apply_safe_host, apply_safe_path, current_uid, plan_directory, stage_plan, PlanFile,
    StagedPlan, PLAN_VERSION,
};
pub use cache::default_cache_dir;
pub use error::{Error, Result};
pub use eval::{Engine, EngineConfig};
pub use intent::{Intent, IntentOp};
pub use option::{OptionSpec, OptionView};
pub use path::OptionPath;
pub use plan::{build_plan, Plan};
pub use render::{render_module, unified_diff};
pub use store::{default_state_dir, IntentStore};
pub use transaction::{history, Transaction};
pub use validate::{validate, Warning};
pub use value::Value;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn engine_name() -> &'static str {
    "nyet"
}
