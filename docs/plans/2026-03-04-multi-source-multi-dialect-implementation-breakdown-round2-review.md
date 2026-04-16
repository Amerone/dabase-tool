# 2026-03-04 多数据源多方言拆分方案 - Round2 审阅

日期: 2026-03-04  
审阅人: Codex

## 1. 审阅范围

- 参照台账: `2026-03-04-multi-source-multi-dialect-implementation-breakdown.md`
- 本轮聚焦阶段: Phase A / B / C 已落地代码与回归验证
- 审阅方式: 代码走查 + 编译检查 + 单元测试 + 前端构建

## 2. 本轮落地项

1. 导出执行路径补齐与收敛
- 文件: `backend/src/export/orchestrator.rs`, `backend/src/api/export.rs`
- 变更:
  - 新增 `MysqlToDm8Poc` 执行路径
  - `DM8 -> MySQL` Data 路径显式返回未实现错误（避免隐式行为）

2. MySQL 源导出能力改造成“目标方言可配置”
- 文件: `backend/src/export/mysql_poc.rs`, `backend/src/export/mysql_to_dm8_poc.rs`
- 变更:
  - 从固定 `MySQL renderer` 升级为 `renderer_for(target_dialect)`
  - 新增 `MySQL -> DM8` DDL/Data PoC 封装模块

3. DM8 方言最小渲染能力补齐（用于跨方言 PoC）
- 文件: `backend/src/dialect/mod.rs`
- 变更:
  - 实现 `render_table_ddl`
  - 实现 `render_insert_batch`

4. 能力报告 reason_code 兜底一致性
- 文件: `backend/src/source/mod.rs`, `backend/src/dialect/mod.rs`, `backend/src/api/capability.rs`
- 变更:
  - 通用规则: `partial_support` / `not_supported`
  - 执行路径缺失时覆盖为 `execution_path_missing`
  - 新增对应单测

## 3. 台账对齐结论

1. Phase A: 已完成核心抽象与能力报告接口
2. Phase B: 已完成连接列表 API 主链路（并保留旧接口兼容）
3. Phase C: 2x2 最小 PoC 已形成
- DM8 -> DM8（legacy）
- DM8 -> MySQL（DDL）
- MySQL -> MySQL（DDL/Data）
- MySQL -> DM8（DDL/Data，PoC）

## 4. 验证结果

1. 后端
- `cargo fmt` 通过
- `cargo check` 通过
- `cargo test` 通过（73 passed）

2. 前端
- `npm.cmd run build` 通过（此前已回归）

## 5. 风险与下一批并行任务

1. P0: 补齐 `DM8 -> MySQL Data` PoC 路径
2. P1: MySQL/ODBC Generic 元数据补齐（index/uk/fk/check/trigger）
3. P1: 引入 strict/non-strict 导出模式与 summary 输出（warnings/skipped_objects/reason_code）

## 6. 审阅结论

本轮继续改造后，架构与台账对齐度提升明显，且未破坏现有 DM8 legacy 主链路。  
建议下一轮优先闭环 `DM8 -> MySQL Data`，再进入对象级能力补齐。
