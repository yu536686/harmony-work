# Harvis AI 创作模块实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 在 WorkBuddy 内交付受登录权限保护的 Harvis 音乐和图片创作页面，并通过营销工作台网关安全调用 Suno V5.5 和 GPT Image2。

**架构：** 桌面客户端调用营销工作台的 Creative Gateway；Gateway 持有供应商令牌、维护异步任务并将成功产物入库至 NAS。WorkBuddy 使用既有营销登录态和 Tauri HTTP 代理，页面和系统 Skill 只消费内部任务 API。

**技术栈：** React 19、TypeScript、Tauri/Rust、Express 5、PostgreSQL、Node fetch、multer、NAS `/mnt/harmony`。

---

## 文件结构

- 创建：`/Users/yutianqi/Documents/亨吉利/src/server/creative.ts`：供应商适配、任务状态、轮询、产物归档。
- 修改：`/Users/yutianqi/Documents/亨吉利/src/server/config.ts`：Harvis 服务端机密配置。
- 修改：`/Users/yutianqi/Documents/亨吉利/src/server/db.ts`：创作任务、产物表与音频素材约束迁移。
- 修改：`/Users/yutianqi/Documents/亨吉利/src/server/index.ts`：经认证的创作 API、上传和回调入口。
- 创建：`/Users/yutianqi/Documents/亨吉利/src/server/creative.test.ts`：请求构建、比例映射和状态归一化测试。
- 创建：`E:/xiaoheng-workbuddy-staffdeck-agpl/src/services/tauri/creative.ts`：客户端创作 API 类型与调用。
- 创建：`E:/xiaoheng-workbuddy-staffdeck-agpl/src/components/Creative/index.tsx`：音乐、图片、任务和产物 UI。
- 创建：`E:/xiaoheng-workbuddy-staffdeck-agpl/src/components/Creative/Creative.css`：创作页样式和过渡。
- 修改：`E:/xiaoheng-workbuddy-staffdeck-agpl/src/App.tsx`：导航、登录门禁和懒加载页面。
- 修改：`E:/xiaoheng-workbuddy-staffdeck-agpl/src/services/tauri/marketing.ts`：原始任务 API 和登录令牌桥接。
- 修改：`E:/xiaoheng-workbuddy-staffdeck-agpl/src-tauri/src/marketing.rs`：认证转发、二进制上传和创作端点。
- 创建：`E:/xiaoheng-workbuddy-staffdeck-agpl/third_party/piscis-engine/system-skills/harvis-creative/SKILL.md`：系统技能说明。
- 修改：`E:/xiaoheng-workbuddy-staffdeck-agpl/third_party/piscis-engine/...`：注册受控 Harvis 工具。

### 任务 1：创作域的纯函数与失败测试

**文件：**
- 创建：`/Users/yutianqi/Documents/亨吉利/src/server/creative.test.ts`
- 创建：`/Users/yutianqi/Documents/亨吉利/src/server/creative.ts`

- [ ] **步骤 1：编写失败测试**

```ts
import { mapCreativeCanvas, normalizeMusicState } from "./creative.js";

test("maps a store poster to a supported supplier ratio and exact archive canvas", () => {
  expect(mapCreativeCanvas({ preset: "store-poster" })).toEqual({
    aspectRatio: "3:4", outputWidth: 420, outputHeight: 592
  });
});

test("maps completed Suno items to succeeded artifacts", () => {
  expect(normalizeMusicState({ status: 30, progress: 100, cld2AudioUrl: "https://a" }).status).toBe("succeeded");
});
```

- [ ] **步骤 2：运行测试验证失败**

运行：`node --test --import tsx src/server/creative.test.ts`

预期：FAIL，因 `creative.ts` 尚不存在。

- [ ] **步骤 3：实现最少纯函数**

