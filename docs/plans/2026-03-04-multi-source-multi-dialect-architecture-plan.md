# 多数据源 + 多目标方言架构优化方案

日期：2026-03-04
作者：Codex

## 1. 背景与现状

当前项目已经具备：

- 多库连接（DM8 / MySQL / Kingbase / Shentong）基础能力。
- DM8 的 DDL/数据导出能力。

但整体仍是 Phase-1 结构，主要问题是：

1. 以 `db_type` 分支驱动业务流程，扩展新库会持续增加 `match` 分支，耦合上升。
2. 导出能力只支持 DM8，尚未形成“源库”和“目标方言”解耦。
3. 导出逻辑集中在大文件中，后续维护和方言扩展成本高。
4. 元数据采集在不同数据源间深度不一致（部分仅有列与主键）。
5. 桌面端驱动发现偏 DM8 专用，未形成统一驱动解析层。

## 2. 目标

### 2.1 业务目标

1. 支持多种数据源接入（至少：MySQL、DM8、Kingbase、Shentong）。
2. 支持导出到多种目标 SQL 方言（源与目标可不同）。
3. 支持同一数据源配置下，按目标方言切换导出策略。

### 2.2 技术目标

1. 解耦“源端采集”和“目标方言渲染”。
2. 引入统一中间模型，避免源库结构直透到目标 SQL。
3. 建立能力声明（capabilities）与测试矩阵（source x target）。
4. 保持向后兼容，分阶段迁移，避免一次性重构风险。

## 3. 非目标（本轮不做）

1. 不做跨库在线迁移（实时写入目标库），本期仍以 SQL 文件导出为主。
2. 不做 GUI 大改，仅补充必要字段与流程。
3. 不追求第一阶段即覆盖所有对象（如复杂触发器、存储过程）全量跨方言转换。

## 4. 目标架构

采用“双轴解耦”：

- 轴 A：`Source Adapter`（负责采集源端元数据与数据）
- 轴 B：`Dialect Renderer`（负责生成目标方言 SQL）

中间通过统一模型连接：

- `CanonicalSchema`（表、列、约束、索引、触发器等）
- `LogicalType`（标准逻辑类型）
- `RowValue`（标准值表达）

编排层：

- `Export Orchestrator` 负责将 `SourceAdapter` + `DialectRenderer` 组合执行。

## 5. 分层设计

### 5.1 Source Adapter 层（源端）

职责：

1. 连接测试、列出 schema/table。
2. 拉取表结构、约束、索引、触发器等元数据。
3. 流式读取数据行（避免大表内存峰值）。

建议接口（示意）：

```rust
pub trait SourceAdapter: Send + Sync {
    fn source_kind(&self) -> SourceKind;
    fn capabilities(&self) -> SourceCapabilities;
    async fn test_connection(&self, cfg: &ConnectionConfig) -> anyhow::Result<()>;
    async fn list_tables(&self, cfg: &ConnectionConfig, schema: &str) -> anyhow::Result<Vec<TableRef>>;
    async fn inspect_schema(&self, req: &InspectRequest) -> anyhow::Result<CanonicalSchema>;
    async fn stream_rows(&self, req: &DataReadRequest) -> anyhow::Result<RowStream>;
}
```

### 5.2 Canonical Domain 层（中间模型）

职责：

1. 统一表达数据库对象，不携带特定数据库语法。
2. 统一类型映射（源类型 -> 逻辑类型）。
3. 为方言渲染提供稳定输入，降低扩展耦合。

核心模型建议：

- `CanonicalSchema`
- `CanonicalTable`
- `CanonicalColumn`
- `CanonicalConstraint`（PK/UK/FK/CHECK）
- `CanonicalIndex`
- `CanonicalTrigger`（先保留原文和基础解析字段）
- `LogicalType`
- `LiteralValue`

### 5.3 Dialect Renderer 层（目标端）

职责：

1. 将 canonical 模型渲染为目标方言 DDL。
2. 将行数据渲染为目标方言 INSERT/LOAD SQL。
3. 提供方言级兼容策略（标识符、关键字、字面量、批量语法）。

建议接口（示意）：

```rust
pub trait DialectRenderer: Send + Sync {
    fn dialect(&self) -> DialectKind;
    fn capabilities(&self) -> DialectCapabilities;
    fn render_ddl(&self, schema: &CanonicalSchema, opt: &DdlRenderOptions) -> anyhow::Result<String>;
    fn render_insert_batch(
        &self,
        table: &CanonicalTable,
        rows: &[CanonicalRow],
        opt: &DataRenderOptions,
    ) -> anyhow::Result<String>;
}
```

### 5.4 Export Orchestrator（编排层）

职责：

1. 根据请求选择 `source adapter` 与 `target renderer`。
2. 拉取 canonical schema，按对象顺序渲染（表、约束、索引、触发器等）。
3. 控制导出流程、文件输出、错误包装与进度。

