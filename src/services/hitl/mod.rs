//! Human-in-the-Loop (HITL) 机制模块
//!
//! 本模块实现了 HITL 机制，允许在关键操作执行前暂停，
//! 等待用户确认、修改或取消，从而确保 Agent 行为符合用户预期。
//!
//! # 架构
//!
//! - `types`: 核心类型定义（请求、响应、状态等）
//! - `config`: 风险配置（HitlConfig、DeclaredRisk、RuntimeLearningConfig）
//! - `store`: Pending Store（内存存储待处理请求）
//! - `risk_inference`: L2 系统推断规则
//! - `trust_store`: L3 信任存储
//! - `risk_assessor`: 风险评估器（三层策略）
//! - `manager`: HITL Manager（核心协调器）
//!
//! # 三层风险合成策略
//!
//! ```text
//! 最终风险 = max(L1 开发者声明, L2 系统推断) + L3 运行时修正
//! ```
//!
//! - L1: 开发者在 SKILL.md / MCP 配置中显式声明（不允许 Safe）
//! - L2: 基于工具名称关键词自动推断（兜底）
//! - L3: 用户批准后积累信任，逐级降低风险
//!
//! # 集成层
//!
//! `integration` 模块提供了 HITL 机制与工具执行的集成支持：
//! - `HitlToolCaller`: 工具调用器，自动检查风险并等待用户确认
//! - `PreviewBuilder`: 操作预览构建器，生成用户友好的操作预览

pub mod config;
pub mod handlers;
pub mod integration;
pub mod manager;
pub mod notifier;
pub mod risk_assessor;
pub mod risk_inference;
pub mod security;
pub mod store;
pub mod trust_store;
pub mod types;

// Re-exports
pub use config::{DeclaredRisk, HitlConfig, RuntimeLearningConfig};
pub use integration::{
    HitlToolCaller, HitlToolContext, HitlToolResult, McpHitlAdapter, McpServerHitlConfig,
    McpToolCategory, PreviewBuilder, SkillHitlAdapter, SkillHitlConfig, SkillInternalToolConfig,
};
pub use manager::HitlManager;
pub use notifier::{HitlEventBridge, HitlNotifier};
pub use risk_assessor::{RiskAssessment, RiskAssessor};
pub use security::SecurityValidator;
pub use store::PendingStore;
pub use trust_store::TrustStore;
pub use types::*;
