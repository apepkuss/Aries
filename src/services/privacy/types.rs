//! 隐私检测类型定义
//!
//! 本模块定义了智能隐私检测机制的所有核心数据类型。

use serde::{Deserialize, Serialize};

/// 隐私类别
///
/// 用于分类检测到的隐私信息类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyCategory {
    /// 个人身份信息（身份证、护照、驾照等）
    PersonalIdentification,
    /// 财务信息（银行卡、信用卡、交易等）
    Financial,
    /// 医疗健康信息（病历、诊断、处方等）
    Medical,
    /// 凭证信息（密码、API Key、Token 等）
    Credentials,
    /// 联系方式（手机号、邮箱、社交账号等）
    Contact,
    /// 位置信息（地址、GPS 坐标等）
    Location,
    /// 生物特征（指纹、人脸等）
    Biometric,
    /// 自定义敏感词
    Custom,
}

impl PrivacyCategory {
    /// 获取类别的中文描述
    pub fn description_zh(&self) -> &'static str {
        match self {
            Self::PersonalIdentification => "个人身份信息",
            Self::Financial => "财务信息",
            Self::Medical => "医疗健康信息",
            Self::Credentials => "凭证信息",
            Self::Contact => "联系方式",
            Self::Location => "位置信息",
            Self::Biometric => "生物特征",
            Self::Custom => "敏感信息",
        }
    }

    /// 获取类别的英文描述
    pub fn description_en(&self) -> &'static str {
        match self {
            Self::PersonalIdentification => "Personal Identification",
            Self::Financial => "Financial Information",
            Self::Medical => "Medical Information",
            Self::Credentials => "Credentials",
            Self::Contact => "Contact Information",
            Self::Location => "Location Information",
            Self::Biometric => "Biometric Data",
            Self::Custom => "Sensitive Information",
        }
    }
}

/// 检测方法
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMethod {
    /// 基于规则检测（关键词、正则表达式）
    RuleBased,
    /// 基于模型推理
    ModelInference,
    /// 混合检测（规则 + 模型）
    Combined,
}

/// 匹配的隐私模式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchedPattern {
    /// 隐私类别
    pub category: PrivacyCategory,
    /// 模式类型描述
    pub pattern_type: String,
    /// 匹配到的文本（已脱敏）
    pub matched_text: Option<String>,
    /// 在原文中的位置（起始字符索引）
    pub position: Option<usize>,
    /// 置信度 (0.0 - 1.0)
    pub confidence: f32,
}

impl MatchedPattern {
    /// 创建新的匹配模式
    pub fn new(category: PrivacyCategory, pattern_type: impl Into<String>) -> Self {
        Self {
            category,
            pattern_type: pattern_type.into(),
            matched_text: None,
            position: None,
            confidence: 1.0,
        }
    }

    /// 设置脱敏后的匹配文本
    pub fn with_masked_text(mut self, text: impl Into<String>) -> Self {
        self.matched_text = Some(text.into());
        self
    }

    /// 设置位置
    pub fn with_position(mut self, pos: usize) -> Self {
        self.position = Some(pos);
        self
    }

    /// 设置置信度
    pub fn with_confidence(mut self, conf: f32) -> Self {
        self.confidence = conf.clamp(0.0, 1.0);
        self
    }
}

/// 隐私检测结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyDetectionResult {
    /// 是否检测到隐私内容
    pub is_private: bool,
    /// 综合置信度 (0.0 - 1.0)
    pub confidence: f32,
    /// 检测方法
    pub detection_method: DetectionMethod,
    /// 匹配到的模式列表
    pub matched_patterns: Vec<MatchedPattern>,
    /// 建议说明
    pub recommendation: Option<String>,
}

impl PrivacyDetectionResult {
    /// 创建未检测到隐私的结果
    pub fn not_private() -> Self {
        Self {
            is_private: false,
            confidence: 0.0,
            detection_method: DetectionMethod::RuleBased,
            matched_patterns: Vec::new(),
            recommendation: None,
        }
    }