```ts
export function mapCreativeCanvas(input: { preset: string; width?: number; height?: number }) {
  if (input.preset === "store-poster") return { aspectRatio: "3:4", outputWidth: 420, outputHeight: 592 };
  return { aspectRatio: "16:9", outputWidth: null, outputHeight: null };
}
```

- [ ] **步骤 4：运行测试验证通过**

运行：`node --test --import tsx src/server/creative.test.ts`

预期：所有创作纯函数测试通过。

- [ ] **步骤 5：提交**

后端代码在服务器工作目录内，提交 WorkBuddy 中的接口契约与测试变更：
`git commit -m "feat: add Harvis creative contract"`。

### 任务 2：Gateway 数据库、供应商适配和归档

**文件：**
- 修改：`/Users/yutianqi/Documents/亨吉利/src/server/config.ts`
- 修改：`/Users/yutianqi/Documents/亨吉利/src/server/db.ts`
- 修改：`/Users/yutianqi/Documents/亨吉利/src/server/creative.ts`

- [ ] **步骤 1：为数据库迁移编写测试**

```ts
test("rejects a creative job without an active user before calling the vendor", async () => {
  await expect(createCreativeJob({ user: pendingUser, request: validMusicRequest })).rejects.toThrow("账号待管理员审批");
});
```

- [ ] **步骤 2：运行并确认权限测试失败**

运行：`node --test --import tsx src/server/creative.test.ts`

预期：FAIL，`createCreativeJob` 未定义。

- [ ] **步骤 3：实现表、适配器和归档**

创建 `creative_jobs`、`creative_artifacts` 表和索引；将 `asset_files.asset_type` 的约束迁移为包含 `audio`。实现 `createMusicJob`、`createImageJob`、`refreshCreativeJob`、`archiveArtifact`；供应商令牌只读自环境变量，任务完成后立即写入用户专属 `openclaw文件/<user>/Harvis 创作/` 并调用 `indexAssetFile`。

- [ ] **步骤 4：运行测试和类型检查**

运行：`node --test --import tsx src/server/creative.test.ts && npm run typecheck`

预期：测试通过、TypeScript 无错误。

- [ ] **步骤 5：部署前保存配置模板**

在 `.env.production.example` 新增空的 `HARVIS_*` 变量，不在任何跟踪文件中写入供应商令牌。

### 任务 3：Gateway HTTP 接口、回调和上传

**文件：**
- 修改：`/Users/yutianqi/Documents/亨吉利/src/server/index.ts`
- 修改：`/Users/yutianqi/Documents/亨吉利/src/server/creative.ts`

- [ ] **步骤 1：编写端点契约测试**

```ts
test("POST /api/creative/music/jobs requires an active session", async () => {
  const response = await request(app).post("/api/creative/music/jobs").send(validMusicRequest);
  expect(response.status).toBe(401);
});
```

- [ ] **步骤 2：运行测试验证失败**

运行：`node --test --import tsx src/server/creative.test.ts`

预期：FAIL，因为端点尚未注册。

- [ ] **步骤 3：注册端点**

实现 `POST /api/creative/music/jobs`、`POST /api/creative/image/jobs`、`GET /api/creative/jobs`、`GET /api/creative/jobs/:id`、`POST /api/creative/jobs/:id/stage-for-chat` 和随机 nonce 回调端点。所有用户端点按 `req.user.id` 过滤；回调仅使用 nonce 识别任务并采用幂等更新。

- [ ] **步骤 4：运行测试和构建**

运行：`npm run typecheck && npm run build`

预期：退出码 0。

### 任务 4：Tauri 客户端桥接与失败测试

**文件：**
- 创建：`E:/xiaoheng-workbuddy-staffdeck-agpl/src/services/tauri/creative.ts`
- 修改：`E:/xiaoheng-workbuddy-staffdeck-agpl/src/services/tauri/marketing.ts`
- 修改：`E:/xiaoheng-workbuddy-staffdeck-agpl/src-tauri/src/marketing.rs`
- 测试：`E:/xiaoheng-workbuddy-staffdeck-agpl/src/services/tauri/creative.test.ts`

