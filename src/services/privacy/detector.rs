//! 隐私检测器
//!
//! 整合规则检测和模型检测，提供统一的隐私检测接口。
//! 支持"规则优先 + 模型兜底"的检测策略。

use serde::{Deserialize, Serialize};

use super::{
    model::{ModelDetector, ModelDetectorConfig},
    rules,
    types::{PrivacyDetectionError, PrivacyDetectionResult},
};

/// 检测模式
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMode {
    /// 仅规则检测
    RulesOnly,
    /// 规则优先，无匹配时模型兜底
    #[default]
    RulesThenModel,
    /// 仅模型检测
    ModelOnly,
    /// 规则和模型并行，取并集
    Combined,
}

/// 隐私检测器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyDetectorConfig {
    /// 是否启用检测
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// 检测模式
    ///
    /// - `rules_only`: 仅使用规则检测（关键词 + 正则模式），不调用模型
    /// - `rules_then_model`: 规则优先检测，若规则未匹配或置信度不足则调用模型兜底（默认）
    /// - `model_only`: 仅使用 LLM 模型检测，跳过规则检测
    /// - `combined`: 规则和模型并行执行，合并两者结果（取并集）
    #[serde(default)]
    pub mode: DetectionMode,

    /// 规则检测置信度阈值
    #[serde(default = "default_rule_confidence_threshold")]
    pub rule_confidence_threshold: f32,

    /// 是否启用模型检测兜底
    #[serde(default = "default_enable_model_fallback")]
    pub enable_model_fallback: bool,

    /// 模型检测置信度阈值
    #[serde(default = "default_model_confidence_threshold")]
    pub model_confidence_threshold: f32,

    /// 自定义敏感词列表
    #[serde(default)]
    pub custom_keywords: Vec<String>,

    /// 最小文本长度（低于此长度跳过检测）
    #[serde(default = "default_min_text_length")]
    pub min_text_length: usize,
}

fn default_enabled() -> bool {
    true
}

fn default_rule_confidence_threshold() -> f32 {
    0.6
}

fn default_enable_model_fallback() -> bool {
    true
}

fn default_model_confidence_threshold() -> f32 {
    0.8
}

fn default_min_text_length() -> usize {
    5
}

impl Default for PrivacyDetectorConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            mode: DetectionMode::default(),
            rule_confidence_threshold: default_rule_confidence_threshold(),
            enable_model_fallback: default_enable_model_fallback(),
            model_confidence_threshold: default_model_confidence_threshold(),
            custom_keywords: Vec::new(),
            min_text_length: default_min_text_length(),
        }
    }
}

/// 隐私检测器
///
/// 提供统一的隐私检测接口，整合规则检测和模型检测。
pub struct PrivacyDetector {
    config: PrivacyDetectorConfig,
    model_detector: ModelDetector,
}

impl PrivacyDetector {
    /// 创建新的隐私检测器
    pub fn new(config: PrivacyDetectorConfig) -> Self {
        let model_config = ModelDetectorConfig {
            confidence_threshold: config.model_confidence_threshold,
            ..Default::default()
        };

        Self {
            config,
            model_detector: ModelDetector::new(model_config),
        }
    }

    /// 创建使用默认配置的检测器
    pub fn with_defaults() -> Self {
        Self::new(PrivacyDetectorConfig::default())
    }

    /// 执行隐私检测（仅规则）
    ///
    /// # 参数
    /// - `text`: 待检测的文本
    ///
    /// # 返回
    /// 隐私检测结果
    pub fn detect_rules_only(&self, text: &str) -> PrivacyDetectionResult {
        rules::detect(text, &self.config.custom_keywords)
    }

    /// 执行隐私检测（同步版本，仅规则）
    ///
    /// # 参数
    /// - `text`: 待检测的文本
    ///
    /// # 返回
    /// 隐私检测结果或错误
    pub fn detect(&self, text: &str) -> Result<PrivacyDetectionResult, PrivacyDetectionError> {
        // 1. 检查是否启用
        if !self.config.enabled {
            return Err(PrivacyDetectionError::NotEnabled);
        }

        // 2. 检查文本长度
        if text.chars().count() < self.config.min_text_length {
            return Ok(PrivacyDetectionResult::not_private());
        }

        // 3. 根据检测模式执行检测
        match self.config.mode {
            DetectionMode::RulesOnly => Ok(self.detect_rules_only(text)),
            DetectionMode::RulesThenModel | DetectionMode::Combined | DetectionMode::ModelOnly => {
                // 对于需要模型的模式，同步版本只执行规则检测
                // 模型检测需要使用异步版本
                let rule_result = self.detect_rules_only(text);

                if self.config.mode == DetectionMode::ModelOnly {
                    // ModelOnly 模式在同步版本中返回空结果
                    Ok(PrivacyDetectionResult::not_private())
                } else {
                    Ok(rule_result)
                }
            }
        }
    }

