//! 基于规则的隐私检测
//!
//! 使用关键词匹配和正则表达式检测用户输入中的隐私信息。
//! 支持中英文关键词，以及常见的 PII（个人身份信息）模式。

use std::sync::LazyLock;

use regex::Regex;

use super::types::{DetectionMethod, MatchedPattern, PrivacyCategory, PrivacyDetectionResult};

/// 隐私关键词及其类别
const PRIVACY_KEYWORDS: &[(&str, PrivacyCategory)] = &[
    // === 个人身份信息 ===
    ("身份证", PrivacyCategory::PersonalIdentification),
    ("身份证号", PrivacyCategory::PersonalIdentification),
    ("居民身份证", PrivacyCategory::PersonalIdentification),
    ("护照", PrivacyCategory::PersonalIdentification),
    ("护照号", PrivacyCategory::PersonalIdentification),
    ("驾照", PrivacyCategory::PersonalIdentification),
    ("驾驶证", PrivacyCategory::PersonalIdentification),
    ("社保号", PrivacyCategory::PersonalIdentification),
    ("社会保障号", PrivacyCategory::PersonalIdentification),
    ("ssn", PrivacyCategory::PersonalIdentification),
    ("social security", PrivacyCategory::PersonalIdentification),
    ("passport", PrivacyCategory::PersonalIdentification),
    ("id card", PrivacyCategory::PersonalIdentification),
    ("driver license", PrivacyCategory::PersonalIdentification),
    ("出生日期", PrivacyCategory::PersonalIdentification),
    ("生日", PrivacyCategory::PersonalIdentification),
    // === 财务信息 ===
    ("银行卡", PrivacyCategory::Financial),
    ("银行卡号", PrivacyCategory::Financial),
    ("信用卡", PrivacyCategory::Financial),
    ("信用卡号", PrivacyCategory::Financial),
    ("借记卡", PrivacyCategory::Financial),
    ("储蓄卡", PrivacyCategory::Financial),
    ("账号", PrivacyCategory::Financial),
    ("账户", PrivacyCategory::Financial),
    ("卡号", PrivacyCategory::Financial),
    ("cvv", PrivacyCategory::Financial),
    ("cvc", PrivacyCategory::Financial),
    ("pin码", PrivacyCategory::Financial),
    ("pin code", PrivacyCategory::Financial),
    ("credit card", PrivacyCategory::Financial),
    ("debit card", PrivacyCategory::Financial),
    ("bank account", PrivacyCategory::Financial),
    ("routing number", PrivacyCategory::Financial),
    ("转账", PrivacyCategory::Financial),
    ("汇款", PrivacyCategory::Financial),
    ("支付宝", PrivacyCategory::Financial),
    ("微信支付", PrivacyCategory::Financial),
    ("余额", PrivacyCategory::Financial),
    // === 医疗健康信息 ===
    ("病历", PrivacyCategory::Medical),
    ("诊断", PrivacyCategory::Medical),
    ("处方", PrivacyCategory::Medical),
    ("病史", PrivacyCategory::Medical),
    ("医疗记录", PrivacyCategory::Medical),
    ("健康档案", PrivacyCategory::Medical),
    ("medical record", PrivacyCategory::Medical),
    ("diagnosis", PrivacyCategory::Medical),
    ("prescription", PrivacyCategory::Medical),
    ("health record", PrivacyCategory::Medical),
    ("血型", PrivacyCategory::Medical),
    ("过敏", PrivacyCategory::Medical),
    // === 凭证信息 ===
    ("密码", PrivacyCategory::Credentials),
    ("口令", PrivacyCategory::Credentials),
    ("登录密码", PrivacyCategory::Credentials),
    ("支付密码", PrivacyCategory::Credentials),
    ("api key", PrivacyCategory::Credentials),
    ("api_key", PrivacyCategory::Credentials),
    ("apikey", PrivacyCategory::Credentials),
    ("secret", PrivacyCategory::Credentials),
    ("secret key", PrivacyCategory::Credentials),
    ("secret_key", PrivacyCategory::Credentials),
    ("access token", PrivacyCategory::Credentials),
    ("access_token", PrivacyCategory::Credentials),
    ("private key", PrivacyCategory::Credentials),
    ("private_key", PrivacyCategory::Credentials),
    ("credential", PrivacyCategory::Credentials),
    ("auth token", PrivacyCategory::Credentials),
    ("bearer token", PrivacyCategory::Credentials),
    ("jwt", PrivacyCategory::Credentials),
    ("oauth", PrivacyCategory::Credentials),
    // === 联系方式 ===
    ("手机号", PrivacyCategory::Contact),
    ("电话号码", PrivacyCategory::Contact),
    ("联系电话", PrivacyCategory::Contact),
    ("座机", PrivacyCategory::Contact),
    ("邮箱", PrivacyCategory::Contact),
    ("电子邮件", PrivacyCategory::Contact),
    ("email", PrivacyCategory::Contact),
    ("phone number", PrivacyCategory::Contact),
    ("mobile", PrivacyCategory::Contact),
    ("qq号", PrivacyCategory::Contact),
    ("微信号", PrivacyCategory::Contact),
    ("wechat", PrivacyCategory::Contact),
    // === 位置信息 ===
    ("家庭住址", PrivacyCategory::Location),
    ("住址", PrivacyCategory::Location),
    ("居住地址", PrivacyCategory::Location),
    ("户籍", PrivacyCategory::Location),
    ("home address", PrivacyCategory::Location),
    ("street address", PrivacyCategory::Location),
    ("邮编", PrivacyCategory::Location),
    ("postal code", PrivacyCategory::Location),
    ("zip code", PrivacyCategory::Location),
    ("gps", PrivacyCategory::Location),
    ("经纬度", PrivacyCategory::Location),
    ("坐标", PrivacyCategory::Location),
    // === 生物特征 ===
    ("指纹", PrivacyCategory::Biometric),
    ("人脸", PrivacyCategory::Biometric),
    ("虹膜", PrivacyCategory::Biometric),
    ("fingerprint", PrivacyCategory::Biometric),
    ("face id", PrivacyCategory::Biometric),
    ("facial recognition", PrivacyCategory::Biometric),
    ("biometric", PrivacyCategory::Biometric),
];

