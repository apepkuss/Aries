//! Sub-Agent 子系统模块
//!
//! 此模块提供 Sub-Agent 功能，允许主 Agent 动态创建和管理子 Agent 来执行特定任务。
//!
//! # 模块结构
//!
//! - `types`: 核心类型定义 (SubAgentId, SubAgentState, SubAgent 等)
//! - `config`: 配置类型 (SubAgentSystemConfig, SubAgentSpawnConfig 等)
//! - `manager`: SubAgentManager 实现，管理 Sub-Agent 生命周期
//! - `executor`: SubAgentExecutor 实现，执行 Sub-Agent 任务
//! - `context`: SubAgentContext 实现，管理独立上下文
//! - `channel`: 通信通道实现（可选）
//! - `tools`: 内部工具定义 (spawn_sub_agent, get_sub_agent_result 等)
//! - `handlers`: HTTP API 处理器
//!
//! # 使用示例
//!
//! ```rust,ignore
//! use aries::subagent::{SubAgentManager, SubAgentSystemConfig};
//!
//! // 创建 Manager
//! let config = SubAgentSystemConfig::default_enabled();
//! let manager = SubAgentManager::new(config);
//!
//! // 创建 Sub-Agent
//! let id = manager.spawn(
//!     "DataAnalyst",
//!     "You are a data analyst...",
//!     "Analyze the sales data",
//!     None,
//!     None,
//! ).await?;
//!
//! // 启动执行
//! manager.start(id, state, emitter).await?;
//!
//! // 获取结果
//! let result = manager.get_result(id).await?;
//! ```

// 阶段一：核心类型与配置
pub mod config;
pub mod types;

// TODO: 阶段一 Week 2 完成后取消注释
// pub mod manager;

// TODO: 阶段二完成后取消注释
// pub mod executor;
// pub mod context;
// pub mod tools;

// TODO: 阶段三完成后取消注释
// pub mod channel;

// TODO: 阶段四完成后取消注释
// pub mod handlers;

// 公开导出
pub use config::*;
pub use types::*;

// TODO: 阶段一 Week 2 完成后取消注释
// pub use manager::SubAgentManager;
