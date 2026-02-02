//! 智能隐私检测模块
//!
//! 提供用户查询的隐私内容自动检测功能，支持：
//! - 基于规则的检测（关键词、正则表达式）
//! - 基于模型的检测（LLM 推理）
//! - 混合检测（规则优先 + 模型兜底）
//!
//! # 使用示例
//!
//! ```rust,ignore
//! use crate::services::privacy::{PrivacyDetector, PrivacyDetectorConfig};
//!
//! // 使用默认配置
//! let detector = PrivacyDetector::with_defaults();
//!
//! // 执行检测
//! let result = detector.detect("我的手机号是13812345678", None)?;
//!
//! if result.is_private {
//!     println!("检测到隐私内容: {:?}", result.matched_patterns);
//! }
//! ```
//!
//! # 检测模式
//!
//! - `RulesOnly`: 仅使用规则检测，适合低延迟场景
//! - `RulesThenModel`: 规则优先，无匹配时模型兜底（默认）
//! - `ModelOnly`: 仅使用模型检测，适合高精度场景
//! - `Combined`: 规则和模型并行，取并集
//!
//! # 隐私类别
//!
//! 支持检测以下类别的隐私信息：
//! - 个人身份信息（身份证、护照等）
//! - 财务信息（银行卡、信用卡等）
//! - 医疗健康信息
//! - 凭证信息（密码、API Key 等）
//! - 联系方式
//! - 位置信息
//! - 生物特征

pub mod detector;
pub mod model;
pub mod rules;
pub mod types;

// Re-export commonly used types
pub use detector::{DetectionMode, PrivacyDetector, PrivacyDetectorConfig};
pub use types::{
    DetectionMethod, MatchedPattern, PrivacyCategory, PrivacyDetectionError, PrivacyDetectionResult,
};
