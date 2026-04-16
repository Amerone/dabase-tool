# 多数据源 + 多方言方案多轮审阅（结合现有代码）

日期：2026-03-04  
评审对象：`docs/plans/2026-03-04-multi-source-multi-dialect-architecture-plan.md`

---

## 0. 审阅范围与方法

本次审阅按 3 轮进行，每轮都基于当前仓库代码进行核对：

1. 轮次一：现状贴合度（方案是否匹配当前系统边界）
2. 轮次二：迁移可行性（是否能低风险落地）
3. 轮次三：执行闭环（是否可分批上线并可验证）

---

## 1. 轮次一：现状贴合度审阅

### 1.1 结论

原方案方向正确，但需要补充“当前系统真实约束”，否则实施阶段会遇到边界偏差。

### 1.2 代码证据与发现

1. 当前后端入口确实是按 `db_type` 分支路由，属于“数据库类型分派”结构。  
证据：`backend/src/db/service.rs`（`match config.db_type`）

2. 导出链路目前硬性限制为 DM8，非 DM8 会被直接拒绝。  
证据：`backend/src/api/export.rs`（`if req.config.db_type != DbType::Dm8`）

3. 导出实现是强 DM8 方言，且 DDL 文件过大（维护风险高）。  
证据：`backend/src/export/ddl.rs`（DM8 注释与语法处理集中），`backend/src/export/data.rs`

4. MySQL / ODBC Generic 的元数据深度不足，很多对象返回空集合。  
证据：`backend/src/db/mysql.rs`、`backend/src/db/odbc_generic.rs`（indexes/fk/check/triggers 为空）

5. Tauri 驱动发现目前是 DM8 专用流程，不是通用驱动解析器。  
证据：`src-tauri/src/driver.rs`、`src-tauri/src/main.rs`

6. 配置存储模型是“每 db_type 一条默认连接”，不是连接列表模式。  
证据：`backend/src/config_store/mod.rs`（`default-<db_type>` + `ON CONFLICT(name)`）

### 1.3 对原方案的修订建议（R1）

1. 在目标架构中增加“过渡态定义”：明确 Phase-1/Phase-2 的能力差异。
2. 把“多数据源配置中心”提前写为关键前置条件，而非后置优化项。
3. 增加“元数据能力等级”定义，避免前端误以为所有库都可导出完整对象。

---

## 2. 轮次二：迁移可行性审阅

### 2.1 结论

原分阶段路径可行，但需调整顺序：应先抽象“导出编排层 + 能力声明”，再做方言扩展。

### 2.2 关键风险

1. 风险：直接做多方言渲染，缺少中间能力声明，会导致前端功能与后端能力不一致。  
当前事实：前端仅在 UI 做 `db_type !== dm8` 限制，后端也做硬拦截，二者是写死逻辑。

2. 风险：驱动层差异（native vs ODBC）可能污染业务编排层。  
当前事实：MySQL 是 `sqlx`，其余多为 ODBC，连接模型并不统一。

3. 风险：一次性把 `ddl.rs` 和 `data.rs` 全量拆分，回归成本过高。  
当前事实：`ddl.rs` 体量大、规则密集（触发器/时间字面量/兼容模式处理）。

4. 风险：配置模型未升级时，无法支持同库多连接场景（测试、生产、多租户）。  
当前事实：当前存储逻辑按 `default-<db_type>` 覆盖。

### 2.3 对原方案的修订建议（R2）

1. 阶段顺序调整为：
   - A：能力声明 + 编排层抽象（不改行为）
   - B：连接配置升级（连接列表）
   - C：方言渲染扩展
   - D：高级对象（trigger/check/procedure）增强

2. 引入显式能力模型：
   - `SourceCapabilities`：支持哪些元数据对象
   - `DialectCapabilities`：支持哪些 SQL 生成功能

3. 把“导出失败”从硬失败改为“能力降级 + 明确提示”：
   - 例如：仅导出 table/column/pk，跳过 trigger。

---

## 3. 轮次三：执行闭环审阅

### 3.1 结论

原方案可执行，但需补齐“每阶段验收标准 + 回滚策略 + 最小 PoC 边界”。

### 3.2 建议的执行闭环

1. 每阶段必须有 DoD（Definition of Done）：
   - 接口稳定性
   - 快照测试通过
   - 兼容旧 API
   - UI 能正确展示能力差异

2. 每阶段设置回滚策略：
   - 保留现有 DM8 导出作为 fallback
   - 新架构通过 feature flag 控制开关

3. 最小 PoC 必须固定边界：
   - 仅 2x2：`source={dm8,mysql}` x `target={dm8,mysql}`
   - 对象范围先做：table/column/pk/basic insert
   - 暂不承诺 trigger/check 的跨方言等价转换

---

## 4. 修订后的方案（V2）

## 4.1 架构主线

1. `SourceAdapter`：只负责读取，不关心目标方言。
2. `DialectRenderer`：只负责生成目标 SQL，不关心源库连接。
3. `ExportOrchestrator`：编排读取与渲染。
4. `CapabilityRegistry`：驱动 UI 和导出流程的能力协商。

## 4.2 建议接口（V2）

```rust
pub struct ExportPlan {
    pub source: SourceKind,
    pub target: DialectKind,
    pub objects: ExportObjects,
    pub compat_mode: CompatMode,
}

pub struct ExportCapabilityReport {
    pub supported: Vec<ExportObjectKind>,
    pub partial: Vec<ExportObjectKind>,
    pub unsupported: Vec<ExportObjectKind>,
    pub notes: Vec<String>,
}
```

在执行导出前先生成 `ExportCapabilityReport`，前端确认后再执行。

## 4.3 分阶段（V2）

### Phase A（抽象不改行为）

1. 新增 `source/`、`dialect/`、`export/orchestrator.rs`。
2. 将现有 DM8 导出包裹为 `Dm8Source + Dm8Dialect`。
3. 保持 API 输入输出不变。

### Phase B（配置中心升级）

1. SQLite 配置从“按 db_type 默认项”升级为“连接列表”。
2. 增加连接 ID，导出请求可直接引用连接 ID + 覆盖项。

### Phase C（2x2 PoC）

1. 落地 `MySqlSource` 与 `MySqlDialect` 的最小对象导出。
2. 打通 2x2 路径，形成首批跨方言导出能力。

### Phase D（能力补齐）

1. 增量补齐 index/fk/unique/check/trigger。
2. 每补齐一类对象，更新能力报告和快照测试。

---

## 5. 本项目代码下的立即行动清单

1. 先在后端新增 `Capability` 查询接口，避免前端继续写死 `dm8` 判断。  
现状证据：`frontend/src/components/ExportConfig.tsx` 中 `db_type !== 'dm8'` 直接阻断。

2. 在 `export` API 中加入 `target_dialect` 字段（先可选），并做旧字段兼容映射。

3. 将 `backend/src/export/ddl.rs` 的规则拆出为：
   - 标识符与字面量渲染器
   - 对象级渲染器（table/index/fk/trigger）
   - 兼容模式渲染器（datagrip/script）

4. 调整配置存储模型，避免 `default-<db_type>` 覆盖带来的多连接管理问题。

5. 为 ODBC 类数据源补一个统一的驱动健康检查输出结构，减少部署期排障成本。

---

## 6. 总体评审结论

1. 原方案方向正确，应继续推进。
2. 为了在本项目内低风险落地，需要采用 V2 的“能力先行 + 编排抽象优先 + 2x2 PoC 收敛”策略。
3. 不建议直接进入“大规模多方言改造”；应先完成接口抽象与能力协商闭环。

