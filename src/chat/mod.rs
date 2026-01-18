pub mod api;
pub mod emitter;
pub mod events;
pub mod plan;
pub mod planner;
pub mod shared;
pub mod trace;
mod utils;
pub mod xml_parser;

#[cfg(test)]
mod task_planner_skills_tests;

// Re-export public API types for convenience
pub use api::{execute_plan, ExecutePlanRequest, ExecutePlanResponse};
pub use emitter::EventEmitter;

// Generate a unique chat id for the chat completion request
pub(crate) fn gen_chat_id() -> String {
    format!("chatcmpl-{}", uuid::Uuid::new_v4())
}
