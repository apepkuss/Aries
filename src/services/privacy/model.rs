//! 基于模型的隐私检测
//!
//! 使用 LLM 作为兜底机制，检测规则无法覆盖的隐私信息。
//! 适用于语义复杂或上下文相关的隐私检测场景。

use serde::{Deserialize, Serialize};

use super::types::{
    DetectionMethod, MatchedPattern, PrivacyCategory, PrivacyDetectionError, PrivacyDetectionResult,
};

/// 模型检测的 prompt 模板
const DETECTION_PROMPT: &str = r#"分析以下用户查询是否包含或请求处理隐私/敏感信息。

隐私信息类别：
- personal_identification: 身份证、护照、驾照、社保号等身份证明
- financial: 银行卡、信用卡、账户信息、交易详情等财务数据
- medical: 病历、诊断、处方、健康档案等医疗信息
- credentials: 密码、API Key、Token、私钥等凭证
- contact: 手机号、邮箱、社交账号等联系方式
- location: 家庭住址、GPS坐标等位置信息
- biometric: 指纹、人脸等生物特征

请以 JSON 格式返回分析结果：
{
  "is_private": true/false,
  "confidence": 0.0-1.0,
  "categories": ["category1", "category2"],
  "reason": "简要说明"
}

用户查询：
"#;

/// 模型检测结果（从 LLM 响应解析）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDetectionResponse {
    /// 是否检测到隐私信息
    pub is_private: bool,
    /// 置信度
    pub confidence: f32,
    /// 检测到的隐私类别
    pub categories: Vec<String>,
    /// 检测原因说明
    pub reason: Option<String>,
}

impl ModelDetectionResponse {
    /// 将类别字符串转换为 PrivacyCategory
    fn parse_category(s: &str) -> Option<PrivacyCategory> {
        match s.to_lowercase().as_str() {
            "personal_identification" | "personalidentification" | "identification" => {
                Some(PrivacyCategory::PersonalIdentification)
            }
            "financial" | "finance" => Some(PrivacyCategory::Financial),
            "medical" | "health" => Some(PrivacyCategory::Medical),
            "credentials" | "credential" | "secret" => Some(PrivacyCategory::Credentials),
            "contact" | "communication" => Some(PrivacyCategory::Contact),
            "location" | "address" => Some(PrivacyCategory::Location),
            "biometric" | "biometrics" => Some(PrivacyCategory::Biometric),
            _ => None,
        }
    }

    /// 转换为 PrivacyDetectionResult
    pub fn into_result(self) -> PrivacyDetectionResult {
        if !self.is_private {
            return PrivacyDetectionResult::not_private();
        }

        let patterns: Vec<MatchedPattern> = self
            .categories
            .iter()
            .filter_map(|cat| Self::parse_category(cat))
            .map(|category| {
                MatchedPattern::new(category, "model_inference").with_confidence(self.confidence)
            })
            .collect();

        if patterns.is_empty() {
            return PrivacyDetectionResult::not_private();
        }

        let mut result = PrivacyDetectionResult::private(patterns, DetectionMethod::ModelInference);

        // 使用模型提供的原因作为建议
        if let Some(reason) = self.reason {
            result.recommendation = Some(reason);
        }

        result
    }
}

/// 构建检测 prompt
///
/// # 参数
/// - `query`: 用户查询文本
///
/// # 返回
/// 完整的检测 prompt
pub fn build_detection_prompt(query: &str) -> String {
    format!("{}{}", DETECTION_PROMPT, query)
}

/// 解析模型响应
///
/// # 参数
/// - `response`: LLM 返回的文本
///
/// # 返回
/// 解析后的检测结果
pub fn parse_model_response(
    response: &str,
) -> Result<ModelDetectionResponse, PrivacyDetectionError> {
    // 尝试提取 JSON 部分
    let json_str = extract_json(response).ok_or_else(|| {
        PrivacyDetectionError::ModelError("Failed to extract JSON from response".to_string())
    })?;

    serde_json::from_str(&json_str)
        .map_err(|e| PrivacyDetectionError::ModelError(format!("Failed to parse JSON: {}", e)))
}

/// 从响应文本中提取 JSON
fn extract_json(text: &str) -> Option<String> {
    // 尝试找到 JSON 对象的开始和结束
    let start = text.find('{')?;
    let end = text.rfind('}')?;

    if start < end {
        Some(text[start..=end].to_string())
    } else {
        None
    }
}

