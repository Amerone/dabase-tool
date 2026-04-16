# 多数据源 + 多方言实施拆分方案（WBS）

日期：2026-03-04  
基线：`2026-03-04-multi-source-multi-dialect-architecture-plan-v2.md`

## 1. 拆分目标

将 V2 方案拆解为可执行任务包，满足以下要求：

1. 每个阶段都有明确输入、输出、依赖、验收标准。
2. 每个阶段都可独立回滚，不破坏现有 DM8 导出链路。
3. 先做架构护栏，再做能力扩展，避免“边改边散”。

## 2. 总体节奏（建议 8-10 周）

1. Week 1-2：Phase A（抽象引入，不改行为）
2. Week 3：Phase B（配置中心升级）
3. Week 4-5：Phase C（2x2 PoC）
4. Week 6-7：Phase D（对象能力补齐）
5. Week 8-10：Phase E（矩阵回归、灰度发布、运维闭环）

## 3. 阶段拆分与任务包

## Phase A：抽象引入（不改行为）

### A-1 目录与接口骨架

任务：

1. 新增目录与模块：
- `backend/src/source/`
- `backend/src/dialect/`
- `backend/src/domain/canonical/`
- `backend/src/export/orchestrator.rs`
2. 定义 trait 与核心类型：
- `SourceAdapter`
- `DialectRenderer`
- `SourceCapabilities`
- `DialectCapabilities`
- `ExportCapabilityReport`

输入：

1. 现有 `backend/src/db/*`、`backend/src/export/*` 实现。

输出：

1. 可编译的抽象层骨架，默认路径仍走 legacy 逻辑。

DoD：

1. `cargo check` 通过。
2. 现有 API 行为无变化。

### A-2 DM8 legacy 封装适配

任务：

1. 封装 `Dm8SourceAdapter`：调用现有 `db/schema.rs` 与连接逻辑。
2. 封装 `Dm8DialectRenderer`：调用现有 `export/ddl.rs`、`export/data.rs`。
3. 新增 `LegacyBridge`：将 canonical 请求映射回 legacy 输入。

输入：

1. A-1 输出接口骨架。

输出：

1. “新编排器调用旧实现”的可运行路径。

DoD：

1. DM8 导出结果与 legacy 快照一致。
2. 新路径默认受 feature flag 保护（默认关闭）。

### A-3 能力报告接口（只读）

任务：

1. 新增 API：`GET /api/export/capabilities`（或等价路由）。
2. 返回 source/target/object 维度能力报告。
3. 对当前系统返回保守结果：
- DM8 source + DM8 target：full/partial 按现状标注
- 其他组合：partial/none

DoD：

1. 契约测试通过。
2. 前端可消费并展示最小信息。

---

## Phase B：配置中心升级

### B-1 数据模型升级

任务：

1. SQLite 表结构从“单默认连接”升级为“连接列表”。
2. 增加字段：
- `id`
- `name`
- `db_type`
- `is_favorite`
- `last_used_at`

3. 提供迁移脚本：
- 将历史 `default-<db_type>` 记录迁移为列表中的初始项。

DoD：

1. 旧配置自动迁移可读。
2. 新旧接口兼容至少 2 个版本窗口。

### B-2 配置 API 升级

任务：

1. 新增列表接口：查询、创建、更新、删除。
2. 导出请求支持 `connection_id` 与 override 字段。
3. 保留旧接口并标记 deprecation。

DoD：

1. 前端能在连接列表中选择配置并完成连接测试。
2. 兼容回归测试通过。

---

## Phase C：2x2 最小 PoC（核心里程碑）

### C-1 Canonical 模型 v1

任务：

1. 定义 `LogicalType v1`：
- integer
- decimal
- float
- string
- text
- binary
- bool
- date
- datetime
- json
2. 定义最小对象模型：
- table
- column
- primary_key
- row_values

DoD：

1. 模型文档和类型映射表入库。
2. 通过单元测试验证最小类型映射。

### C-2 MySQL Source + MySQL Dialect（最小对象）

任务：

1. 实现 `MySqlSourceAdapter`（table/column/pk/rows）。
2. 实现 `MySqlDialectRenderer`（DDL + basic insert）。

DoD：

1. `MySQL -> MySQL` 导出可运行。
2. 快照测试通过。

### C-3 DM8 ↔ MySQL 双向打通

任务：

1. `DM8 -> MySQL`：最小对象导出。
2. `MySQL -> DM8`：最小对象导出。
3. 形成 2x2 验证矩阵：
- DM8->DM8
- DM8->MySQL
- MySQL->DM8
- MySQL->MySQL

DoD：

1. 4 条链路均有自动化用例。
2. 每条链路至少 1 套 golden SQL 快照。

---

## Phase D：对象能力补齐

### D-1 元数据采集增强

任务：

1. 为 MySQL/ODBC Generic 逐步补齐：
- index
- unique
- foreign_key
- check
- trigger

2. 为每类对象标注能力等级：
- full
- partial
- none

DoD：

1. 能力报告可精确反映对象级支持情况。
2. 不支持对象具备明确 `reason_code`。

### D-2 渲染能力增强

任务：

1. 补齐渲染器对新增对象的输出支持。
2. strict mode 行为定义：
- strict=true：遇到 none 直接失败
- strict=false：跳过并告警

DoD：

1. 行为与能力报告一致。
2. 导出 summary 输出告警明细。

---

## Phase E：质量、灰度与运维

### E-1 测试矩阵

任务：

1. 建立最小矩阵：
- source：dm8/mysql/kingbase/shentong
- target：dm8/mysql（第一批）
2. 维护 smoke + snapshot + integration 分层测试。

DoD：

1. PR 必跑 smoke + snapshot。
2. release 分支必跑 integration。

### E-2 灰度发布与回滚

任务：

1. Feature flag 按环境控制：
- dev：默认开
- test：灰度开
- prod：默认关，白名单开
2. 回滚策略：
- 一键切回 legacy 导出链路
- 保留异常导出请求日志

DoD：

1. 30 分钟内可回滚。
2. 回滚后导出成功率恢复到基线。

### E-3 观测与告警

任务：

1. 指标：
- 导出成功率
- 平均耗时
- 警告率
- 失败分类 TopN
2. 输出 `summary.json`：
- capability_report
- warnings
- skipped_objects
- duration_ms

DoD：

1. 可按 source/target 维度定位失败趋势。

## 4. 里程碑定义

1. M1（Phase A 完成）：新抽象可编译，行为不变。
2. M2（Phase B 完成）：连接列表模型可用，旧配置兼容。
3. M3（Phase C 完成）：2x2 最小 PoC 完成。
4. M4（Phase D 完成）：对象能力补齐并可解释。
5. M5（Phase E 完成）：具备灰度与回滚闭环。

## 5. 任务追踪模板（建议）

每个任务卡片至少包含：

1. 任务编号（如 A-1、C-3）
2. 负责人
3. 依赖任务
4. 输出文件/模块
5. 验收用例
6. 回滚方案

## 6. 立即可开工的首批任务

1. A-1：抽象目录 + trait 骨架。
2. A-2：DM8 legacy bridge 封装。
3. A-3：能力报告只读接口。
4. B-1：配置表迁移脚本设计。

