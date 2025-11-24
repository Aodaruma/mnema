//! Core domain models and services for Mnema.

pub mod assistant;
pub mod automation_log;
pub mod ids;
pub mod list;
pub mod llm_memory;
pub mod milestone;
pub mod project;
pub mod repository;
pub mod status;
pub mod task;
pub mod user_settings;

pub mod prelude {
    pub use crate::assistant::*;
    pub use crate::automation_log::*;
    pub use crate::ids::*;
    pub use crate::list::*;
    pub use crate::llm_memory::*;
    pub use crate::milestone::*;
    pub use crate::project::*;
    pub use crate::repository::*;
    pub use crate::status::*;
    pub use crate::task::*;
    pub use crate::user_settings::*;
}