    /// 准备模型检测请求
    ///
    /// # 参数
    /// - `text`: 待检测的文本
    ///
    /// # 返回
    /// 用于发送给 LLM 的 prompt
    pub fn prepare_model_request(&self, text: &str) -> String {
        self.model_detector.prepare_request(text)
    }

    /// 处理模型检测响应
    ///
    /// # 参数
    /// - `response`: LLM 返回的文本
    ///
    /// # 返回
    /// 隐私检测结果
    pub fn process_model_response(
        &self,
        response: &str,
    ) -> Result<PrivacyDetectionResult, PrivacyDetectionError> {
        self.model_detector.process_response(response)
    }

    /// 执行完整检测（规则 + 模型兜底）
    ///
    /// # 参数
    /// - `text`: 待检测的文本
    /// - `model_response`: 模型检测的响应（如果需要模型检测）
    ///
    /// # 返回
    /// 隐私检测结果
    pub fn detect_with_model(
        &self,
        text: &str,
        model_response: Option<&str>,
    ) -> Result<PrivacyDetectionResult, PrivacyDetectionError> {
        // 1. 检查是否启用
        if !self.config.enabled {
            return Err(PrivacyDetectionError::NotEnabled);
        }

        // 2. 检查文本长度
        if text.chars().count() < self.config.min_text_length {
            return Ok(PrivacyDetectionResult::not_private());
        }

        // 3. 根据检测模式执行检测
        match self.config.mode {
            DetectionMode::RulesOnly => Ok(self.detect_rules_only(text)),

            DetectionMode::RulesThenModel => {
                let rule_result = self.detect_rules_only(text);

                // 如果规则检测到了隐私，直接返回
                if rule_result.is_private
                    && rule_result.confidence >= self.config.rule_confidence_threshold
                {
                    return Ok(rule_result);
                }

                // 规则未检测到或置信度不足，尝试模型检测
                if self.config.enable_model_fallback
                    && let Some(response) = model_response
                {
                    let model_result = self.process_model_response(response)?;
                    // 合并结果
                    return Ok(rule_result.merge(model_result));
                }

                Ok(rule_result)
            }

            DetectionMode::ModelOnly => {
                if let Some(response) = model_response {
                    self.process_model_response(response)
                } else {
                    Ok(PrivacyDetectionResult::not_private())
                }
            }

            DetectionMode::Combined => {
                let rule_result = self.detect_rules_only(text);

                if let Some(response) = model_response {
                    let model_result = self.process_model_response(response)?;
                    Ok(rule_result.merge(model_result))
                } else {
                    Ok(rule_result)
                }
            }
        }
    }

    /// 判断是否需要模型检测
    ///
    /// # 参数
    /// - `text`: 待检测的文本
    ///
    /// # 返回
    /// 是否需要调用模型进行检测
    pub fn needs_model_detection(&self, text: &str) -> Result<bool, PrivacyDetectionError> {
        // 检查是否启用
        if !self.config.enabled {
            return Ok(false);
        }

        // 检查文本长度
        if text.chars().count() < self.config.min_text_length {
            return Ok(false);
        }

        // 根据检测模式判断
        match self.config.mode {
            DetectionMode::RulesOnly => Ok(false),
            DetectionMode::ModelOnly => Ok(true),
            DetectionMode::Combined => Ok(true),
            DetectionMode::RulesThenModel => {
                if !self.config.enable_model_fallback {
                    return Ok(false);
                }

                // 先执行规则检测
                let rule_result = self.detect_rules_only(text);

                // 如果规则检测到了高置信度的隐私，不需要模型
                if rule_result.is_private
                    && rule_result.confidence >= self.config.rule_confidence_threshold
                {
                    Ok(false)
                } else {
                    // 规则未检测到或置信度不足，需要模型兜底
                    Ok(true)
                }
            }
        }
    }

