//! Sub-Agent 集成测试
//!
//! 测试 Sub-Agent 工具的完整调用流程，包括：
//! - 工具 schema 正确性
//! - spawn_sub_agent 工具调用
//! - get_sub_agent_result 工具调用
//! - cancel_sub_agent 工具调用
//! - Executor 基本执行流程

use aries::subagent::{
    CANCEL_SUB_AGENT_TOOL, CancelSubAgentArgs, GET_SUB_AGENT_RESULT_TOOL, GetSubAgentResultArgs,
    SPAWN_SUB_AGENT_TOOL, SpawnSubAgentArgs, SubAgent, SubAgentContext, SubAgentManager,
    SubAgentSpawnConfig, SubAgentState, SubAgentSystemConfig, all_subagent_tool_descriptions,
    all_subagent_tool_schemas, is_subagent_tool, parse_subagent_tool_name, subagent_tool_name,
};

// ============================================================================
// Tool Schema Tests
// ============================================================================

#[test]
fn test_subagent_tool_schemas_valid_json() {
    let schemas = all_subagent_tool_schemas();

    assert_eq!(schemas.len(), 3, "Should have 3 Sub-Agent tools");

    for schema in &schemas {
        // Each schema should be a valid JSON object
        assert!(schema.is_object(), "Schema should be a JSON object");

        // Each schema should have "type": "function"
        assert_eq!(
            schema["type"], "function",
            "Schema type should be 'function'"
        );

        // Each schema should have a function object with name and parameters
        let function = &schema["function"];
        assert!(function.is_object(), "Function should be a JSON object");
        assert!(
            function["name"].is_string(),
            "Function should have a name string"
        );
        assert!(
            function["parameters"].is_object(),
            "Function should have parameters object"
        );
    }
}

#[test]
fn test_spawn_sub_agent_schema_structure() {
    let schemas = all_subagent_tool_schemas();
    let spawn_schema = &schemas[0];

    let function = &spawn_schema["function"];
    let name = function["name"].as_str().unwrap();
    assert!(
        name.contains("spawn_sub_agent"),
        "First tool should be spawn_sub_agent"
    );

    // Check required parameters
    let params = &function["parameters"];
    let required = params["required"].as_array().unwrap();
    assert!(
        required.contains(&serde_json::json!("name")),
        "name should be required"
    );
    assert!(
        required.contains(&serde_json::json!("role")),
        "role should be required"
    );
    assert!(
        required.contains(&serde_json::json!("task")),
        "task should be required"
    );

    // Check properties exist
    let properties = &params["properties"];
    assert!(properties["name"].is_object(), "name property should exist");
    assert!(properties["role"].is_object(), "role property should exist");
    assert!(properties["task"].is_object(), "task property should exist");
}

#[test]
fn test_get_sub_agent_result_schema_structure() {
    let schemas = all_subagent_tool_schemas();
    let get_result_schema = &schemas[1];

    let function = &get_result_schema["function"];
    let name = function["name"].as_str().unwrap();
    assert!(
        name.contains("get_sub_agent_result"),
        "Second tool should be get_sub_agent_result"
    );

    // Check required parameters
    let params = &function["parameters"];
    let required = params["required"].as_array().unwrap();
    assert!(
        required.contains(&serde_json::json!("subagent_id")),
        "subagent_id should be required"
    );

    // Check properties exist
    let properties = &params["properties"];
    assert!(
        properties["subagent_id"].is_object(),
        "subagent_id property should exist"
    );
}

#[test]
fn test_cancel_sub_agent_schema_structure() {
    let schemas = all_subagent_tool_schemas();
    let cancel_schema = &schemas[2];

    let function = &cancel_schema["function"];
    let name = function["name"].as_str().unwrap();
    assert!(
        name.contains("cancel_sub_agent"),
        "Third tool should be cancel_sub_agent"
    );

    // Check required parameters
    let params = &function["parameters"];
    let required = params["required"].as_array().unwrap();
    assert!(
        required.contains(&serde_json::json!("subagent_id")),
        "subagent_id should be required"
    );
}