/// PII 正则模式
struct PiiPattern {
    name: &'static str,
    pattern: &'static str,
    category: PrivacyCategory,
    confidence: f32,
}

const PII_PATTERNS: &[PiiPattern] = &[
    // 中国身份证号（18位）
    PiiPattern {
        name: "chinese_id_card",
        pattern: r"\b[1-9]\d{5}(?:19|20)\d{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\d|3[01])\d{3}[\dXx]\b",
        category: PrivacyCategory::PersonalIdentification,
        confidence: 0.95,
    },
    // 中国手机号
    PiiPattern {
        name: "chinese_phone",
        pattern: r"\b1[3-9]\d{9}\b",
        category: PrivacyCategory::Contact,
        confidence: 0.9,
    },
    // 电子邮箱
    PiiPattern {
        name: "email",
        pattern: r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b",
        category: PrivacyCategory::Contact,
        confidence: 0.95,
    },
    // 信用卡号（常见格式）
    PiiPattern {
        name: "credit_card",
        pattern: r"\b(?:\d{4}[\s-]?){3}\d{4}\b",
        category: PrivacyCategory::Financial,
        confidence: 0.85,
    },
    // 银行卡号（16-19位数字）
    PiiPattern {
        name: "bank_card",
        pattern: r"\b[3-6]\d{15,18}\b",
        category: PrivacyCategory::Financial,
        confidence: 0.8,
    },
    // 美国 SSN
    PiiPattern {
        name: "us_ssn",
        pattern: r"\b\d{3}-\d{2}-\d{4}\b",
        category: PrivacyCategory::PersonalIdentification,
        confidence: 0.95,
    },
    // 护照号（中国）
    PiiPattern {
        name: "chinese_passport",
        pattern: r"\b[EeGg]\d{8}\b",
        category: PrivacyCategory::PersonalIdentification,
        confidence: 0.85,
    },
    // IPv4 地址（可能包含位置信息）
    PiiPattern {
        name: "ipv4",
        pattern: r"\b(?:(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\.){3}(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\b",
        category: PrivacyCategory::Location,
        confidence: 0.7,
    },
    // API Key 模式（常见格式）
    PiiPattern {
        name: "api_key_pattern",
        pattern: r"\b(?:sk|pk|api|key|token)[-_]?[A-Za-z0-9]{20,}\b",
        category: PrivacyCategory::Credentials,
        confidence: 0.85,
    },
    // JWT Token
    PiiPattern {
        name: "jwt_token",
        pattern: r"\beyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b",
        category: PrivacyCategory::Credentials,
        confidence: 0.95,
    },
];

