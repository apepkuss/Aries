//! HITL 安全验证模块
//!
//! 提供 HITL 请求的安全验证功能，包括：
//! - 响应权限验证
//! - 超时行为安全检查
//! - 风险级别一致性验证

use super::types::{
    HitlError, HitlRequest, HitlRequestStatus, HitlRequestType, HitlResponse, RiskLevel,
    TimeoutBehavior,
};

/// 安全验证器
pub struct SecurityValidator;

impl SecurityValidator {
    /// 验证用户是否有权限响应请求
    ///
    /// # 验证规则
    /// 1. 用户 ID 必须匹配请求的 user_id
    /// 2. 请求必须处于可响应状态（Pending）
    /// 3. 请求未过期
    ///
    /// # 参数
    /// - `request`: HITL 请求
    /// - `user_id`: 尝试响应的用户 ID
    ///
    /// # 返回
    /// - `Ok(())`: 验证通过
    /// - `Err(HitlError)`: 验证失败
    pub fn validate_response_permission(
        request: &HitlRequest,
        user_id: &str,
    ) -> Result<(), HitlError> {
        // 验证用户权限
        if request.user_id != user_id {
            return Err(HitlError::PermissionDenied(format!(
                "User '{}' is not authorized to respond to request '{}' (owned by '{}')",
                user_id, request.id, request.user_id
            )));
        }

        // 验证请求状态
        if !request.status.can_respond() {
            return Err(HitlError::NotPending(format!(
                "Request '{}' is not in pending status (current: {:?})",
                request.id, request.status
            )));
        }

        // 验证请求是否过期
        if request.is_expired() {
            return Err(HitlError::RequestExpired(request.id.clone()));
        }

        Ok(())
    }

    /// 验证响应类型是否与请求类型匹配
    ///
    /// # 匹配规则
    /// - Confirmation: Approve, Reject, Modify, Abort
    /// - Clarification: Clarify, Abort
    /// - Feedback: ProvideFeedback, Abort
    /// - Pause: Resume, Abort
    ///
    /// # 参数
    /// - `request_type`: 请求类型
    /// - `response`: 用户响应
    ///
    /// # 返回
    /// - `Ok(())`: 验证通过
    /// - `Err(HitlError)`: 验证失败
    pub fn validate_response_type(
        request_type: &HitlRequestType,
        response: &HitlResponse,
    ) -> Result<(), HitlError> {
        let is_valid = match (request_type, response) {
            // 确认请求允许：Approve, Reject, Modify, Abort
            (
                HitlRequestType::Confirmation(_),
                HitlResponse::Approve
                | HitlResponse::Reject { .. }
                | HitlResponse::Modify { .. }
                | HitlResponse::Abort { .. },
            ) => true,

            // 澄清请求允许：Clarify, Abort
            (
                HitlRequestType::Clarification(_),
                HitlResponse::Clarify { .. } | HitlResponse::Abort { .. },
            ) => true,

            // 反馈请求允许：ProvideFeedback, Abort
            (
                HitlRequestType::Feedback(_),
                HitlResponse::ProvideFeedback { .. } | HitlResponse::Abort { .. },
            ) => true,

            // 暂停请求允许：Resume, Abort
            (HitlRequestType::Pause(_), HitlResponse::Resume | HitlResponse::Abort { .. }) => true,

            _ => false,
        };

        if is_valid {
            Ok(())
        } else {
            Err(HitlError::InvalidResponse(format!(
                "Response type {:?} is not valid for request type",
                response
            )))
        }
    }

    /// 获取有效的超时行为
    ///
    /// 某些超时行为在高风险场景下会被强制修改：
    /// - High/Critical 级别的请求不允许自动批准，会被强制改为 Reject
    ///
    /// # 参数
    /// - `request`: HITL 请求
    ///
    /// # 返回
    /// 经过安全调整的超时行为
    pub fn get_effective_timeout_behavior(request: &HitlRequest) -> TimeoutBehavior {
        let risk_level = request.risk_level().unwrap_or(RiskLevel::Low);
        request.timeout_behavior.effective_for_risk(risk_level)
    }

