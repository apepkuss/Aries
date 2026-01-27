//! HITL 集成模块
//!
//! 提供 HITL 机制与工具执行的集成支持，包括：
//! - 工具执行前的风险检查和用户确认
//! - 操作预览构建
//! - 用户修改应用
//!
//! # 架构
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    工具执行流程                              │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  ┌─────────────┐   ┌──────────────┐   ┌───────────────────┐│
//! │  │ 工具调用    │──▶│ HITL 检查    │──▶│ 执行 / 等待确认   ││
//! │  └─────────────┘   └──────────────┘   └───────────────────┘│
//! │                           │                                 │
//! │                           ▼                                 │
//! │                    ┌──────────────┐                        │
//! │                    │ 风险评估     │                        │
//! │                    │ (三层策略)   │                        │
//! │                    └──────────────┘                        │
//! │                           │                                 │
//! │              ┌────────────┼────────────┐                   │
//! │              ▼            ▼            ▼                   │
//! │        ┌────────┐  ┌────────────┐  ┌────────┐             │
//! │        │ 低风险 │  │ 中/高风险   │  │ 极高   │             │
//! │        │ 直接执行│  │ 创建HITL   │  │ 强制   │             │
//! │        └────────┘  │ 请求等待   │  │ 等待   │             │
//! │                    └────────────┘  └────────┘             │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # 使用方式
//!
//! ```ignore
//! use crate::services::hitl::integration::{HitlToolCaller, HitlToolResult};
//!
//! // 创建 HITL 工具调用器
//! let caller = HitlToolCaller::new(hitl_manager.clone());
//!
//! // 检查并执行工具
//! let result = caller.check_and_execute(
//!     &tool_name,
//!     &tool_args,
//!     &conversation_id,
//!     &user_id,
//!     execute_fn,
//! ).await?;
//!
//! match result {
//!     HitlToolResult::Executed(output) => { /* 正常执行 */ }
//!     HitlToolResult::Skipped(reason) => { /* 被跳过 */ }
//!     HitlToolResult::Rejected(reason) => { /* 被拒绝 */ }
//!     HitlToolResult::Aborted(reason) => { /* 被中止 */ }
//!     HitlToolResult::Modified(output) => { /* 修改后执行 */ }
//! }
//! ```

mod mcp;
mod preview;
mod skills;
mod tool_caller;

pub use mcp::{McpHitlAdapter, McpServerHitlConfig, McpToolCategory};
pub use preview::PreviewBuilder;
pub use skills::{SkillHitlAdapter, SkillHitlConfig, SkillInternalToolConfig};
pub use tool_caller::{HitlToolCaller, HitlToolContext, HitlToolResult};
