//! HITL Pending Store
//!
//! 内存存储待处理的 HITL 请求，使用 DashMap 实现并发安全。

use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::Utc;
use dashmap::DashMap;

use super::types::{HitlError, HitlRequest, HitlRequestId, HitlRequestStatus};

/// Pending Store - 存储待处理的 HITL 请求
///
/// 使用内存存储，因为：
/// 1. HITL 请求生命周期短暂（通常 30 秒 - 5 分钟）
/// 2. 服务重启后，关联的 React Loop 也会中断，请求本身失去意义
/// 3. 内存访问速度快，满足实时交互需求
#[derive(Debug)]
pub struct PendingStore {
    /// 主存储：请求 ID -> 请求
    requests: DashMap<HitlRequestId, HitlRequest>,
    /// 按 conversation_id 索引
    by_conversation: DashMap<String, Vec<HitlRequestId>>,
    /// 按 user_id 索引
    by_user: DashMap<String, Vec<HitlRequestId>>,
    /// 当前待处理请求数
    pending_count: AtomicUsize,
    /// 最大待处理请求数
    max_pending: usize,
}

impl PendingStore {
    /// 创建新的 Pending Store
    pub fn new(max_pending: usize) -> Self {
        Self {
            requests: DashMap::new(),
            by_conversation: DashMap::new(),
            by_user: DashMap::new(),
            pending_count: AtomicUsize::new(0),
            max_pending,
        }
    }