/// 编译后的正则表达式缓存
static COMPILED_PATTERNS: LazyLock<Vec<(Regex, &'static PiiPattern)>> = LazyLock::new(|| {
    PII_PATTERNS
        .iter()
        .filter_map(|p| Regex::new(p.pattern).ok().map(|r| (r, p)))
        .collect()
});

/// 基于关键词检测隐私信息
///
/// # 参数
/// - `text`: 待检测的文本
///
/// # 返回
/// 匹配到的隐私模式列表
pub fn detect_by_keywords(text: &str) -> Vec<MatchedPattern> {
    let text_lower = text.to_lowercase();
    let mut patterns = Vec::new();

    for (keyword, category) in PRIVACY_KEYWORDS {
        if text_lower.contains(keyword) {
            // 找到关键词的位置
            if let Some(pos) = text_lower.find(keyword) {
                patterns.push(
                    MatchedPattern::new(*category, format!("keyword:{}", keyword))
                        .with_position(pos)
                        .with_confidence(0.85),
                );
            }
        }
    }

    // 去重：同一类别只保留一个
    let mut seen_categories = std::collections::HashSet::new();
    patterns.retain(|p| seen_categories.insert(p.category));

    patterns
}

/// 基于正则表达式检测 PII
///
/// # 参数
/// - `text`: 待检测的文本
///
/// # 返回
/// 匹配到的隐私模式列表
pub fn detect_by_patterns(text: &str) -> Vec<MatchedPattern> {
    let mut patterns = Vec::new();

    for (regex, pii_pattern) in COMPILED_PATTERNS.iter() {
        if let Some(m) = regex.find(text) {
            let matched_text = m.as_str();
            let masked = mask_text(matched_text, pii_pattern.category);

            patterns.push(
                MatchedPattern::new(
                    pii_pattern.category,
                    format!("pattern:{}", pii_pattern.name),
                )
                .with_masked_text(masked)
                .with_position(m.start())
                .with_confidence(pii_pattern.confidence),
            );
        }
    }

    patterns
}

/// 执行完整的规则检测
///
/// # 参数
/// - `text`: 待检测的文本
/// - `custom_keywords`: 自定义敏感词列表
///
/// # 返回
/// 隐私检测结果
pub fn detect(text: &str, custom_keywords: &[String]) -> PrivacyDetectionResult {
    let mut all_patterns = Vec::new();

    // 1. 关键词检测
    all_patterns.extend(detect_by_keywords(text));

    // 2. 正则模式检测
    all_patterns.extend(detect_by_patterns(text));

    // 3. 自定义关键词检测
    let text_lower = text.to_lowercase();
    for keyword in custom_keywords {
        let keyword_lower = keyword.to_lowercase();
        if let Some(pos) = text_lower.find(&keyword_lower) {
            all_patterns.push(
                MatchedPattern::new(PrivacyCategory::Custom, format!("custom:{}", keyword))
                    .with_position(pos)
                    .with_confidence(0.9),
            );
        }
    }

    if all_patterns.is_empty() {
        PrivacyDetectionResult::not_private()
    } else {
        PrivacyDetectionResult::private(all_patterns, DetectionMethod::RuleBased)
    }
}

/// 对敏感文本进行脱敏处理
///
/// # 参数
/// - `text`: 原始文本
/// - `category`: 隐私类别
///
/// # 返回
/// 脱敏后的文本
pub fn mask_text(text: &str, category: PrivacyCategory) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    match category {
        PrivacyCategory::Contact => {
            // 手机号：保留前3后4
            if len >= 7 {
                let prefix: String = chars[..3].iter().collect();
                let suffix: String = chars[len - 4..].iter().collect();
                format!("{}****{}", prefix, suffix)
            } else {
                mask_middle(text)
            }
        }
        PrivacyCategory::Financial => {
            // 卡号：保留后4位
            if len >= 4 {
                let suffix: String = chars[len - 4..].iter().collect();
                format!("****{}", suffix)
            } else {
                "****".to_string()
            }
        }
        PrivacyCategory::PersonalIdentification => {
            // 身份证：保留前6后4
            if len >= 10 {
                let prefix: String = chars[..6].iter().collect();
                let suffix: String = chars[len - 4..].iter().collect();
                format!("{}****{}", prefix, suffix)
            } else {
                mask_middle(text)
            }
        }
        PrivacyCategory::Credentials => {
            // 凭证：只显示前4字符
            if len >= 4 {
                let prefix: String = chars[..4].iter().collect();
                format!("{}...", prefix)
            } else {
                "****".to_string()
            }
        }
        _ => mask_middle(text),
    }
}