    /// 验证修改是否安全
    ///
    /// 对于 Modify 响应，验证修改内容是否符合安全要求
    ///
    /// # 参数
    /// - `request`: HITL 请求
    /// - `modifications`: 修改内容
    ///
    /// # 返回
    /// - `Ok(())`: 验证通过
    /// - `Err(HitlError)`: 验证失败
    pub fn validate_modifications(
        request: &HitlRequest,
        modifications: &serde_json::Value,
    ) -> Result<(), HitlError> {
        // 检查是否允许修改
        if let HitlRequestType::Confirmation(conf) = &request.request_type {
            if !conf.allow_modification {
                return Err(HitlError::InvalidResponse(
                    "Modifications are not allowed for this request".to_string(),
                ));
            }

            // 检查修改的字段是否在允许列表中
            if !conf.modifiable_fields.is_empty()
                && let Some(obj) = modifications.as_object()
            {
                for key in obj.keys() {
                    if !conf.modifiable_fields.contains(key) {
                        return Err(HitlError::InvalidResponse(format!(
                            "Field '{}' is not modifiable. Allowed fields: {:?}",
                            key, conf.modifiable_fields
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    /// 完整的响应验证
    ///
    /// 组合所有验证检查
    ///
    /// # 参数
    /// - `request`: HITL 请求
    /// - `user_id`: 响应用户 ID
    /// - `response`: 用户响应
    ///
    /// # 返回
    /// - `Ok(())`: 所有验证通过
    /// - `Err(HitlError)`: 验证失败
    pub fn validate_response(
        request: &HitlRequest,
        user_id: &str,
        response: &HitlResponse,
    ) -> Result<(), HitlError> {
        // 1. 验证权限
        Self::validate_response_permission(request, user_id)?;

        // 2. 验证响应类型
        Self::validate_response_type(&request.request_type, response)?;

        // 3. 如果是修改响应，验证修改内容
        if let HitlResponse::Modify { modifications } = response {
            Self::validate_modifications(request, modifications)?;
        }

        Ok(())
    }
}

/// 响应转状态映射
pub fn response_to_status(response: &HitlResponse) -> HitlRequestStatus {
    match response {
        HitlResponse::Approve | HitlResponse::Resume => HitlRequestStatus::Approved,
        HitlResponse::Reject { .. } => HitlRequestStatus::Rejected,
        HitlResponse::Modify { .. } => HitlRequestStatus::Modified,
        HitlResponse::Abort { .. } => HitlRequestStatus::Cancelled,
        HitlResponse::Clarify { .. } | HitlResponse::ProvideFeedback { .. } => {
            HitlRequestStatus::Completed
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;
    use crate::services::hitl::types::{
        ConfirmationRequest, GenericPreview, OperationPreview, PauseReason, PauseRequest,
    };

    fn create_test_confirmation_request(
        id: &str,
        user_id: &str,
        allow_modification: bool,
    ) -> HitlRequest {
        HitlRequest::new(
            id.to_string(),
            HitlRequestType::Confirmation(Box::new(ConfirmationRequest {
                summary: "Test operation".to_string(),
                risk_level: RiskLevel::High,
                tool_name: "test_tool".to_string(),
                tool_args: json!({}),
                preview: OperationPreview::Generic(GenericPreview {
                    title: "Test".to_string(),
                    description: "Test description".to_string(),
                    details: HashMap::new(),
                }),
                risk_factors: vec![],
                allow_modification,
                modifiable_fields: vec!["path".to_string(), "content".to_string()],
            })),
            "conv1".to_string(),
            user_id.to_string(),
            300,
            TimeoutBehavior::Reject,
        )
    }

    fn create_test_pause_request(id: &str, user_id: &str) -> HitlRequest {
        HitlRequest::new(
            id.to_string(),
            HitlRequestType::Pause(PauseRequest {
                reason: PauseReason::UserRequested,
                current_state: "Running".to_string(),
                completed_steps: vec!["Step 1".to_string()],
                pending_steps: vec!["Step 2".to_string()],
            }),
            "conv1".to_string(),
            user_id.to_string(),
            300,
            TimeoutBehavior::Wait,
        )
    }

    #[test]
    fn test_validate_response_permission_success() {
        let request = create_test_confirmation_request("req1", "user1", false);

        let result = SecurityValidator::validate_response_permission(&request, "user1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_response_permission_wrong_user() {
        let request = create_test_confirmation_request("req1", "user1", false);

        let result = SecurityValidator::validate_response_permission(&request, "user2");
        assert!(matches!(result, Err(HitlError::PermissionDenied(_))));
    }

    #[test]
    fn test_validate_response_permission_not_pending() {
        let mut request = create_test_confirmation_request("req1", "user1", false);
        request.status = HitlRequestStatus::Approved;

        let result = SecurityValidator::validate_response_permission(&request, "user1");
        assert!(matches!(result, Err(HitlError::NotPending(_))));
    }

    #[test]
    fn test_validate_response_permission_expired() {
        let mut request = create_test_confirmation_request("req1", "user1", false);
        request.expires_at = chrono::Utc::now() - chrono::Duration::seconds(10);

        let result = SecurityValidator::validate_response_permission(&request, "user1");
        assert!(matches!(result, Err(HitlError::RequestExpired(_))));
    }

    #[test]
    fn test_validate_response_type_confirmation() {
        let request = create_test_confirmation_request("req1", "user1", false);

        // 有效响应
        assert!(
            SecurityValidator::validate_response_type(
                &request.request_type,
                &HitlResponse::Approve
            )
            .is_ok()
        );
        assert!(
            SecurityValidator::validate_response_type(
                &request.request_type,
                &HitlResponse::Reject { reason: None }
            )
            .is_ok()
        );
        assert!(
            SecurityValidator::validate_response_type(
                &request.request_type,
                &HitlResponse::Modify {
                    modifications: json!({})
                }
            )
            .is_ok()
        );
        assert!(
            SecurityValidator::validate_response_type(
                &request.request_type,
                &HitlResponse::Abort { reason: None }
            )
            .is_ok()
        );

        // 无效响应
        assert!(
            SecurityValidator::validate_response_type(&request.request_type, &HitlResponse::Resume)
                .is_err()
        );
        assert!(
            SecurityValidator::validate_response_type(
                &request.request_type,
                &HitlResponse::Clarify {
                    selected_option: None,
                    input: None
                }
            )
            .is_err()
        );
    }

    #[test]
    fn test_validate_response_type_pause() {
        let request = create_test_pause_request("req1", "user1");

        // 有效响应
        assert!(
            SecurityValidator::validate_response_type(&request.request_type, &HitlResponse::Resume)
                .is_ok()
        );
        assert!(
            SecurityValidator::validate_response_type(
                &request.request_type,
                &HitlResponse::Abort { reason: None }
            )
            .is_ok()
        );

        // 无效响应
        assert!(
            SecurityValidator::validate_response_type(
                &request.request_type,
                &HitlResponse::Approve
            )
            .is_err()
        );
    }

    #[test]
    fn test_validate_modifications_not_allowed() {
        let request = create_test_confirmation_request("req1", "user1", false);

        let result =
            SecurityValidator::validate_modifications(&request, &json!({"path": "/new/path"}));
        assert!(matches!(result, Err(HitlError::InvalidResponse(_))));
    }

    #[test]
    fn test_validate_modifications_allowed_field() {
        let request = create_test_confirmation_request("req1", "user1", true);

        let result =
            SecurityValidator::validate_modifications(&request, &json!({"path": "/new/path"}));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_modifications_disallowed_field() {
        let request = create_test_confirmation_request("req1", "user1", true);

        let result =
            SecurityValidator::validate_modifications(&request, &json!({"unknown_field": "value"}));
        assert!(matches!(result, Err(HitlError::InvalidResponse(_))));
    }

    #[test]
    fn test_get_effective_timeout_behavior() {
        // High 风险请求不允许自动批准
        let mut request = create_test_confirmation_request("req1", "user1", false);
        request.timeout_behavior = TimeoutBehavior::Approve;

        let effective = SecurityValidator::get_effective_timeout_behavior(&request);
        assert_eq!(effective, TimeoutBehavior::Reject);

        // 其他行为不受影响
        request.timeout_behavior = TimeoutBehavior::Skip;
        let effective = SecurityValidator::get_effective_timeout_behavior(&request);
        assert_eq!(effective, TimeoutBehavior::Skip);
    }

    #[test]
    fn test_validate_response_full() {
        let request = create_test_confirmation_request("req1", "user1", false);

        // 完整验证通过
        let result =
            SecurityValidator::validate_response(&request, "user1", &HitlResponse::Approve);
        assert!(result.is_ok());

        // 权限验证失败
        let result =
            SecurityValidator::validate_response(&request, "user2", &HitlResponse::Approve);
        assert!(matches!(result, Err(HitlError::PermissionDenied(_))));

        // 响应类型验证失败
        let result = SecurityValidator::validate_response(&request, "user1", &HitlResponse::Resume);
        assert!(matches!(result, Err(HitlError::InvalidResponse(_))));
    }

    #[test]
    fn test_response_to_status() {
        assert_eq!(
            response_to_status(&HitlResponse::Approve),
            HitlRequestStatus::Approved
        );
        assert_eq!(
            response_to_status(&HitlResponse::Resume),
            HitlRequestStatus::Approved
        );
        assert_eq!(
            response_to_status(&HitlResponse::Reject { reason: None }),
            HitlRequestStatus::Rejected
        );
        assert_eq!(
            response_to_status(&HitlResponse::Modify {
                modifications: json!({})
            }),
            HitlRequestStatus::Modified
        );
        assert_eq!(
            response_to_status(&HitlResponse::Abort { reason: None }),
            HitlRequestStatus::Cancelled
        );
        assert_eq!(
            response_to_status(&HitlResponse::Clarify {
                selected_option: None,
                input: None
            }),
            HitlRequestStatus::Completed
        );
        assert_eq!(
            response_to_status(&HitlResponse::ProvideFeedback {
                rating: None,
                comment: None
            }),
            HitlRequestStatus::Completed
        );
    }
}