    /// 创建检测到隐私的结果
    pub fn private(patterns: Vec<MatchedPattern>, method: DetectionMethod) -> Self {
        let confidence = if patterns.is_empty() {
            0.0
        } else {
            // 取所有匹配模式中的最高置信度
            patterns
                .iter()
                .map(|p| p.confidence)
                .fold(0.0_f32, f32::max)
        };

        let categories: Vec<_> = patterns
            .iter()
            .map(|p| p.category.description_zh())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let recommendation = if categories.is_empty() {
            None
        } else {
            Some(format!(
                "检测到可能的{}，建议使用隐私模式处理",
                categories.join("、")
            ))
        };

        Self {
            is_private: true,
            confidence,
            detection_method: method,
            matched_patterns: patterns,
            recommendation,
        }
    }

    /// 合并另一个检测结果
    pub fn merge(mut self, other: Self) -> Self {
        if other.is_private {
            self.is_private = true;
            self.confidence = self.confidence.max(other.confidence);
            self.matched_patterns.extend(other.matched_patterns);

            // 更新检测方法
            self.detection_method = match (self.detection_method, other.detection_method) {
                (DetectionMethod::RuleBased, DetectionMethod::ModelInference)
                | (DetectionMethod::ModelInference, DetectionMethod::RuleBased) => {
                    DetectionMethod::Combined
                }
                (DetectionMethod::Combined, _) | (_, DetectionMethod::Combined) => {
                    DetectionMethod::Combined
                }
                (method, _) => method,
            };

            // 重新生成建议
            let categories: Vec<_> = self
                .matched_patterns
                .iter()
                .map(|p| p.category.description_zh())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            self.recommendation = Some(format!(
                "检测到可能的{}，建议使用隐私模式处理",
                categories.join("、")
            ));
        }
        self
    }
}

/// 隐私检测错误
#[derive(Debug, Clone, thiserror::Error)]
pub enum PrivacyDetectionError {
    #[error("Detection not enabled")]
    NotEnabled,

    #[error("Model detection failed: {0}")]
    ModelError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_privacy_category_description() {
        assert_eq!(
            PrivacyCategory::PersonalIdentification.description_zh(),
            "个人身份信息"
        );
        assert_eq!(
            PrivacyCategory::Financial.description_en(),
            "Financial Information"
        );
    }

    #[test]
    fn test_matched_pattern_builder() {
        let pattern = MatchedPattern::new(PrivacyCategory::Contact, "phone")
            .with_masked_text("138****1234")
            .with_position(10)
            .with_confidence(0.95);

        assert_eq!(pattern.category, PrivacyCategory::Contact);
        assert_eq!(pattern.matched_text, Some("138****1234".to_string()));
        assert_eq!(pattern.position, Some(10));
        assert!((pattern.confidence - 0.95).abs() < f32::EPSILON);
    }

    #[test]
    fn test_detection_result_not_private() {
        let result = PrivacyDetectionResult::not_private();
        assert!(!result.is_private);
        assert!(result.matched_patterns.is_empty());
    }

    #[test]
    fn test_detection_result_private() {
        let patterns = vec![
            MatchedPattern::new(PrivacyCategory::Contact, "phone").with_confidence(0.9),
            MatchedPattern::new(PrivacyCategory::Financial, "credit_card").with_confidence(0.8),
        ];

        let result = PrivacyDetectionResult::private(patterns, DetectionMethod::RuleBased);
        assert!(result.is_private);
        assert!((result.confidence - 0.9).abs() < f32::EPSILON);
        assert_eq!(result.matched_patterns.len(), 2);
        assert!(result.recommendation.is_some());
    }

    #[test]
    fn test_detection_result_merge() {
        let result1 = PrivacyDetectionResult::private(
            vec![MatchedPattern::new(PrivacyCategory::Contact, "phone").with_confidence(0.8)],
            DetectionMethod::RuleBased,
        );

        let result2 = PrivacyDetectionResult::private(
            vec![MatchedPattern::new(PrivacyCategory::Financial, "card").with_confidence(0.9)],
            DetectionMethod::ModelInference,
        );

        let merged = result1.merge(result2);
        assert!(merged.is_private);
        assert!((merged.confidence - 0.9).abs() < f32::EPSILON);
        assert_eq!(merged.matched_patterns.len(), 2);
        assert_eq!(merged.detection_method, DetectionMethod::Combined);
    }
}