// ============================================================================
// Tool Name Tests
// ============================================================================

#[test]
fn test_tool_name_generation() {
    let spawn_name = subagent_tool_name(SPAWN_SUB_AGENT_TOOL);
    assert_eq!(spawn_name, "internal__spawn_sub_agent");

    let get_result_name = subagent_tool_name(GET_SUB_AGENT_RESULT_TOOL);
    assert_eq!(get_result_name, "internal__get_sub_agent_result");

    let cancel_name = subagent_tool_name(CANCEL_SUB_AGENT_TOOL);
    assert_eq!(cancel_name, "internal__cancel_sub_agent");
}

#[test]
fn test_is_subagent_tool_detection() {
    // Valid Sub-Agent tools
    assert!(is_subagent_tool("internal__spawn_sub_agent"));
    assert!(is_subagent_tool("internal__get_sub_agent_result"));
    assert!(is_subagent_tool("internal__cancel_sub_agent"));

    // Not Sub-Agent tools
    assert!(!is_subagent_tool("internal__skill_run_script"));
    assert!(!is_subagent_tool("mcp__server__tool"));
    assert!(!is_subagent_tool("random_tool"));
}

#[test]
fn test_parse_subagent_tool_name() {
    assert_eq!(
        parse_subagent_tool_name("internal__spawn_sub_agent"),
        Some(SPAWN_SUB_AGENT_TOOL)
    );
    assert_eq!(
        parse_subagent_tool_name("internal__get_sub_agent_result"),
        Some(GET_SUB_AGENT_RESULT_TOOL)
    );
    assert_eq!(
        parse_subagent_tool_name("internal__cancel_sub_agent"),
        Some(CANCEL_SUB_AGENT_TOOL)
    );

    // Invalid names
    assert_eq!(parse_subagent_tool_name("internal__skill_run_script"), None);
    assert_eq!(parse_subagent_tool_name("mcp__server__tool"), None);
}

// ============================================================================
// Tool Arguments Parsing Tests
// ============================================================================

#[test]
fn test_spawn_sub_agent_args_parsing() {
    let json = serde_json::json!({
        "name": "DataAnalyst",
        "role": "You are a data analyst",
        "task": "Analyze the sales data"
    });

    let args: SpawnSubAgentArgs = serde_json::from_value(json).unwrap();
    assert_eq!(args.name, "DataAnalyst");
    assert_eq!(args.role, "You are a data analyst");
    assert_eq!(args.task, "Analyze the sales data");
    assert!(args.allowed_tools.is_none());
    assert!(!args.wait_for_completion);
    assert!(args.timeout_secs.is_none());
    assert!(args.max_iterations.is_none());
}

#[test]
fn test_spawn_sub_agent_args_with_options() {
    let json = serde_json::json!({
        "name": "CodeReviewer",
        "role": "You are a code reviewer",
        "task": "Review the pull request",
        "allowed_tools": ["mcp__github__search_code"],
        "wait_for_completion": true,
        "timeout_secs": 120,
        "max_iterations": 10
    });

    let args: SpawnSubAgentArgs = serde_json::from_value(json).unwrap();
    assert_eq!(args.name, "CodeReviewer");
    assert_eq!(
        args.allowed_tools,
        Some(vec!["mcp__github__search_code".to_string()])
    );
    assert!(args.wait_for_completion);
    assert_eq!(args.timeout_secs, Some(120));
    assert_eq!(args.max_iterations, Some(10));
}

#[test]
fn test_get_sub_agent_result_args_parsing() {
    let json = serde_json::json!({
        "subagent_id": "sa-12345678"
    });

    let args: GetSubAgentResultArgs = serde_json::from_value(json).unwrap();
    assert_eq!(args.subagent_id, "sa-12345678");
    assert!(!args.wait);
    assert!(args.timeout_secs.is_none());
}