/// 模型检测器配置
#[derive(Debug, Clone)]
pub struct ModelDetectorConfig {
    /// 置信度阈值
    pub confidence_threshold: f32,
    /// 最大查询长度（超过则截断）
    pub max_query_length: usize,
}

impl Default for ModelDetectorConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.8,
            max_query_length: 2000,
        }
    }
}

/// 模型检测器
///
/// 注意：实际的 LLM 调用需要在集成时实现，
/// 这里只提供 prompt 构建和响应解析的工具函数。
pub struct ModelDetector {
    config: ModelDetectorConfig,
}

impl ModelDetector {
    /// 创建新的模型检测器
    pub fn new(config: ModelDetectorConfig) -> Self {
        Self { config }
    }

    /// 准备检测请求
    ///
    /// # 参数
    /// - `query`: 用户查询文本
    ///
    /// # 返回
    /// 用于发送给 LLM 的 prompt
    pub fn prepare_request(&self, query: &str) -> String {
        // 截断过长的查询
        let truncated = if query.chars().count() > self.config.max_query_length {
            let chars: String = query.chars().take(self.config.max_query_length).collect();
            format!("{}...", chars)
        } else {
            query.to_string()
        };

        build_detection_prompt(&truncated)
    }

    /// 处理检测响应
    ///
    /// # 参数
    /// - `response`: LLM 返回的文本
    ///
    /// # 返回
    /// 隐私检测结果
    pub fn process_response(
        &self,
        response: &str,
    ) -> Result<PrivacyDetectionResult, PrivacyDetectionError> {
        let parsed = parse_model_response(response)?;

        // 检查置信度是否达到阈值
        if parsed.is_private && parsed.confidence < self.config.confidence_threshold {
            // 置信度不足，视为未检测到
            return Ok(PrivacyDetectionResult::not_private());
        }

        Ok(parsed.into_result())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_detection_prompt() {
        let prompt = build_detection_prompt("我的手机号是多少");
        assert!(prompt.contains("分析以下用户查询"));
        assert!(prompt.contains("我的手机号是多少"));
    }

    #[test]
    fn test_parse_model_response_success() {
        let response = r#"
        Based on the analysis, here is the result:
        {
            "is_private": true,
            "confidence": 0.95,
            "categories": ["contact", "financial"],
            "reason": "查询涉及联系方式和财务信息"
        }
        "#;

        let parsed = parse_model_response(response).unwrap();
        assert!(parsed.is_private);
        assert!((parsed.confidence - 0.95).abs() < f32::EPSILON);
        assert_eq!(parsed.categories.len(), 2);
    }

    #[test]
    fn test_parse_model_response_not_private() {
        let response =
            r#"{"is_private": false, "confidence": 0.1, "categories": [], "reason": null}"#;

        let parsed = parse_model_response(response).unwrap();
        assert!(!parsed.is_private);
    }

    #[test]
    fn test_model_detection_response_into_result() {
        let response = ModelDetectionResponse {
            is_private: true,
            confidence: 0.9,
            categories: vec!["contact".to_string(), "financial".to_string()],
            reason: Some("检测到敏感信息".to_string()),
        };

        let result = response.into_result();
        assert!(result.is_private);
        assert_eq!(result.detection_method, DetectionMethod::ModelInference);
        assert_eq!(result.matched_patterns.len(), 2);
    }

    #[test]
    fn test_model_detector_prepare_request() {
        let config = ModelDetectorConfig {
            max_query_length: 10,
            ..Default::default()
        };
        let detector = ModelDetector::new(config);

        let prompt = detector.prepare_request("这是一个很长的查询文本");
        assert!(prompt.contains("..."));
    }

    #[test]
    fn test_model_detector_process_response_below_threshold() {
        let config = ModelDetectorConfig {
            confidence_threshold: 0.9,
            ..Default::default()
        };
        let detector = ModelDetector::new(config);

        let response =
            r#"{"is_private": true, "confidence": 0.7, "categories": ["contact"], "reason": null}"#;
        let result = detector.process_response(response).unwrap();

        // 置信度 0.7 低于阈值 0.9，应返回 not_private
        assert!(!result.is_private);
    }

    #[test]
    fn test_extract_json() {
        let text = "Here is the analysis: {\"key\": \"value\"} done.";
        let json = extract_json(text).unwrap();
        assert_eq!(json, "{\"key\": \"value\"}");
    }

    #[test]
    fn test_extract_json_nested() {
        let text = r#"Result: {"outer": {"inner": "value"}} end"#;
        let json = extract_json(text).unwrap();
        assert!(json.contains("outer"));
        assert!(json.contains("inner"));
    }
}
