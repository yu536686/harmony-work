# Harness v2、StaffDeck、Harvis 与 UI 融合设计

> 状态：已批准  
> 目标版本：0.9.1  
> 基线：`feat/ui-reference-refresh` (`c34a6cc`)

## 目标

在保留当前无边框 UI、项目式会话、StaffDeck 人员能力和 Harvis 创作流程的前提下，引入 Codex 与 DeepSeek Harness 中成熟的工具运行时设计，减少无意义模型轮次、工具误选、伪工具调用和不可恢复的执行中断。

## 分支与合并策略

新分支 `codex/0.9-harness-v2` 从 `c34a6cc` 建立。将 `276253f` 中已经验证过的单模型入口、运行状态事件、计划去重和局部循环保护合入新分支，而不是以旧 Harness 分支覆盖 UI 分支。

必须保留：

- `design.md` 及 UI Token。
- 无边框窗口和统一侧边栏。
- 显式项目目录与 IM/CLI 系统项目。
- StaffDeck 人员能力模型与人员推荐。
- Harvis 音乐、图片、任务查询、保存和下载流程。
- 会话附件、IM、市场、素材库及现有权限语义。

## Tool Runtime v2

### 工具元数据

每个工具通过统一元数据声明运行属性：

```rust
pub enum ToolExposure {
    Core,
    Deferred,
    Hidden,
}

pub enum ToolExecutionMode {
    Parallel,
    Exclusive,
}

pub enum ToolRisk {
    Read,
    Write,
    External,
}

pub struct ToolMetadata {
    pub category: &'static str,
    pub exposure: ToolExposure,
    pub execution: ToolExecutionMode,
    pub risk: ToolRisk,
    pub timeout_secs: u64,
    pub produces_artifacts: bool,
}
```

现有工具不需要一次性全部修改。`Tool` trait 提供保守默认值，核心、高频工具逐步覆盖元数据。

### 工具曝光

- `Core`：每轮直接暴露，如文件读取、文件搜索、Shell、计划、权限请求、工具检索。
- `Deferred`：只向模型提供名称、类别和最小描述，通过 `tool_search` 搜索后加载完整 schema。
- `Hidden`：仅供宿主或内部流程调用。
- 已经选中的 Skill 所依赖工具可在当前轮临时提升为直接曝光。

该设计取代“把所有启用工具完整发送给模型”的方式，降低请求体积并减少工具混淆。

### 统一结果

工具结果在保留现有文本内容的同时增加结构化字段：

```rust
pub struct ToolArtifact {
    pub kind: String,
    pub name: String,
    pub path: Option<String>,
    pub url: Option<String>,
    pub media_type: Option<String>,
}

pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    pub data: Option<serde_json::Value>,
    pub artifacts: Vec<ToolArtifact>,
}
```

文件、PPT、图片、音乐等产物不再依赖模型从文本中猜测路径。

## Agent Loop v2

- 一个 Step 只进行一次模型请求。
- 模型返回工具调用后，运行时按工具元数据决定并发或独占执行。
- 并行工具可以重叠执行，但结果按模型调用顺序写回。
- `plan_todo` 是进度投影，不是实际工作；相同计划不触发新事件。
- 简单问答不自动创建 Todo，不调用额外意图路由模型。
- 只有网络、限流和暂时性服务错误允许自动重试。
- 循环保护只拒绝重复调用本身，不阻断同一轮其他工具。
- Activity 面板展示阶段、工具调用、批准、错误和产物，不展示隐藏思维链。

## Harvis 融合

保留 `harvis_creative` 兼容入口，同时提供四个语义明确的代理工具：

- `harvis_image_generate`
- `harvis_music_generate`
- `harvis_job_status`
- `harvis_save_result`

这些工具共享现有营销服务和认证配置，不复制 API 客户端。生成工具为外部写操作，需要会话批准；查询为只读并允许并行；保存结果返回结构化 Artifact。

## StaffDeck 融合

StaffDeck 继续作为人员能力、推荐和协作调度层。人员不是独立工具集合的复制品，而是拥有角色说明、能力标签和允许工具范围的执行配置。主 Agent 只有在任务可独立分派时才调用人员，简单任务仍由主 Agent 直接完成。

## UI 融合

严格沿用 `design.md`。Activity、Todo、Tools 和 Artifacts 属于同一个可收起过程面板；Harvis 生成结果同时进入创作页面预览和会话 Artifacts。所有新动效只使用 `opacity`、`transform` 和现有时长 Token，并尊重减少动态效果设置。

## 非目标

- 不移植完整 Codex Core、Cordis 或 DeepSeek 插件系统。
- 不在本版本重写所有现有工具。
- 不向用户展示模型隐藏推理内容。
- 不改变现有登录、素材库、IM、Skill 市场和工作区权限边界。

## 验收标准

- UI、StaffDeck 和 Harvis 现有测试全部通过。
- 重复计划不会阻断后续真实工具。
- 延迟工具可以检索并在下一 Step 使用。
- 只读工具可有序并行，写入和外部工具保持独占。
- Harvis 文生图、音乐、查询和本地保存均可通过会话工具调用。
- 前端生产构建、Rust 测试和 Windows NSIS 打包成功。

