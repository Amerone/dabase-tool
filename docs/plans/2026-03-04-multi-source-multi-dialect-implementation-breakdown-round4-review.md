# 2026-03-04 多数据源多方言拆分方案 - Round4 审阅

日期: 2026-03-04  
审阅人: Codex

## 1. 本轮目标

- 在不破坏已闭环 PoC 的前提下，推进 Phase D-1（元数据能力补齐）
- 优先补齐 MySQL Source 的对象级元数据采集

## 2. 本轮改造

1. MySQL 元数据采集增强（`get_table_details`）
- 文件: `backend/src/db/mysql.rs`
- 新增查询与聚合能力:
  - indexes（`information_schema.STATISTICS`）
  - unique constraints（`TABLE_CONSTRAINTS + KEY_COLUMN_USAGE`）
  - foreign keys（`REFERENTIAL_CONSTRAINTS + KEY_COLUMN_USAGE`）
  - check constraints（`CHECK_CONSTRAINTS`）
  - triggers（`information_schema.TRIGGERS`）

2. 兼容性处理
- 对 check/triggers 采用安全降级（查询失败时返回空集合），避免不同 MySQL 版本和权限差异导致主流程失败。

3. Source 能力画像更新
- 文件: `backend/src/source/mod.rs`
- MySQL Source 能力从“仅列+主键”升级为:
  - indexes / unique / foreign_keys / triggers: `full`
  - check_constraints: `partial`

## 3. 验证结果

1. 后端校验
- `cargo fmt` 通过
- `cargo check` 通过
- `cargo test` 全量通过（77 passed）

2. 稳定性结论
- 现有 DM8 legacy 导出链路未受影响
- 2x2 最小 PoC 链路保持可用

## 4. 台账推进状态

1. Phase C（2x2 最小 PoC）: 已闭环
2. Phase D-1（元数据补齐）: 已完成 MySQL Source 子项
3. 待推进:
- ODBC Generic（Kingbase/Shentong）对象元数据补齐
- strict/non-strict 导出模式与 summary 输出（Phase D-2）

## 5. 审阅结论

本轮符合“继续拆分并审阅”的要求，且沿架构路线稳定推进。建议下一轮转入 ODBC Generic 的对象采集补齐。
