# 代码审阅文档（架构/封装/整洁度/完成度）

- 审阅日期：2026-03-09
- 审阅范围：`backend`、`frontend` 近期多库导出改造代码
- 快照说明：基于本地工作树 2026-03-09 当前快照（`cargo check` 可通过）

## 1. 执行摘要

当前版本在“可编译性”与“基础流程可运行性”上较前一轮明显提升，但仍存在会影响线上结果可信度的关键风险，集中在 Kingbase 路径：

1. 同名表跨 schema 的解析存在歧义，可能导出错表。
2. 数据解码失败被静默吞掉并落成 `NULL`，存在隐蔽数据损坏风险。
3. 大表导出仍是整表加载，内存与稳定性风险较高。

## 2. 验证结果

- 后端：`cd backend && cargo check` 通过
- 后端：`cd backend && cargo test --lib` 通过（101 passed）
- 后端：`cd backend && cargo test` 在当前环境失败（`dm8-export-backend.exe` 文件占用，`os error 5`）
- 前端：`cd frontend && npm run build` 通过（但仍有大 chunk 告警）

## 3. 主要问题（按严重级别）

### P0：Kingbase 同名表跨 schema 导出歧义，存在导错表风险

- 证据：
  - `get_tables` 仅返回 `relname`，不含 schema：[backend/src/db/pg_native.rs:113](/E:/self/tool-database/backend/src/db/pg_native.rs:113)、[backend/src/db/pg_native.rs:129](/E:/self/tool-database/backend/src/db/pg_native.rs:129)
  - `resolve_table_schema` 对同名表按 schema 字典序取第一条：[backend/src/db/pg_native.rs:169](/E:/self/tool-database/backend/src/db/pg_native.rs:169)、[backend/src/db/pg_native.rs:179](/E:/self/tool-database/backend/src/db/pg_native.rs:179)
- 影响：
  - 业务上存在同名表时，导出对象不确定，属于高风险数据正确性问题。
- 建议：
  - API 与前端选择项改为“schema.table”唯一标识。
  - 导出/详情查询不再二次猜 schema，改为显式传入 schema。

### P0：Kingbase 数据解码错误被吞掉并静默转 `NULL`

- 证据：
  - 解码路径统一 `.ok().flatten().unwrap_or(CanonicalValue::Null)`，错误不会上抛：
    - [backend/src/export/kingbase_poc.rs:257](/E:/self/tool-database/backend/src/export/kingbase_poc.rs:257)
    - [backend/src/export/kingbase_to_other_poc.rs:339](/E:/self/tool-database/backend/src/export/kingbase_to_other_poc.rs:339)
- 影响：
  - 一旦类型解码不匹配，导出脚本会“成功生成但值被置空”，属于隐蔽数据损坏。
- 建议：
  - 改为“解码失败即返回错误并中止当前任务”。
  - 增加按类型的显式转换器与回归测试样例（日期、时间、numeric、json、bytea）。

### P1：Kingbase 数据导出仍是整表加载，缺少流式读取

- 证据：
  - `client.query(&sql, &[])` 全量拉取后再分 batch 写文件：
    - [backend/src/export/kingbase_poc.rs:216](/E:/self/tool-database/backend/src/export/kingbase_poc.rs:216)
    - [backend/src/export/kingbase_to_other_poc.rs:298](/E:/self/tool-database/backend/src/export/kingbase_to_other_poc.rs:298)
- 影响：
  - 大表场景内存峰值高，任务稳定性差。
- 建议：
  - 参考 MySQL 路径改为游标/流式读取与批量渲染。

### P1：`schema` 字段语义混用（Kingbase 下被当作 dbname），封装边界不清晰