#[test]
fn test_get_sub_agent_result_args_with_wait() {
    let json = serde_json::json!({
        "subagent_id": "sa-12345678",
        "wait": true,
        "timeout_secs": 60
    });

    let args: GetSubAgentResultArgs = serde_json::from_value(json).unwrap();
    assert_eq!(args.subagent_id, "sa-12345678");
    assert!(args.wait);
    assert_eq!(args.timeout_secs, Some(60));
}

#[test]
fn test_cancel_sub_agent_args_parsing() {
    let json = serde_json::json!({
        "subagent_id": "sa-12345678"
    });

    let args: CancelSubAgentArgs = serde_json::from_value(json).unwrap();
    assert_eq!(args.subagent_id, "sa-12345678");
    assert!(args.reason.is_none());
}

#[test]
fn test_cancel_sub_agent_args_with_reason() {
    let json = serde_json::json!({
        "subagent_id": "sa-12345678",
        "reason": "User requested cancellation"
    });

    let args: CancelSubAgentArgs = serde_json::from_value(json).unwrap();
    assert_eq!(args.subagent_id, "sa-12345678");
    assert_eq!(args.reason, Some("User requested cancellation".to_string()));
}

// ============================================================================
// Tool Descriptions Tests
// ============================================================================

#[test]
fn test_all_tool_descriptions() {
    let descriptions = all_subagent_tool_descriptions();

    assert_eq!(descriptions.len(), 3, "Should have 3 tool descriptions");

    // Check spawn_sub_agent
    assert!(descriptions[0].name.contains("spawn_sub_agent"));
    assert!(!descriptions[0].description.is_empty());

    // Check get_sub_agent_result
    assert!(descriptions[1].name.contains("get_sub_agent_result"));
    assert!(!descriptions[1].description.is_empty());

    // Check cancel_sub_agent
    assert!(descriptions[2].name.contains("cancel_sub_agent"));
    assert!(!descriptions[2].description.is_empty());
}

// ============================================================================
// SubAgentManager Integration Tests
// ============================================================================

#[tokio::test]
async fn test_manager_spawn_and_get() {
    let config = SubAgentSystemConfig::default_enabled();
    let manager = SubAgentManager::new(config);

    // Spawn a Sub-Agent
    let id = manager
        .spawn(
            "TestAgent",
            "You are a test agent",
            "Complete the test task",
            None,
            None,
        )
        .await
        .expect("Should spawn successfully");

    // Get the Sub-Agent
    let agent = manager.get(&id).await.expect("Should get agent");
    assert_eq!(agent.name, "TestAgent");
    assert_eq!(agent.state, SubAgentState::Pending);
}

#[tokio::test]
async fn test_manager_spawn_with_tool_restrictions() {
    let config = SubAgentSystemConfig::default_enabled();
    let manager = SubAgentManager::new(config);

    // Create spawn config with tool restrictions
    let spawn_config = SubAgentSpawnConfig::new().with_allowed_tools(vec![
        "mcp__github__search_code".to_string(),
        "mcp__github__get_file_contents".to_string(),
    ]);

    let id = manager
        .spawn(
            "RestrictedAgent",
            "You are a restricted agent",
            "Search for code",
            Some(spawn_config),
            None,
        )
        .await
        .expect("Should spawn successfully");

    let agent = manager.get(&id).await.expect("Should get agent");
    assert!(agent.allowed_tools.is_some());
}

#[tokio::test]
async fn test_manager_cancel_agent() {
    let config = SubAgentSystemConfig::default_enabled();
    let manager = SubAgentManager::new(config);

    let id = manager
        .spawn(
            "CancellableAgent",
            "You are a test agent",
            "A long running task",
            None,
            None,
        )
        .await
        .expect("Should spawn successfully");

    // Cancel the agent (works in any non-terminal state)
    manager.cancel(&id).await.expect("Should cancel");

    let agent = manager.get(&id).await.expect("Should get agent");
    assert_eq!(agent.state, SubAgentState::Cancelled);
}