    /// 获取配置
    pub fn config(&self) -> &PrivacyDetectorConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PrivacyDetectorConfig::default();
        assert!(config.enabled);
        assert_eq!(config.mode, DetectionMode::RulesThenModel);
        assert!((config.rule_confidence_threshold - 0.6).abs() < f32::EPSILON);
        assert!(config.enable_model_fallback);
    }

    #[test]
    fn test_detector_with_defaults() {
        let detector = PrivacyDetector::with_defaults();
        assert!(detector.config.enabled);
    }

    #[test]
    fn test_detect_not_enabled() {
        let config = PrivacyDetectorConfig {
            enabled: false,
            ..Default::default()
        };
        let detector = PrivacyDetector::new(config);

        let result = detector.detect("我的手机号是13812345678");
        assert!(matches!(result, Err(PrivacyDetectionError::NotEnabled)));
    }

    #[test]
    fn test_detect_short_text() {
        let config = PrivacyDetectorConfig {
            min_text_length: 10,
            ..Default::default()
        };
        let detector = PrivacyDetector::new(config);

        let result = detector.detect("hi").unwrap();
        assert!(!result.is_private);
    }

    #[test]
    fn test_detect_rules_only_mode() {
        let config = PrivacyDetectorConfig {
            mode: DetectionMode::RulesOnly,
            ..Default::default()
        };
        let detector = PrivacyDetector::new(config);

        let result = detector.detect("我的手机号是13812345678").unwrap();
        assert!(result.is_private);
    }

    #[test]
    fn test_detect_with_custom_keywords() {
        let config = PrivacyDetectorConfig {
            custom_keywords: vec!["机密".to_string()],
            ..Default::default()
        };
        let detector = PrivacyDetector::new(config);

        let result = detector.detect("这是机密文档").unwrap();
        assert!(result.is_private);
    }

    #[test]
    fn test_needs_model_detection_rules_only() {
        let config = PrivacyDetectorConfig {
            mode: DetectionMode::RulesOnly,
            ..Default::default()
        };
        let detector = PrivacyDetector::new(config);

        let needs_model = detector.needs_model_detection("some text").unwrap();
        assert!(!needs_model);
    }

    #[test]
    fn test_needs_model_detection_model_only() {
        let config = PrivacyDetectorConfig {
            mode: DetectionMode::ModelOnly,
            ..Default::default()
        };
        let detector = PrivacyDetector::new(config);

        let needs_model = detector.needs_model_detection("some text here").unwrap();
        assert!(needs_model);
    }

    #[test]
    fn test_needs_model_detection_rules_then_model() {
        let config = PrivacyDetectorConfig {
            mode: DetectionMode::RulesThenModel,
            enable_model_fallback: true,
            ..Default::default()
        };
        let detector = PrivacyDetector::new(config);

        // 规则检测到隐私，不需要模型
        let needs_model = detector
            .needs_model_detection("我的手机号是13812345678")
            .unwrap();
        assert!(!needs_model);

        // 规则未检测到，需要模型
        let needs_model = detector.needs_model_detection("今天天气很好啊").unwrap();
        assert!(needs_model);
    }

    #[test]
    fn test_detect_with_model() {
        let config = PrivacyDetectorConfig {
            mode: DetectionMode::RulesThenModel,
            ..Default::default()
        };
        let detector = PrivacyDetector::new(config);

        // 模拟模型响应
        let model_response = r#"{"is_private": true, "confidence": 0.9, "categories": ["financial"], "reason": "包含财务信息"}"#;

        let result = detector
            .detect_with_model("请帮我处理一些事情", Some(model_response))
            .unwrap();

        // 由于规则未检测到，应该使用模型结果
        assert!(result.is_private);
    }

    #[test]
    fn test_config_toml_deserialization() {
        // Test default config from TOML
        let toml_str = r#"
            enabled = true
            mode = "rules_then_model"
            rule_confidence_threshold = 0.7
            enable_model_fallback = true
            model_confidence_threshold = 0.85
            custom_keywords = ["机密", "内部"]
            min_text_length = 10
        "#;

        let config: PrivacyDetectorConfig = toml::from_str(toml_str).unwrap();
        assert!(config.enabled);
        assert_eq!(config.mode, DetectionMode::RulesThenModel);
        assert!((config.rule_confidence_threshold - 0.7).abs() < f32::EPSILON);
        assert!(config.enable_model_fallback);
        assert!((config.model_confidence_threshold - 0.85).abs() < f32::EPSILON);
        assert_eq!(config.custom_keywords.len(), 2);
        assert_eq!(config.min_text_length, 10);
    }

    #[test]
    fn test_config_toml_mode_variants() {
        // Test rules_only mode
        let toml_str = r#"mode = "rules_only""#;
        let config: PrivacyDetectorConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.mode, DetectionMode::RulesOnly);

        // Test model_only mode
        let toml_str = r#"mode = "model_only""#;
        let config: PrivacyDetectorConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.mode, DetectionMode::ModelOnly);

        // Test combined mode
        let toml_str = r#"mode = "combined""#;
        let config: PrivacyDetectorConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.mode, DetectionMode::Combined);
    }

    #[test]
    fn test_config_toml_defaults() {
        // Test minimal config uses defaults
        let toml_str = r#""#;
        let config: PrivacyDetectorConfig = toml::from_str(toml_str).unwrap();
        assert!(config.enabled);
        assert_eq!(config.mode, DetectionMode::RulesThenModel);
        assert!((config.rule_confidence_threshold - 0.6).abs() < f32::EPSILON);
        assert!(config.enable_model_fallback);
        assert!((config.model_confidence_threshold - 0.8).abs() < f32::EPSILON);
        assert!(config.custom_keywords.is_empty());
        assert_eq!(config.min_text_length, 5);
    }
}