/// 通用脱敏：保留首尾，中间用 * 替换
fn mask_middle(text: &str) -> String {
    let len = text.chars().count();
    if len <= 2 {
        "*".repeat(len)
    } else {
        let chars: Vec<char> = text.chars().collect();
        let first = chars[0];
        let last = chars[len - 1];
        format!("{}{}{}*", first, "*".repeat(len - 2), last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_by_keywords_chinese() {
        let text = "请帮我查询身份证号码";
        let patterns = detect_by_keywords(text);
        assert!(!patterns.is_empty());
        assert!(
            patterns
                .iter()
                .any(|p| p.category == PrivacyCategory::PersonalIdentification)
        );
    }

    #[test]
    fn test_detect_by_keywords_english() {
        let text = "Please help me with my credit card information";
        let patterns = detect_by_keywords(text);
        assert!(!patterns.is_empty());
        assert!(
            patterns
                .iter()
                .any(|p| p.category == PrivacyCategory::Financial)
        );
    }

    #[test]
    fn test_detect_by_keywords_credentials() {
        let text = "我的 API key 是什么";
        let patterns = detect_by_keywords(text);
        assert!(!patterns.is_empty());
        assert!(
            patterns
                .iter()
                .any(|p| p.category == PrivacyCategory::Credentials)
        );
    }

    #[test]
    fn test_detect_by_patterns_phone() {
        let text = "我的手机号是 13812345678";
        let patterns = detect_by_patterns(text);
        assert!(!patterns.is_empty());
        assert!(
            patterns
                .iter()
                .any(|p| p.category == PrivacyCategory::Contact)
        );
    }

    #[test]
    fn test_detect_by_patterns_email() {
        let text = "请发邮件到 test@example.com";
        let patterns = detect_by_patterns(text);
        assert!(!patterns.is_empty());
        assert!(
            patterns
                .iter()
                .any(|p| p.category == PrivacyCategory::Contact)
        );
    }

    #[test]
    fn test_detect_by_patterns_id_card() {
        let text = "身份证号 110101199001011234";
        let patterns = detect_by_patterns(text);
        assert!(!patterns.is_empty());
        assert!(
            patterns
                .iter()
                .any(|p| p.category == PrivacyCategory::PersonalIdentification)
        );
    }

    #[test]
    fn test_detect_no_privacy() {
        let text = "今天天气怎么样？";
        let result = detect(text, &[]);
        assert!(!result.is_private);
        assert!(result.matched_patterns.is_empty());
    }

    #[test]
    fn test_detect_with_custom_keywords() {
        let text = "这是公司内部机密文档";
        let custom = vec!["机密文档".to_string()];
        let result = detect(text, &custom);
        assert!(result.is_private);
        assert!(
            result
                .matched_patterns
                .iter()
                .any(|p| p.category == PrivacyCategory::Custom)
        );
    }

    #[test]
    fn test_mask_phone() {
        let masked = mask_text("13812345678", PrivacyCategory::Contact);
        assert_eq!(masked, "138****5678");
    }

    #[test]
    fn test_mask_card() {
        let masked = mask_text("6222021234567890", PrivacyCategory::Financial);
        assert_eq!(masked, "****7890");
    }

    #[test]
    fn test_mask_id_card() {
        let masked = mask_text(
            "110101199001011234",
            PrivacyCategory::PersonalIdentification,
        );
        assert_eq!(masked, "110101****1234");
    }

    #[test]
    fn test_mask_credentials() {
        let masked = mask_text("sk-1234567890abcdef", PrivacyCategory::Credentials);
        assert_eq!(masked, "sk-1...");
    }
}
