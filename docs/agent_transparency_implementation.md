# Aries 前端执行透明度 (Execution Transparency) 实现方案

本方案旨在解决 Agent 执行过程中的“黑盒”问题，通过可视化“思维链”和“工具调用”，增强用户对 Agent 协作过程的信任感和控制力。

## 1. 核心交互逻辑

设计原则：**“简约即美，详情可选”**。
- **默认状态**：当 Agent 开始处理任务时，在对话框下方或中间显示简洁的进度状态（例如：“正在规划逻辑...”）。
- **展开状态**：用户可点击进度条，展开完整的 **“执行日志”**，查看具体的思考过程和工具调用细节。

## 2. 数据结构设计 (Zustand Store)

我们需要在 `frontend/src/store.ts` 中引入一个新的 `steps` 状态，用于追踪当前会话的执行流。

```typescript
interface ExecutionStep {
  id: string;
  type: 'thought' | 'tool_call' | 'tool_result' | 'status';
  content?: string;
  toolName?: string;
  args?: any;
  result?: any;
  status: 'running' | 'completed' | 'error';
  timestamp: number;
}

interface AriesState {
  // ... 其他状态
  currentSteps: ExecutionStep[]; // 当前正在处理的消息关联的步骤
  addStep: (step: Partial<ExecutionStep>) => void;
  updateStep: (id: string, updates: Partial<ExecutionStep>) => void;
}
```

## 3. SSE 事件解析桥接

利用后端提供的 `Structured Events` SSE 协议，前端流式解析逻辑将按如下方式映射：

| 后端事件 | 前端行为 |
| :--- | :--- |
| `event: status` | 创建一个 `type: status` 的步骤，更新全局状态栏。 |
| `event: thought` | 创建或追加一个 `type: thought` 的步骤内容。 |
| `event: tool_call` | 创建一个 `type: tool_call` 步骤，显示“正在准备调用工具 [X]”及其参数。 |
| `event: tool_result`| 找到对应的 `tool_call` 步骤，将其 `status` 设为 `completed`，并挂载 `result`。 |
| `event: text` | 正常渲染主对话气泡内容。 |

## 4. UI 组件设计

### 4.1 `ThoughtChain` (执行逻辑容器)
- **位置**：位于用户消息和正在生成的回复之间。
- **视觉风格**：细灰线连接的垂直时间轴，整体背景微透明 (Neutral 900/50)。
- **动效**：正在进行的步骤旁边有一个微小的旋转 Loading 环。

### 4.2 `ToolCallCard` (工具调用细节)
- **样式**：采用类似代码块的小卡片形式。
- **内容**：展示工具名、简化的 JSON 参数清单。如果执行成功，显示一个绿色的对勾；如果失败，显示红色告警及错误信息。

### 4.3 状态栏 (Minimal Status Bar)
- **位置**：紧贴输入框上方。
- **交互**：显示“Agent 正在 [执行操作...] (查看详情)”，点击“(查看详情)”即滚动至 `ThoughtChain` 区域。

## 5. 实施步骤 (Phase 6)

1.  **基础设施**：在 `store.ts` 中实现步骤管理逻辑。
2.  **协议对接**：重构 `sendMessage` 函数，引入 SSE `EventSource` 或基于 `fetch` 的流式解析器（支持 `event` 字段提取）。
3.  **骨架屏与组件**：开发 `ExecutionChain` 基础组件及其子组件（`StepItem`, `ThoughtBlock`）。
4.  **动效优化**：添加步骤渐入渐出的过渡动画，确保从“思维链”到“最终回答”的转场平滑。

## 6. 验证指标
- **可见性**：用户在 Agent 搜索或读文件时，能清楚看到具体是哪个环节在耗时。
- **调试力**：当 Agent 由于工具参数错误停止时，用户能在 UI 上看到报错的 JSON 载荷，而不是无响应。