- [ ] **步骤 1：编写失败测试**

```ts
import { resolveCreativeRequest } from "./creative";

test("stages a finished artifact instead of sending it as a chat message", () => {
  expect(resolveCreativeRequest("stage", "job-1")).toEqual({ command: "creative_stage_for_chat", jobId: "job-1" });
});
```

- [ ] **步骤 2：运行测试验证失败**

运行：`npm test -- src/services/tauri/creative.test.ts`

预期：FAIL，模块不存在。

- [ ] **步骤 3：实现最少调用层**

定义 `CreativeJob`、`CreativeArtifact`、音乐/图片请求类型；在 Rust 端复用营销登录 Cookie 调用 Gateway，禁止桌面端直接拼供应商 URL 或读取供应商配置。

- [ ] **步骤 4：运行前端测试和 Rust 检查**

运行：`npm test -- src/services/tauri/creative.test.ts && cargo check --manifest-path src-tauri/Cargo.toml`

预期：测试通过、Rust 检查通过。

### 任务 5：原生创作页和导航

**文件：**
- 创建：`E:/xiaoheng-workbuddy-staffdeck-agpl/src/components/Creative/index.tsx`
- 创建：`E:/xiaoheng-workbuddy-staffdeck-agpl/src/components/Creative/Creative.css`
- 修改：`E:/xiaoheng-workbuddy-staffdeck-agpl/src/App.tsx`

- [ ] **步骤 1：编写组件失败测试**

```tsx
test("shows custom pixel fields only after choosing the custom canvas", async () => {
  render(<CreativePage initialKind="image" />);
  expect(screen.queryByLabelText("宽度")) .toBeNull();
  await userEvent.click(screen.getByRole("radio", { name: "自定义" }));
  expect(screen.getByLabelText("宽度")).toBeVisible();
});
```

- [ ] **步骤 2：运行测试验证失败**

运行：`npm test -- src/components/Creative/Creative.test.tsx`

预期：FAIL，组件不存在。

- [ ] **步骤 3：实现音乐、图片和任务 UI**

实现自由/助手音乐模式、确定性提示词模板、图片模板、五个比例预设、自定义像素输入、任务卡片、产物预览和“放入会话输入框”。将“AI 创作”加入左侧一级导航并复用 `highLevelUnlocked` 门禁。

- [ ] **步骤 4：运行前端测试和生产构建**

运行：`npm test -- src/components/Creative/Creative.test.tsx && npm run build`

预期：测试通过、Vite 构建成功。

### 任务 6：系统 Skill 与安装包验证

**文件：**
- 创建：`E:/xiaoheng-workbuddy-staffdeck-agpl/third_party/piscis-engine/system-skills/harvis-creative/SKILL.md`
- 修改：对应系统 Skill 注册清单和工具目录。

- [ ] **步骤 1：编写 Skill 发现失败测试**

```rust
assert!(find_system_skill("harvis-creative").is_some());
```

- [ ] **步骤 2：运行失败测试**

运行：`cargo test --manifest-path third_party/piscis-engine/piscis-kernel/Cargo.toml harvis`

预期：FAIL，Skill 未注册。

- [ ] **步骤 3：实现 Skill**

Skill 只能调用内部 Gateway 工具；“写提示词”不创建收费任务；图片和音频完成后只创建会话附件草稿。

- [ ] **步骤 4：全量验证、提交与打包**

运行：`npm test && npm run build && cargo test --manifest-path third_party/piscis-engine/piscis-kernel/Cargo.toml && cargo check --manifest-path src-tauri/Cargo.toml && npx tauri build --bundles nsis`

预期：测试和构建均以 0 退出，生成新的 Windows NSIS 安装包。

- [ ] **步骤 5：提交**

运行：`git add -A && git commit -m "feat: add Harvis creative module"`。
