# 2026-03-04 多数据源多方言拆分方案 - Round3 审阅

日期: 2026-03-04  
审阅人: Codex

## 1. 本轮目标

- 闭环 `DM8 -> MySQL Data` PoC
- 保持既有 DM8 legacy 链路稳定
- 校准 orchestrator 执行路径与能力模型的一致性

## 2. 代码改造

1. `DM8 -> MySQL Data` 导出实现
- 文件: `backend/src/export/dm8_to_mysql_poc.rs`
- 关键点:
  - 基于 ODBC `TextRowSet` 分批读取 DM8 行数据
  - 按 `LogicalType` 转换为 `CanonicalValue`
  - 复用 `MySqlDialectRenderer` 统一渲染 INSERT 批次

2. 执行路径升级
- 文件: `backend/src/export/orchestrator.rs`
- 变更:
  - `DM8 -> MySQL` 从“仅 DDL”升级为“DDL/Data 都走 `Dm8ToMysqlPoc`”
  - 新增测试 `dm8_to_mysql_data_uses_poc_path`

3. API 分发接入
- 文件: `backend/src/api/export.rs`
- 变更:
  - `export_data` 的 `Dm8ToMysqlPoc` 分支改为真实导出调用，不再返回未实现错误

4. 单元测试补充
- 文件: `backend/src/export/dm8_to_mysql_poc.rs`
- 新增:
  - `parse_hex_bytes_supports_prefixed_input`
  - `parse_dm8_value_maps_numeric_and_bool`
  - `quote_qualified_identifier_quotes_each_part`

## 3. 验证结果

1. 编译与格式化
- `cargo fmt` 通过
- `cargo check` 通过

2. 测试
- `cargo test export::orchestrator::tests` 通过
- `cargo test export::dm8_to_mysql_poc::tests` 通过
- `cargo test` 全量通过（77 passed）

## 4. 台账状态更新

1. Phase C（2x2 最小 PoC）
- 已覆盖:
  - DM8 -> DM8（legacy）
  - DM8 -> MySQL（DDL/Data）
  - MySQL -> MySQL（DDL/Data）
  - MySQL -> DM8（DDL/Data）

2. 剩余重点
- Phase D-1: 元数据对象补齐（index/uk/fk/check/trigger）
- Phase D-2: strict/non-strict 与 summary 输出

## 5. 审阅结论

本轮完成后，Phase C 的最小 PoC 目标已闭环，且回归稳定。下一轮建议进入 Phase D 的对象级能力补齐。
