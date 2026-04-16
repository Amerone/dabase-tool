# 多数据源 + 多目标方言架构方案（V2 修订稿）

日期：2026-03-04  
基线文档：`2026-03-04-multi-source-multi-dialect-architecture-plan.md`

## 1. 修订目标

本修订稿围绕基线方案，重点解决三个落地问题：

1. 先做什么才不会破坏现有 DM8 导出能力。
2. 多数据源和多方言如何分阶段上线并可回滚。
3. 前后端如何基于“能力声明”避免功能错配。

## 2. 关键修订点（相对基线）

### 2.1 阶段顺序调整

基线方案中“配置中心升级”靠后，修订为前置：

1. `Phase A`：抽象层引入（不改行为）
2. `Phase B`：配置中心升级（连接列表）
3. `Phase C`：2x2 最小跨方言 PoC
4. `Phase D`：对象能力补齐
5. `Phase E`：测试矩阵与发布治理

原因：如果不先升级配置模型，后续多环境/多连接验证会受限。

### 2.2 能力模型前置

新增能力协商机制，导出前先计算能力报告：

- `SourceCapabilities`
- `DialectCapabilities`
- `ExportCapabilityReport`

执行策略：

1. 可支持对象：正常导出。
2. 部分支持对象：降级导出并写入 warning。
3. 不支持对象：默认跳过，支持“严格模式失败”。

### 2.3 API 合约修订

保持兼容前提下引入新字段：

1. `source`（连接信息或连接 ID）
2. `target_dialect`
3. `compat_mode`
4. `strict_mode`

兼容规则：

1. 旧请求未传 `target_dialect` 时，默认等于 `source.db_type`。
2. 旧请求继续可用，后端做映射并返回 deprecation 提示。

### 2.4 风险控制修订

新增以下控制点：

1. Feature Flag：新编排器默认关闭，可按环境开启。
2. 双写比对：PoC 阶段对关键对象输出进行快照对比。
3. 回滚开关：异常时回落到原 DM8 导出路径。

## 3. V2 目标架构

## 3.1 分层

1. `source/`：源端元数据与数据采集（按数据库实现 adapter）。
2. `domain/canonical/`：统一中间模型。
3. `dialect/`：目标方言渲染。
4. `export/orchestrator/`：编排执行、能力协商、输出控制。

## 3.2 接口草案

```rust
pub trait SourceAdapter {
    fn source_kind(&self) -> SourceKind;
    fn capabilities(&self) -> SourceCapabilities;
    async fn inspect_schema(&self, req: &InspectRequest) -> anyhow::Result<CanonicalSchema>;
    async fn stream_rows(&self, req: &DataReadRequest) -> anyhow::Result<RowStream>;
}

pub trait DialectRenderer {
    fn dialect_kind(&self) -> DialectKind;
    fn capabilities(&self) -> DialectCapabilities;
    fn render_ddl(&self, schema: &CanonicalSchema, opt: &DdlRenderOptions) -> anyhow::Result<String>;
    fn render_data(&self, table: &CanonicalTable, rows: &[CanonicalRow], opt: &DataRenderOptions) -> anyhow::Result<String>;
}
```

## 3.3 编排流程

1. 解析请求 -> 解析 source / target。
2. 计算能力报告 -> 返回给前端确认（可选）或直接执行。
3. 采集 schema -> canonical 转换。
4. 按目标方言渲染 DDL / Data。
5. 写入输出文件 + summary（含 warnings）。

## 4. 分阶段实施（V2）

## Phase A：抽象引入（不改行为）

目标：

1. 新增 `SourceAdapter` / `DialectRenderer` / `Orchestrator`。
2. 封装现有 DM8 逻辑为 `Dm8SourceAdapter + Dm8Renderer`。
3. API 行为不变，回归测试通过。

DoD：

1. 旧接口无行为变化。
2. DM8 导出结果快照不变。

## Phase B：配置中心升级

目标：

1. 从 `default-<db_type>` 升级为连接列表模型。
2. 导出请求支持 `connection_id` + 临时覆盖字段。

DoD：

1. 支持新增、编辑、删除、最近使用。
2. 不影响现有保存配置读写（提供迁移脚本/兼容读取）。

## Phase C：2x2 最小 PoC

范围固定：

1. `source={dm8,mysql}`
2. `target={dm8,mysql}`
3. 对象：table/column/pk/basic insert

DoD：

1. 4 条链路均可导出 SQL 文件。
2. 每条链路至少有 1 套快照测试。

## Phase D：对象能力补齐

增量支持：

1. index/unique/fk/check/trigger
2. 能力报告实时反映覆盖率。

DoD：

1. 非支持对象具备可解释的 warning 输出。
2. 严格模式行为可预测。

## Phase E：质量与发布

1. 建立 `source x target` 回归矩阵。
2. 发布分级：灰度 -> 全量。
3. 监控导出失败率与主要错误分类。

## 5. 文件级改造建议（本项目）

1. 新增目录：
- `backend/src/source/`
- `backend/src/dialect/`
- `backend/src/domain/canonical/`
- `backend/src/export/orchestrator.rs`

2. 逐步迁移：
- 保留 `backend/src/export/ddl.rs`、`backend/src/export/data.rs` 作为 legacy 实现。
- V2 编排器先调用 legacy DM8 逻辑，确保首阶段可落地。

3. 前端调整：
- 增加能力报告展示区（支持对象、部分支持、不支持对象）。
- 去掉仅靠 `db_type !== dm8` 的硬拦截，改为读取能力报告。

4. Tauri 调整：
- 驱动发现能力从 DM8 专用扩展为按 DB 类型可配置。

## 6. 风险与应对（V2）

1. 风险：跨方言类型映射不等价。  
应对：维护显式 mapping 表 + 样本库回归。

2. 风险：ODBC 元数据差异大。  
应对：adapter 内分层 fallback 查询 + 能力降级输出。

3. 风险：大文件重构引发回归。  
应对：先包裹后迁移，快照守护。

4. 风险：用户认知偏差（以为全量支持）。  
应对：能力报告 + 导出 summary 明确告知。

## 7. 验收标准（V2）

1. 架构层面：新增 source 或新增 dialect 时无需改核心编排流程。
2. 功能层面：完成 2x2 PoC 且可稳定导出。
3. 质量层面：核心路径有自动化快照测试。
4. 运营层面：导出结果包含 warning 与失败分类，便于定位问题。

