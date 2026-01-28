# SQLite 配置持久化 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 为连接配置提供本地 SQLite 持久化，暴露 GET/POST 配置接口，前端可加载/保存并提示来源。

**Architecture:** 后端新增 `config_store` 模块使用 `rusqlite` 存储默认连接，启动时初始化 `~/.amarone/config.db`；API 返回 `source/updated_at`。前端扩展类型/store/API，`ConnectionForm` 支持加载已保存、保存配置并提示状态。

**Tech Stack:** Rust (Axum, rusqlite), React + TS, Ant Design, Zustand, Axios, Vite。

---

### Task 1: 后端 SQLite 存储基础

**Files:**
- Create: `backend/src/config_store/mod.rs`
- Modify: `backend/Cargo.toml`
- Test: `backend/src/config_store/mod.rs`（内嵌 tests 模块）

**Step 1: 写失败测试**  
在 `mod.rs` 内编写 `#[cfg(test)]`，使用临时目录构造 config_store，对 `init_db`/`get_default`/`upsert_default` 行为（含 `.env` fallback 标记 `source`）编写测试，预期初次无记录时返回 None 或 env 构造值。

**Step 2: 运行测试看其失败**  
Run: `cd backend && cargo test config_store -- --nocapture`  
Expected: 编译/逻辑缺失导致失败。

**Step 3: 写最小实现**  
实现目录创建、SQLite 连接、表建表、`get_default`、`upsert_default`，`source` 枚举为 `sqlite`，无记录时返回 None（由上层决定 env fallback）。

**Step 4: 再跑测试确保通过**  
Run: 同上，预期 PASS。

**Step 5: 代码清理**  
整理错误信息、避免重复 SQL，确保 `updated_at` 为 ISO8601。

### Task 2: 后端配置 API

**Files:**
- Modify: `backend/src/api/mod.rs`
- Modify: `backend/src/api/config.rs`（新建）
- Modify: `backend/src/main.rs`
- Modify: `backend/src/models/mod.rs`
- Test: `backend/tests/config_api.rs`

**Step 1: 写失败测试（集成）**  
在 `backend/tests/config_api.rs` 用 `axum::Router` + 测试客户端调用 `GET/POST /api/config/connection`，断言返回 `config+source+updated_at`，首次 GET fallback 为 env 标记 `env`，POST 保存后再 GET 为 `sqlite` 且字段匹配。

**Step 2: 跑测试看失败**  
Run: `cd backend && cargo test config_api -- --nocapture`  
Expected: 404/未实现失败。

**Step 3: 实现最小代码**  
新增 `api/config.rs` 定义路由 handler，扩展 `create_router` 注册路由，`main.rs` 初始化 config_store 并注入共享 state；`models` 添加 API 响应结构含 `source/updated_at`。handler 中：GET 调用 store，若 None 则读取 env 组装并标记 `env`；POST 校验字段后 upsert。

**Step 4: 跑测试确保通过**  
Run: 同上，预期 PASS。

**Step 5: 清理**  
错误映射统一为 `ApiResponse::error`，日志添加。

### Task 3: 前端类型/状态/API 扩展

**Files:**
- Modify: `frontend/src/types/index.ts`
- Modify: `frontend/src/store/useExportStore.ts`
- Modify: `frontend/src/services/api.ts`
- Test: （待 Task 6）

**Step 1: 写失败测试**  
在 Task 6 的前端测试文件中先写类型/API 行为断言（例如加载保存时返回对象应包含 `source/updated_at`），此处只声明接口签名编译失败以促成类型更新。

**Step 2: 跑测试看失败**  
Run: `cd frontend && npm test`（或 `npm run lint` 若无测试）预期类型错误。

**Step 3: 最小实现**  
扩展 `ConnectionConfig` 增加可选 `source?: 'sqlite' | 'env'`、`updated_at?: string`；store 增加 `loadedFrom: 'saved' | 'manual' | null` 与 `setLoadedFrom`；API 添加 `getSavedConnection`/`saveConnection`，返回体类型包含 `config/source/updated_at`。

**Step 4: 再跑测试**  
Run: 同上，类型/测试通过。

**Step 5: 清理**  
确保未使用字段有合理初始值，API 错误处理保持一致。

### Task 4: 前端 ConnectionForm UI

**Files:**
- Modify: `frontend/src/components/ConnectionForm.tsx`
- Modify: `frontend/src/index.css`（如需样式）
- Test: （Task 6 补充）

**Step 1: 写失败测试**  
在 Task 6 的组件测试中编写：加载按钮调用 GET 填充表单并显示来源提示；保存按钮调用 POST；未保存更改提示；来源为 env 时提示建议保存。

**Step 2: 跑测试失败**  
Run: `cd frontend && npm test ConnectionForm`（或对应命令），预期缺少 UI/逻辑导致失败。

**Step 3: 最小实现**  
添加“加载已保存”“保存配置”按钮，来源/时间展示文本；加载成功后填充 Form 并设置 `loadedFrom='saved'`；保存调用 POST 成功提示并更新时间；未保存改动时点击下一步给出确认/提示；来源 env 时展示“建议保存到本地”。

**Step 4: 跑测试通过**  
Run: 同上，确保组件测试通过。

**Step 5: 清理**  
对动画/样式保持现有风格，避免重复状态。

### Task 5: 后端测试补充

**Files:**
- Modify: `backend/src/config_store/mod.rs`（tests）
- Modify: `backend/tests/config_api.rs`

**Step 1: 添加缺失边界测试**  
覆盖：空字符串/端口 0 校验失败；重复保存 updated_at 变化。

**Step 2: 跑测试失败/通过**  
Run: `cd backend && cargo test`.

**Step 3: 最小修正**  
根据失败修正校验/时间刷新。

**Step 4: 再跑测试通过**  
Run: 同上。

**Step 5: 清理**  
提取测试辅助函数减少重复。

### Task 6: 前端测试补充

**Files:**
- Create: `frontend/src/components/__tests__/ConnectionForm.test.tsx`
- (可选) 配置测试框架文件

**Step 1: 编写组件测试**  
使用 RTL：模拟 GET/POST 成功/失败，断言按钮状态、来源提示、未保存警告、下一步阻塞逻辑。

**Step 2: 跑测试看失败/通过**  
Run: `cd frontend && npm test -- ConnectionForm.test.tsx`。

**Step 3: 修复实现/测试**  
根据失败调整组件或测试。

**Step 4: 再跑确保通过**  
Run: 同上。

**Step 5: 清理**  
移除多余 mock，保持快照最小。

### Task 7: 手动验收

**Steps:**
1. 删除/重命名 `~/.amarone/config.db`；启动后端，前端加载默认应显示 `source=env`。
2. 在前端填表 -> 保存配置 -> 再次加载显示 `source=sqlite` 和 `updated_at`；测试连接成功。
3. 走完整导出流程验证未受影响。

