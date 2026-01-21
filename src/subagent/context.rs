//! Sub-Agent 上下文管理
//!
//! 此模块实现 SubAgentContext，为每个 Sub-Agent 提供独立的执行上下文。

use std::collections::HashSet;

use endpoints::chat::{
    ChatCompletionAssistantMessage, ChatCompletionRequestMessage, ChatCompletionSystemMessage,
    ChatCompletionToolMessage, ChatCompletionUserMessage, ChatCompletionUserMessageContent,
};

use super::{SubAgent, SubAgentId};

// ============================================================================
// SubAgentContext
// ============================================================================

/// Sub-Agent 执行上下文
///
/// 为 Sub-Agent 提供独立的消息历史和工具访问控制。
/// 每个 Sub-Agent 都有自己的上下文，与主 Agent 和其他 Sub-Agent 隔离。
#[derive(Debug, Clone)]
pub struct SubAgentContext {
    /// Sub-Agent ID
    pub id: SubAgentId,

    /// Sub-Agent 名称
    pub name: String,

    /// 系统提示词
    pub system_prompt: String,

    /// 任务描述
    pub task: String,

    /// 消息历史
    messages: Vec<ChatCompletionRequestMessage>,

    /// 允许的工具集合（None 表示允许所有）
    allowed_tools: Option<HashSet<String>>,

    /// 禁止的工具集合
    blocked_tools: HashSet<String>,

    /// 嵌套深度
    pub depth: u32,

    /// 父 Agent ID（如果有）
    pub parent_id: Option<SubAgentId>,

    /// 当前迭代次数
    pub iteration_count: u32,
}

impl SubAgentContext {
    /// 从 SubAgent 创建上下文
    pub fn from_agent(agent: &SubAgent) -> Self {
        let mut ctx = Self {
            id: agent.id.clone(),
            name: agent.name.clone(),
            system_prompt: agent.system_prompt.clone(),
            task: agent.task.clone(),
            messages: Vec::new(),
            allowed_tools: agent.allowed_tools.clone(),
            blocked_tools: agent.blocked_tools.clone(),
            depth: agent.depth,
            parent_id: agent.parent_id.clone(),
            iteration_count: 0,
        };

        // 初始化消息
        ctx.initialize_messages();
        ctx
    }

    /// 创建新的上下文
    pub fn new(
        id: SubAgentId,
        name: impl Into<String>,
        system_prompt: impl Into<String>,
        task: impl Into<String>,
    ) -> Self {
        let mut ctx = Self {
            id,
            name: name.into(),
            system_prompt: system_prompt.into(),
            task: task.into(),
            messages: Vec::new(),
            allowed_tools: None,
            blocked_tools: HashSet::new(),
            depth: 0,
            parent_id: None,
            iteration_count: 0,
        };

        ctx.initialize_messages();
        ctx
    }

    /// 初始化消息列表（系统消息 + 用户任务）
    fn initialize_messages(&mut self) {
        self.messages.clear();

        // 添加系统消息
        let system_content = self.build_system_message();
        self.messages.push(ChatCompletionRequestMessage::System(
            ChatCompletionSystemMessage::new(system_content, None),
        ));

        // 添加用户任务消息
        let user_content = self.build_user_message();
        self.messages.push(ChatCompletionRequestMessage::User(
            ChatCompletionUserMessage::new(
                ChatCompletionUserMessageContent::Text(user_content),
                None,
            ),
        ));
    }

    /// 构建系统消息内容
    fn build_system_message(&self) -> String {
        let mut content = self.system_prompt.clone();

        // 添加 Sub-Agent 特定指令
        content.push_str("\n\n## Sub-Agent Guidelines\n\n");
        content.push_str("You are operating as a Sub-Agent with the following characteristics:\n");
        content.push_str(&format!("- **Name**: {}\n", self.name));
        content.push_str(&format!("- **Depth Level**: {}\n", self.depth));
        content.push_str("\n### Important Instructions:\n");
        content.push_str("1. Focus solely on the assigned task.\n");
        content.push_str("2. Use available tools efficiently to complete the task.\n");
        content.push_str("3. Provide a clear, concise final answer when the task is complete.\n");
        content.push_str("4. If you cannot complete the task, explain the blockers clearly.\n");
        content.push_str("5. Do not attempt tasks outside your assigned scope.\n");

        content
    }

    /// 构建用户消息内容
    fn build_user_message(&self) -> String {
        format!(
            "## Task\n\n{}\n\nPlease complete this task step by step, using available tools as needed. \
             When you have completed the task, provide your final answer clearly.",
            self.task
        )
    }

    /// 设置允许的工具
    pub fn with_allowed_tools(mut self, tools: HashSet<String>) -> Self {
        self.allowed_tools = Some(tools);
        self
    }

    /// 设置禁止的工具
    pub fn with_blocked_tools(mut self, tools: HashSet<String>) -> Self {
        self.blocked_tools = tools;
        self
    }