## 6. 推荐目录重构

建议逐步演进到：

```text
backend/src/
  domain/
    canonical/
      model.rs
      logical_type.rs
  source/
    mod.rs
    dm8_adapter.rs
    mysql_adapter.rs
    kingbase_adapter.rs
    shentong_adapter.rs
  dialect/
    mod.rs
    dm8_renderer.rs
    mysql_renderer.rs
    kingbase_renderer.rs
    shentong_renderer.rs
  export/
    orchestrator.rs
    pipeline.rs
    writers/
      sql_file_writer.rs
  driver/
    resolver.rs
  api/
    export.rs
    connection.rs
```

说明：

1. 现有 `db/schema.rs`、`export/ddl.rs`、`export/data.rs` 可先“包裹接入”，再逐步拆分。
2. 避免一次性重写，先建立接口和编排层，再逐模块迁移。

## 7. API 合约优化建议

新增或升级导出请求结构：

```json
{
  "source": {
    "db_type": "dm8|mysql|kingbase|shentong",
    "host": "...",
    "port": 0,
    "username": "...",
    "password": "...",
    "schema": "..."
  },
  "target_dialect": "dm8|mysql|kingbase|shentong",
  "objects": {
    "tables": ["..."],
    "include_ddl": true,
    "include_data": true
  },
  "compat_mode": "datagrip|script|datagrip-script",
  "options": {
    "drop_existing": true,
    "batch_size": 1000,
    "include_row_counts": false
  }
}
```

兼容策略：

1. 保留旧字段一段时间，后端做兼容映射。
2. 前端逐步切换到新结构，降低一次性发布风险。

## 8. 驱动与连接策略

统一驱动解析层 `DriverResolver`：

1. Native 通道：MySQL（`sqlx/mysql`）。
2. ODBC 通道：DM8 / Kingbase / Shentong。
3. 每个数据库声明 `DriverRequirement`：
   - 是否必须本地驱动文件
   - 支持的环境变量
   - 自动探测路径

输出统一健康检查结果：

- 文件存在性
- Driver Manager 可见性
- 动态库可加载性
- 依赖缺失提示

## 9. 分阶段实施计划

### Phase 0：基线与护栏（1-2 天）

1. 补齐当前行为基线测试（DM8 导出 + 多库连接）。
2. 建立 golden file 机制（至少 DM8 DDL/Data）。

### Phase 1：接口抽象与无行为迁移（3-5 天）

1. 引入 `SourceAdapter`、`DialectRenderer`、`ExportOrchestrator`。
2. 把现有 DM8 导出能力封装成 `Dm8SourceAdapter + Dm8Renderer`。
3. 外部 API 行为保持不变。

### Phase 2：最小多方言 PoC（3-5 天）

1. 新增 `MySqlRenderer`（先覆盖 Table/Column/PK/基础 Insert）。
2. 打通 2x2：
   - DM8 -> DM8
   - DM8 -> MySQL
   - MySQL -> MySQL
   - MySQL -> DM8

### Phase 3：元数据能力补齐（5-8 天）

1. MySQL/Kingbase/Shentong 补齐 index、uk、fk、check、trigger。
2. 引入 capability 告知前端“哪些对象可导出”。

### Phase 4：多数据源配置中心（2-4 天）

1. 从“每 db_type 默认一条”升级为“连接列表管理”。
2. 支持命名、最近使用、复制配置、连接健康状态缓存。

### Phase 5：质量与发布（持续）

1. 建立 `source x target` 回归矩阵。
2. 建立方言快照测试 + 小规模真实库集成测试。

## 10. 风险与应对

1. 风险：ODBC 驱动依赖复杂，环境差异大。  
   应对：驱动健康检查工具化，错误信息标准化。

2. 风险：不同库 `information_schema` 不一致。  
   应对：每个 SourceAdapter 建立“标准查询 + 回退查询”。

3. 风险：类型映射不确定导致 SQL 不可执行。  
   应对：建立 `LogicalType -> DialectType` 显式映射表和测试用例。

4. 风险：大表导出内存与耗时。  
   应对：流式读取 + 分批渲染 + 进度事件上报。

## 11. 验收标准

1. 架构层面：
   - 新增数据库或新方言时，不需要改核心编排流程，只需新增 adapter/renderer。
2. 功能层面：
   - 支持至少 2 个源库、2 个目标方言双向导出 PoC。
3. 质量层面：
   - 关键导出路径具备自动化测试和 golden file 对比。
4. 兼容层面：
   - 旧接口在迁移窗口内可用，前端可平滑切换。

## 12. 当前仓库的落地起点（建议）

1. 先落地 Phase 1：抽象接口 + 封装现有 DM8 逻辑，不改变业务行为。
2. 紧接着做 Phase 2 的最小 PoC（2x2），验证模型与渲染器设计正确性。
3. PoC 验证通过后再进入 Phase 3 的批量补齐，避免过早大规模重构。