    /// 插入新请求
    pub fn insert(&self, request: HitlRequest) -> Result<(), HitlError> {
        // 检查是否达到最大限制
        if self.pending_count.load(Ordering::Relaxed) >= self.max_pending {
            return Err(HitlError::StoreError(format!(
                "Pending store is full (max: {})",
                self.max_pending
            )));
        }

        let id = request.id.clone();
        let conversation_id = request.conversation_id.clone();
        let user_id = request.user_id.clone();

        // 插入主存储
        if self.requests.insert(id.clone(), request).is_some() {
            return Err(HitlError::StoreError(format!(
                "Request already exists: {}",
                id
            )));
        }

        // 更新索引
        self.by_conversation
            .entry(conversation_id)
            .or_default()
            .push(id.clone());

        self.by_user.entry(user_id).or_default().push(id);

        // 增加计数
        self.pending_count.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// 获取请求
    pub fn get(&self, id: &str) -> Option<HitlRequest> {
        self.requests.get(id).map(|r| r.clone())
    }

    /// 检查请求是否存在
    pub fn contains(&self, id: &str) -> bool {
        self.requests.contains_key(id)
    }

    /// 更新请求
    pub fn update<F>(&self, id: &str, f: F) -> Result<HitlRequest, HitlError>
    where
        F: FnOnce(&mut HitlRequest),
    {
        let mut entry = self
            .requests
            .get_mut(id)
            .ok_or_else(|| HitlError::RequestNotFound(id.to_string()))?;

        f(&mut entry);
        entry.updated_at = Utc::now();

        Ok(entry.clone())
    }

    /// 更新请求状态
    pub fn update_status(
        &self,
        id: &str,
        status: HitlRequestStatus,
    ) -> Result<HitlRequest, HitlError> {
        self.update(id, |r| {
            r.status = status;
        })
    }

    /// 移除请求
    pub fn remove(&self, id: &str) -> Option<HitlRequest> {
        let (_, request) = self.requests.remove(id)?;

        // 更新索引
        self.remove_from_index(&self.by_conversation, &request.conversation_id, id);
        self.remove_from_index(&self.by_user, &request.user_id, id);

        // 如果是待处理状态，减少计数
        if request.status == HitlRequestStatus::Pending {
            self.pending_count.fetch_sub(1, Ordering::Relaxed);
        }

        Some(request)
    }

    /// 从索引中移除
    fn remove_from_index(&self, index: &DashMap<String, Vec<HitlRequestId>>, key: &str, id: &str) {
        if let Some(mut ids) = index.get_mut(key) {
            ids.retain(|i| i != id);
        }
    }

    /// 获取指定对话的待处理请求
    pub fn get_pending_by_conversation(&self, conversation_id: &str) -> Vec<HitlRequest> {
        self.by_conversation
            .get(conversation_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| {
                        self.requests.get(id).and_then(|r| {
                            if r.status == HitlRequestStatus::Pending {
                                Some(r.clone())
                            } else {
                                None
                            }
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 获取指定用户的待处理请求
    pub fn get_pending_by_user(&self, user_id: &str) -> Vec<HitlRequest> {
        self.by_user
            .get(user_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| {
                        self.requests.get(id).and_then(|r| {
                            if r.status == HitlRequestStatus::Pending {
                                Some(r.clone())
                            } else {
                                None
                            }
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 获取所有待处理请求
    pub fn get_all_pending(&self) -> Vec<HitlRequest> {
        self.requests
            .iter()
            .filter(|r| r.status == HitlRequestStatus::Pending)
            .map(|r| r.clone())
            .collect()
    }

    /// 获取所有已过期的待处理请求
    pub fn get_expired(&self) -> Vec<HitlRequest> {
        let now = Utc::now();
        self.requests
            .iter()
            .filter(|r| r.status == HitlRequestStatus::Pending && r.expires_at < now)
            .map(|r| r.clone())
            .collect()
    }

    /// 获取即将过期的请求（用于发送警告）
    pub fn get_expiring_soon(&self, warning_secs: u64) -> Vec<HitlRequest> {
        let now = Utc::now();
        let warning_threshold = now + chrono::Duration::seconds(warning_secs as i64);

        self.requests
            .iter()
            .filter(|r| {
                r.status == HitlRequestStatus::Pending
                    && r.expires_at > now
                    && r.expires_at <= warning_threshold
            })
            .map(|r| r.clone())
            .collect()
    }

    /// 获取待处理请求数
    pub fn count_pending(&self) -> usize {
        self.pending_count.load(Ordering::Relaxed)
    }

    /// 获取总请求数
    pub fn count_total(&self) -> usize {
        self.requests.len()
    }

    /// 清理已完成/已取消的请求
    pub fn cleanup_completed(&self) -> usize {
        let to_remove: Vec<_> = self
            .requests
            .iter()
            .filter(|r| r.status.is_terminal())
            .map(|r| r.id.clone())
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            self.remove(&id);
        }

        count
    }

    /// 清空所有请求
    pub fn clear(&self) {
        self.requests.clear();
        self.by_conversation.clear();
        self.by_user.clear();
        self.pending_count.store(0, Ordering::Relaxed);
    }
}

impl Default for PendingStore {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::hitl::types::{
        ConfirmationRequest, GenericPreview, HitlRequestType, OperationPreview, RiskLevel,
        TimeoutBehavior,
    };

    fn create_test_request(id: &str, conversation_id: &str, user_id: &str) -> HitlRequest {
        HitlRequest::new(
            id.to_string(),
            HitlRequestType::Confirmation(Box::new(ConfirmationRequest {
                summary: "Test operation".to_string(),
                risk_level: RiskLevel::High,
                tool_name: "test_tool".to_string(),
                tool_args: serde_json::json!({}),
                preview: OperationPreview::Generic(GenericPreview {
                    title: "Test".to_string(),
                    description: "Test description".to_string(),
                    details: Default::default(),
                }),
                risk_factors: vec![],
                allow_modification: false,
                modifiable_fields: vec![],
            })),
            conversation_id.to_string(),
            user_id.to_string(),
            300,
            TimeoutBehavior::Reject,
        )
    }

    #[test]
    fn test_insert_and_get() {
        let store = PendingStore::new(10);
        let request = create_test_request("req1", "conv1", "user1");

        store.insert(request.clone()).unwrap();

        let retrieved = store.get("req1").unwrap();
        assert_eq!(retrieved.id, "req1");
        assert_eq!(retrieved.conversation_id, "conv1");
        assert_eq!(retrieved.user_id, "user1");
    }

    #[test]
    fn test_insert_duplicate() {
        let store = PendingStore::new(10);
        let request1 = create_test_request("req1", "conv1", "user1");
        let request2 = create_test_request("req1", "conv2", "user2");

        store.insert(request1).unwrap();
        let result = store.insert(request2);

        assert!(result.is_err());
    }

    #[test]
    fn test_max_pending_limit() {
        let store = PendingStore::new(2);

        store
            .insert(create_test_request("req1", "conv1", "user1"))
            .unwrap();
        store
            .insert(create_test_request("req2", "conv1", "user1"))
            .unwrap();

        let result = store.insert(create_test_request("req3", "conv1", "user1"));
        assert!(result.is_err());
    }

    #[test]
    fn test_update() {
        let store = PendingStore::new(10);
        store
            .insert(create_test_request("req1", "conv1", "user1"))
            .unwrap();

        let updated = store
            .update("req1", |r| {
                r.status = HitlRequestStatus::Approved;
            })
            .unwrap();

        assert_eq!(updated.status, HitlRequestStatus::Approved);
    }

    #[test]
    fn test_update_not_found() {
        let store = PendingStore::new(10);

        let result = store.update("nonexistent", |_| {});
        assert!(result.is_err());
    }

    #[test]
    fn test_remove() {
        let store = PendingStore::new(10);
        store
            .insert(create_test_request("req1", "conv1", "user1"))
            .unwrap();

        assert_eq!(store.count_pending(), 1);

        let removed = store.remove("req1").unwrap();
        assert_eq!(removed.id, "req1");
        assert_eq!(store.count_pending(), 0);
        assert!(store.get("req1").is_none());
    }

    #[test]
    fn test_get_pending_by_conversation() {
        let store = PendingStore::new(10);

        store
            .insert(create_test_request("req1", "conv1", "user1"))
            .unwrap();
        store
            .insert(create_test_request("req2", "conv1", "user1"))
            .unwrap();
        store
            .insert(create_test_request("req3", "conv2", "user1"))
            .unwrap();

        let pending = store.get_pending_by_conversation("conv1");
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn test_get_pending_by_user() {
        let store = PendingStore::new(10);

        store
            .insert(create_test_request("req1", "conv1", "user1"))
            .unwrap();
        store
            .insert(create_test_request("req2", "conv2", "user1"))
            .unwrap();
        store
            .insert(create_test_request("req3", "conv1", "user2"))
            .unwrap();

        let pending = store.get_pending_by_user("user1");
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn test_get_expired() {
        let store = PendingStore::new(10);

        // 创建一个已过期的请求
        let mut request = create_test_request("req1", "conv1", "user1");
        request.expires_at = Utc::now() - chrono::Duration::seconds(10);
        store.insert(request).unwrap();

        // 创建一个未过期的请求
        store
            .insert(create_test_request("req2", "conv1", "user1"))
            .unwrap();

        let expired = store.get_expired();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].id, "req1");
    }

    #[test]
    fn test_cleanup_completed() {
        let store = PendingStore::new(10);

        store
            .insert(create_test_request("req1", "conv1", "user1"))
            .unwrap();
        store
            .insert(create_test_request("req2", "conv1", "user1"))
            .unwrap();

        // 标记一个为完成
        store
            .update("req1", |r| {
                r.status = HitlRequestStatus::Completed;
            })
            .unwrap();

        let cleaned = store.cleanup_completed();
        assert_eq!(cleaned, 1);
        assert_eq!(store.count_total(), 1);
    }

    #[test]
    fn test_count() {
        let store = PendingStore::new(10);

        store
            .insert(create_test_request("req1", "conv1", "user1"))
            .unwrap();
        store
            .insert(create_test_request("req2", "conv1", "user1"))
            .unwrap();

        assert_eq!(store.count_pending(), 2);
        assert_eq!(store.count_total(), 2);

        // 标记一个为完成
        store
            .update("req1", |r| {
                r.status = HitlRequestStatus::Completed;
            })
            .unwrap();

        // pending_count 不会自动更新，只有 remove 时才会更新
        assert_eq!(store.count_total(), 2);
    }

    #[test]
    fn test_clear() {
        let store = PendingStore::new(10);

        store
            .insert(create_test_request("req1", "conv1", "user1"))
            .unwrap();
        store
            .insert(create_test_request("req2", "conv1", "user1"))
            .unwrap();

        store.clear();

        assert_eq!(store.count_pending(), 0);
        assert_eq!(store.count_total(), 0);
    }
}