- 证据：
  - Kingbase 连接将 `config.schema` 当作 `dbname`：[backend/src/db/pg_native.rs:20](/E:/self/tool-database/backend/src/db/pg_native.rs:20)、[backend/src/db/pg_native.rs:29](/E:/self/tool-database/backend/src/db/pg_native.rs:29)
  - 前端仍统一以“模式名 schema”采集：[frontend/src/components/ConnectionForm.tsx:324](/E:/self/tool-database/frontend/src/components/ConnectionForm.tsx:324)
  - 导出上下文将该字段直接视为 `source_schema`：[backend/src/api/export/context.rs:197](/E:/self/tool-database/backend/src/api/export/context.rs:197)
- 影响：
  - 配置含义因数据库类型变化，容易造成连接与导出语义错配。
- 建议：
  - 将连接模型拆分为 `database` 与 `schema` 两个显式字段。
  - 不同数据源适配器只消费自己所需字段，避免跨层“猜语义”。

### P1：本地密钥文件安全策略在 Windows 侧不完整

- 证据：
  - `.key` 与 `config.db` 同目录：[backend/src/config_store/mod.rs:131](/E:/self/tool-database/backend/src/config_store/mod.rs:131)
  - 权限收敛仅在 `unix` 分支执行：[backend/src/config_store/mod.rs:77](/E:/self/tool-database/backend/src/config_store/mod.rs:77)
- 影响：
  - Windows 端缺少 ACL/系统密钥托管，合规和审计解释成本高。
- 建议：
  - Windows 接入 DPAPI（或 Credential Manager）并补 ACL 策略。

### P1：测试覆盖存在盲区，关键路径缺少自动化守护

- 证据：
  - 集成测试默认全 `#[ignore]`：[backend/tests/integration_db.rs:3](/E:/self/tool-database/backend/tests/integration_db.rs:3)
  - `kingbase_poc.rs`、`kingbase_to_other_poc.rs`、`mysql_to_dm8_poc.rs` 等导出模块无单元测试（文件内未发现 `#[test]`）
- 影响：
  - 跨库导出回归容易后移到联调/手工验证阶段。
- 建议：
  - 为关键转换函数补单测；建立最小可运行的多库 smoke CI。

## 4. 架构与封装观察

### 做得好的点

- 已形成 `source`（源能力）+ `dialect`（目标渲染）+ `orchestrator`（路径决策）的分层雏形。
- `db::service` 将不同驱动访问收敛在统一入口，路由层较薄。
- 导出能力接口（capability report）与严格模式逻辑清晰，可扩展性不错。

### 主要架构债务

- Kingbase 路径仍存在“连接参数语义”和“对象唯一标识”未统一的问题。
- 导出实现在多个 POC 文件中重复较多，跨路径修复成本偏高。

## 5. 代码整洁度

- 超长文件已影响可维护性：
  - [backend/src/dialect/mod.rs:1](/E:/self/tool-database/backend/src/dialect/mod.rs:1)（524 行）
  - [frontend/src/components/ExportConfig.tsx:1](/E:/self/tool-database/frontend/src/components/ExportConfig.tsx:1)（535 行）
- 构建产物管理仍可优化：`.gitignore` 仅忽略 `/backend/target/`，未覆盖 `target-codex-*` 目录：[.gitignore:2](/E:/self/tool-database/.gitignore:2)
- 前端打包仍有大包告警（`vendor-antd-core` 约 828kB），需持续做按路由拆分与按需加载。

## 6. 功能完成度评估

- 基础连接与导出流程：可运行
- 跨库导出正确性：中等风险（Kingbase 路径需优先补强）
- 可发布性：建议“修复 P0 后再对外承诺稳定跨库导出”

## 7. 建议优先级（两周窗口）

1. P0（本周）：修复 Kingbase 表唯一标识（schema.table）与导出定位逻辑。
2. P0（本周）：修复 Kingbase 解码错误静默吞掉的问题，改为 fail-fast。
3. P1（下周）：Kingbase 导出改为流式读取，压低大表内存峰值。
4. P1（下周）：重构连接配置模型（database/schema 分离）。
5. P1（并行）：补关键导出模块单测 + 最小集成 smoke 流程。