#[tokio::test]
async fn test_manager_list_agents() {
    let config = SubAgentSystemConfig::default_enabled();
    let manager = SubAgentManager::new(config);

    // Spawn multiple agents
    let _id1 = manager
        .spawn("Agent1", "Role 1", "Task 1", None, None)
        .await
        .expect("Should spawn");
    let _id2 = manager
        .spawn("Agent2", "Role 2", "Task 2", None, None)
        .await
        .expect("Should spawn");

    // List all agents
    let agents = manager.list().await;
    assert_eq!(agents.len(), 2);

    // List by state
    let pending = manager.list_by_state(SubAgentState::Pending).await;
    assert_eq!(pending.len(), 2);
}

#[tokio::test]
async fn test_manager_stats() {
    let config = SubAgentSystemConfig::default_enabled();
    let manager = SubAgentManager::new(config);

    // Initial stats
    let stats = manager.stats().await;
    assert_eq!(stats.total, 0);
    assert_eq!(stats.pending, 0);

    // Spawn an agent
    let _id = manager
        .spawn("StatAgent", "Role", "Task", None, None)
        .await
        .expect("Should spawn");

    let stats = manager.stats().await;
    assert_eq!(stats.total, 1);
    assert_eq!(stats.pending, 1);
}

// ============================================================================
// SubAgentContext Integration Tests
// ============================================================================

#[test]
fn test_context_tool_filtering() {
    use aries::subagent::context::ToolInfo;

    let agent = SubAgent::new("FilterAgent", "You filter tools", "Test filtering")
        .with_allowed_tools(
            vec!["tool_a".to_string(), "tool_b".to_string()]
                .into_iter()
                .collect(),
        );

    let ctx = SubAgentContext::from_agent(&agent);

    let tools = vec![
        ToolInfo::new("tool_a", "Allowed"),
        ToolInfo::new("tool_b", "Allowed"),
        ToolInfo::new("tool_c", "Not allowed"),
    ];

    let filtered = ctx.filter_tools(&tools);
    assert_eq!(filtered.len(), 2);
    assert!(filtered.iter().any(|t| t.name == "tool_a"));
    assert!(filtered.iter().any(|t| t.name == "tool_b"));
}

#[test]
fn test_context_message_building() {
    let agent = SubAgent::new(
        "MessageAgent",
        "You are a message builder",
        "Build messages",
    );

    let ctx = SubAgentContext::from_agent(&agent);

    // Should have system and user messages
    assert_eq!(ctx.messages().len(), 2);

    // System message should contain guidelines
    let system_msg = &ctx.messages()[0];
    if let endpoints::chat::ChatCompletionRequestMessage::System(msg) = system_msg {
        let content = msg.content();
        assert!(content.contains("Sub-Agent Guidelines"));
        assert!(content.contains("MessageAgent"));
    } else {
        panic!("First message should be system message");
    }
}

#[test]
fn test_context_iteration_tracking() {
    let agent = SubAgent::new("IterAgent", "Role", "Task");
    let mut ctx = SubAgentContext::from_agent(&agent);

    assert_eq!(ctx.current_iteration(), 0);

    ctx.increment_iteration();
    assert_eq!(ctx.current_iteration(), 1);

    ctx.increment_iteration();
    ctx.increment_iteration();
    assert_eq!(ctx.current_iteration(), 3);
}

// ============================================================================
// SpawnConfig Tests
// ============================================================================

#[test]
fn test_spawn_config_builder() {
    let config = SubAgentSpawnConfig::new()
        .with_timeout(120)
        .with_max_iterations(20);

    assert_eq!(config.timeout_secs, Some(120));
    assert_eq!(config.max_iterations, Some(20));
}

#[test]
fn test_spawn_config_with_only_tools() {
    let config = SubAgentSpawnConfig::new().with_allowed_tools(vec![
        "mcp__server__tool1".to_string(),
        "mcp__server__tool2".to_string(),
    ]);

    let allowed = config
        .tool_access
        .allowed_tools
        .expect("Should have allowed tools");
    assert_eq!(allowed.len(), 2);
    assert!(allowed.contains("mcp__server__tool1"));
    assert!(allowed.contains("mcp__server__tool2"));
}