    /// 设置嵌套深度
    pub fn with_depth(mut self, depth: u32) -> Self {
        self.depth = depth;
        self
    }

    /// 设置父 Agent ID
    pub fn with_parent(mut self, parent_id: SubAgentId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// 检查工具是否允许使用
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        // 首先检查是否被禁止
        if self.blocked_tools.contains(tool_name) {
            return false;
        }

        // 如果有允许列表，检查是否在列表中
        if let Some(allowed) = &self.allowed_tools {
            return allowed.contains(tool_name);
        }

        // 默认允许
        true
    }

    /// 过滤工具列表，返回允许的工具
    pub fn filter_tools<'a>(&self, tools: &'a [ToolInfo]) -> Vec<&'a ToolInfo> {
        tools
            .iter()
            .filter(|t| self.is_tool_allowed(&t.name))
            .collect()
    }

    /// 获取消息列表引用
    pub fn messages(&self) -> &[ChatCompletionRequestMessage] {
        &self.messages
    }

    /// 获取可变消息列表引用
    pub fn messages_mut(&mut self) -> &mut Vec<ChatCompletionRequestMessage> {
        &mut self.messages
    }

    /// 添加助手消息
    pub fn add_assistant_message(
        &mut self,
        content: Option<String>,
        tool_calls: Option<Vec<endpoints::chat::ToolCall>>,
    ) {
        self.messages.push(ChatCompletionRequestMessage::Assistant(
            ChatCompletionAssistantMessage::new(content, None, tool_calls),
        ));
    }

    /// 添加工具结果消息
    pub fn add_tool_result(&mut self, content: &str, tool_call_id: &str) {
        self.messages.push(ChatCompletionRequestMessage::Tool(
            ChatCompletionToolMessage::new(content, tool_call_id),
        ));
    }

    /// 添加用户消息
    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(ChatCompletionRequestMessage::User(
            ChatCompletionUserMessage::new(
                ChatCompletionUserMessageContent::Text(content.to_string()),
                None,
            ),
        ));
    }

    /// 递增迭代计数
    pub fn increment_iteration(&mut self) {
        self.iteration_count += 1;
    }

    /// 获取当前迭代次数
    pub fn current_iteration(&self) -> u32 {
        self.iteration_count
    }

    /// 重置上下文（重新初始化消息）
    pub fn reset(&mut self) {
        self.iteration_count = 0;
        self.initialize_messages();
    }

    /// 获取消息数量
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
}

// ============================================================================
// ToolInfo - 简单工具信息（用于过滤）
// ============================================================================

/// 工具信息（用于上下文过滤）
#[derive(Debug, Clone)]
pub struct ToolInfo {
    /// 工具名称
    pub name: String,
    /// 工具描述
    pub description: String,
}

impl ToolInfo {
    /// 创建新的工具信息
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context() -> SubAgentContext {
        SubAgentContext::new(
            SubAgentId::new(),
            "TestAgent",
            "You are a test assistant.",
            "Complete the test task.",
        )
    }

    #[test]
    fn test_context_creation() {
        let ctx = create_test_context();

        assert_eq!(ctx.name, "TestAgent");
        assert_eq!(ctx.task, "Complete the test task.");
        assert_eq!(ctx.depth, 0);
        assert!(ctx.parent_id.is_none());
        assert_eq!(ctx.iteration_count, 0);
    }

    #[test]
    fn test_context_initial_messages() {
        let ctx = create_test_context();

        // 应该有系统消息和用户消息
        assert_eq!(ctx.messages.len(), 2);

        // 检查系统消息
        match &ctx.messages[0] {
            ChatCompletionRequestMessage::System(msg) => {
                assert!(msg.content().contains("You are a test assistant"));
                assert!(msg.content().contains("Sub-Agent Guidelines"));
            }
            _ => panic!("First message should be system message"),
        }

        // 检查用户消息
        match &ctx.messages[1] {
            ChatCompletionRequestMessage::User(msg) => {
                // UserMessage content is an enum, get text
                let content_str = format!("{:?}", msg.content());
                assert!(content_str.contains("Complete the test task"));
            }
            _ => panic!("Second message should be user message"),
        }
    }

    #[test]
    fn test_tool_filtering_no_restrictions() {
        let ctx = create_test_context();

        assert!(ctx.is_tool_allowed("any_tool"));
        assert!(ctx.is_tool_allowed("mcp__server__tool"));
    }

    #[test]
    fn test_tool_filtering_with_allowed_list() {
        let mut allowed = HashSet::new();
        allowed.insert("tool_a".to_string());
        allowed.insert("tool_b".to_string());

        let ctx = create_test_context().with_allowed_tools(allowed);

        assert!(ctx.is_tool_allowed("tool_a"));
        assert!(ctx.is_tool_allowed("tool_b"));
        assert!(!ctx.is_tool_allowed("tool_c"));
    }

    #[test]
    fn test_tool_filtering_with_blocked_list() {
        let mut blocked = HashSet::new();
        blocked.insert("dangerous_tool".to_string());

        let ctx = create_test_context().with_blocked_tools(blocked);

        assert!(ctx.is_tool_allowed("safe_tool"));
        assert!(!ctx.is_tool_allowed("dangerous_tool"));
    }

    #[test]
    fn test_tool_filtering_blocked_overrides_allowed() {
        let mut allowed = HashSet::new();
        allowed.insert("tool_a".to_string());
        allowed.insert("tool_b".to_string());

        let mut blocked = HashSet::new();
        blocked.insert("tool_a".to_string());

        let ctx = create_test_context()
            .with_allowed_tools(allowed)
            .with_blocked_tools(blocked);

        // tool_a is in allowed but also blocked, should be blocked
        assert!(!ctx.is_tool_allowed("tool_a"));
        assert!(ctx.is_tool_allowed("tool_b"));
    }

    #[test]
    fn test_filter_tools() {
        let mut allowed = HashSet::new();
        allowed.insert("tool_a".to_string());

        let ctx = create_test_context().with_allowed_tools(allowed);

        let tools = vec![
            ToolInfo::new("tool_a", "Allowed tool"),
            ToolInfo::new("tool_b", "Not allowed"),
            ToolInfo::new("tool_c", "Not allowed"),
        ];

        let filtered = ctx.filter_tools(&tools);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "tool_a");
    }

    #[test]
    fn test_add_messages() {
        let mut ctx = create_test_context();
        let initial_count = ctx.message_count();

        // 添加助手消息
        ctx.add_assistant_message(Some("I will help you.".to_string()), None);
        assert_eq!(ctx.message_count(), initial_count + 1);

        // 添加工具结果
        ctx.add_tool_result("Tool output", "call_123");
        assert_eq!(ctx.message_count(), initial_count + 2);

        // 添加用户消息
        ctx.add_user_message("Thanks!");
        assert_eq!(ctx.message_count(), initial_count + 3);
    }

    #[test]
    fn test_iteration_count() {
        let mut ctx = create_test_context();

        assert_eq!(ctx.current_iteration(), 0);

        ctx.increment_iteration();
        assert_eq!(ctx.current_iteration(), 1);

        ctx.increment_iteration();
        ctx.increment_iteration();
        assert_eq!(ctx.current_iteration(), 3);
    }

    #[test]
    fn test_context_reset() {
        let mut ctx = create_test_context();

        // 添加一些消息和迭代
        ctx.add_assistant_message(Some("Hello".to_string()), None);
        ctx.increment_iteration();
        ctx.increment_iteration();

        assert!(ctx.message_count() > 2);
        assert!(ctx.current_iteration() > 0);

        // 重置
        ctx.reset();

        assert_eq!(ctx.message_count(), 2); // 只有系统和用户消息
        assert_eq!(ctx.current_iteration(), 0);
    }

    #[test]
    fn test_context_with_depth_and_parent() {
        let parent_id = SubAgentId::new();
        let ctx = create_test_context()
            .with_depth(2)
            .with_parent(parent_id.clone());

        assert_eq!(ctx.depth, 2);
        assert_eq!(ctx.parent_id, Some(parent_id));
    }

    #[test]
    fn test_context_from_agent() {
        let mut agent = SubAgent::new("AgentFromStruct", "System prompt here", "Task description");
        agent = agent.with_allowed_tools(vec!["tool_x".to_string()].into_iter().collect());

        let ctx = SubAgentContext::from_agent(&agent);

        assert_eq!(ctx.id, agent.id);
        assert_eq!(ctx.name, "AgentFromStruct");
        assert_eq!(ctx.system_prompt, "System prompt here");
        assert_eq!(ctx.task, "Task description");
        assert!(ctx.is_tool_allowed("tool_x"));
        assert!(!ctx.is_tool_allowed("tool_y"));
    }

    #[test]
    fn test_system_message_includes_guidelines() {
        let ctx = SubAgentContext::new(
            SubAgentId::new(),
            "DataAnalyst",
            "You analyze data.",
            "Analyze sales data",
        )
        .with_depth(1);

        match &ctx.messages[0] {
            ChatCompletionRequestMessage::System(msg) => {
                let content = msg.content();
                assert!(
                    content.contains("Sub-Agent Guidelines"),
                    "Missing 'Sub-Agent Guidelines' in: {}",
                    content
                );
                assert!(
                    content.contains("DataAnalyst"),
                    "Missing 'DataAnalyst' in: {}",
                    content
                );
                // Check for depth info (format may vary)
                assert!(
                    content.contains("Depth") && content.contains("1"),
                    "Missing depth info in: {}",
                    content
                );
            }
            _ => panic!("Expected system message"),
        }
    }
}
