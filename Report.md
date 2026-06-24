# 跨数据库转换与性能调查报告

生成日期：2026-04-20

调查范围：后端导出 API、跨库执行路径、方言渲染、元数据读取、数据导出管线、连接层，以及前端导出配置入口。

调查方式：静态代码审查。未运行测试，未连接真实 DM8、MySQL、KingbaseES、神通/OSCAR 数据库，因此本报告聚焦代码层可确认的问题、风险和修复建议。

## 1. 总体结论

当前项目已经具备跨数据库导出框架，支持 DM8、MySQL、KingbaseES、ShenTong/OSCAR 四类数据库，并通过执行路径把 DDL 导出和 INSERT 数据导出分发到不同实现。但是整体仍处于“DM8 主路径较成熟，其他方向多为 PoC 或部分能力”的状态。

最需要优先处理的问题不是单个 SQL 方言细节，而是三个系统性风险：跨库语义信息丢失、部分路径静默把解析/解码错误转成 NULL、并行数据导出管线存在无界并发和大文件一次性读入内存的问题。

优先级最高的结论如下：

| 优先级 | 问题 | 影响 |
|---|---|---|
| P0 | Kingbase/PostgreSQL 风格 schema 解析会按表名反查首个 schema | 多 schema 同名表时可能导出错表 |
| P0 | Kingbase、Shentong、DM8 部分数据解析失败会静默变成 NULL | 可能产生不可见数据损坏 |
| P0 | 数据导出管线计算了 `max_parallelism`，但实际按层内所有表同时 spawn | 大 schema 导出时可能打爆数据库连接、线程和磁盘 |
| P1 | 临时文件合并使用 `read_to_end` 一次性读入单表 SQL 文件 | 大表导出时内存峰值不可控 |
| P1 | Canonical 模型过粗，跨库 DDL 会丢长度、精度、默认值、约束语义 | 生成目标库结构可能不等价 |
| P1 | Shentong 大 LOB 追加写入假设第一列可唯一定位记录 | 无主键或首列非唯一时可能更新错行 |
| P1 | 前端选择表时只保存表名，导出时统一写入 `config.schema` | 放大多 schema 错表风险 |
| P2 | 大量中文提示、注释和生成 SQL 说明出现 mojibake | 用户不可读，也影响排障 |

## 2. 当前跨库能力现状

### 2.1 已注册执行路径

执行路径集中在 `backend/src/export/orchestrator.rs:31` 到 `backend/src/export/orchestrator.rs:95`。当前显式支持的组合如下：

| 源库 | 目标库 | 当前路径 | 状态判断 |
|---|---|---|---|
| DM8 | DM8 | Legacy DM8 | 主路径，能力最完整 |
| DM8 | MySQL | DM8 to MySQL PoC | 有专用 DDL 映射和数据管线 |
| DM8 | Kingbase | DM8 to Kingbase PoC | 有专用 DDL 映射和数据管线 |
| DM8 | Shentong | DM8 to Shentong PoC | 有专用 DDL 映射和数据管线，但数据路径更保守 |
| MySQL | MySQL | MySQL PoC | 数据流式导出较好，DDL 依赖通用 Canonical |
| MySQL | DM8 | MySQL to DM8 PoC | 数据流式导出，DDL 语义有限 |
| MySQL | Kingbase | MySQL to Kingbase PoC | 数据流式导出，DDL 语义有限 |
| Kingbase | Kingbase | Kingbase PoC | 数据已使用 `query_raw` 流式路径，但 schema 和解码问题突出 |
| Kingbase | MySQL | Kingbase to MySQL PoC | 同上 |
| Kingbase | DM8 | Kingbase to DM8 PoC | 同上 |
| Shentong | Shentong | Shentong PoC | 部分能力，仅同库方向 |

未覆盖或基本不可用的组合包括 Shentong 到其他目标库、MySQL 到 Shentong、Kingbase 到 Shentong。能力报告会把这些方向标成不支持，但前端仍会展示所有目标方言选项，依赖后端 capability 拦截。

### 2.2 能力报告与真实执行能力存在偏差

能力报告由源端 adapter 与目标 renderer 合成，位置在 `backend/src/export/capability.rs:11` 到 `backend/src/export/capability.rs:45`。源端能力定义在 `backend/src/source/mod.rs:23` 到 `backend/src/source/mod.rs:216`，目标端能力分散在 `backend/src/dialect/mod.rs` 和 `backend/src/dialect/mysql_renderer.rs`。

这个机制能给 UI 一个大致“full/partial/none”提示，但它不是由真实执行路径自动生成的，因此存在两个偏差：

| 偏差 | 说明 |
|---|---|
| 静态声明偏差 | adapter 只声明能力，不保证当前 PoC 路径实际完整保留该对象语义 |
| 运行时约束偏差 | `build_runtime_export_capability_report` 只根据路径可用性修正能力，无法表达字段级、类型级、对象级的降级 |

例如 include row counts 的支持先在 `backend/src/export/capability.rs:80` 到 `backend/src/export/capability.rs:90` 以源目标对静态判断，随后又在 `backend/src/export/capability.rs:117` 到 `backend/src/export/capability.rs:139` 通过 execution path 覆盖。这说明当前 capability 已经出现了“静态能力”和“实际路径能力”双重来源，后续容易继续漂移。

## 3. 关键正确性问题

### 3.1 P0：Kingbase 多 schema 同名表可能导出错表

现象：Kingbase/PostgreSQL native 层用 `config.schema` 作为数据库名连接，同时表列表又返回多个 PostgreSQL schema 下的表。真正导出数据时，代码只把表名传给 `resolve_table_schema`，然后按表名查找首个 user schema。

证据：

| 位置 | 说明 |
|---|---|
| `backend/src/db/pg_native.rs:65` 到 `backend/src/db/pg_native.rs:77` | `config.schema` 被用于连接 dbname |
| `backend/src/db/pg_native.rs:104` 到 `backend/src/db/pg_native.rs:135` | 表列表读取多个 namespace/schema |
| `backend/src/db/pg_native.rs:169` 到 `backend/src/db/pg_native.rs:190` | `resolve_table_schema` 只按表名选第一个 schema |
| `backend/src/export/kingbase_poc.rs:247` 到 `backend/src/export/kingbase_poc.rs:258` | 导出时传入 `source_table.name`，没有传 schema |
| `backend/src/export/kingbase_to_other_poc.rs:373` 到 `backend/src/export/kingbase_to_other_poc.rs:385` | 跨库路径同样按表名反查 schema |
| `frontend/src/components/ExportConfig.tsx:343` | 前端把 `selectedTables` 统一映射成 `{ schema: config.schema, name }` |
| `frontend/src/components/SchemaExplorer.tsx:241` | 表格选择的 row key 是 `name`，不是 `schema.name` |

影响：如果 `public.user`、`crm.user`、`audit.user` 同时存在，前端选择和后端导出都无法稳定区分它们。结果可能是导出错表，且导出的 SQL 看起来完全合法。

建议：

| 步骤 | 修复方向 |
|---|---|
| 1 | `Table` 类型增加 `schema` 字段，前端 selection key 改成稳定的 `schema.name` 或结构化 key |
| 2 | `ExportRequest.tables` 必须保留真实 source schema，不再统一使用 `config.schema` |
| 3 | Kingbase 数据导出函数直接使用 `TableIdentifier.schema`，删除按表名反查 schema 的路径 |
| 4 | `ConnectionConfig` 拆分 `database` 与 `schema/search_path`，避免 `schema` 字段一处表示数据库、一处表示 namespace |

### 3.2 P0：数据解码或类型转换失败会静默变成 NULL

现象：多个导出路径在解析数据库值失败时返回 `CanonicalValue::Null` 或 `None`，而不是报错。这是跨库转换里最危险的一类问题，因为用户最终得到的 INSERT 文件可以成功执行，但数据已经被悄悄改写。

证据：

| 位置 | 说明 |
|---|---|
| `backend/src/export/kingbase_poc.rs:304` 到 `backend/src/export/kingbase_poc.rs:358` | `try_get(...).ok().flatten().unwrap_or(CanonicalValue::Null)` |
| `backend/src/export/kingbase_to_other_poc.rs:430` 到 `backend/src/export/kingbase_to_other_poc.rs:484` | 跨库 Kingbase 路径同样吞掉解码错误 |
| `backend/src/export/shentong_poc.rs:274` 到 `backend/src/export/shentong_poc.rs:281` | `row.get(...).unwrap_or(None)` 把读取失败转成 NULL |
| `backend/src/export/pipeline.rs:750` 到 `backend/src/export/pipeline.rs:784` | DM8 pipeline 的整数、浮点、二进制解析失败会返回 Null |

影响：数值溢出、驱动解码异常、时间格式差异、二进制 HEX 错误、字符集错误都可能被转换为 NULL。严格模式目前主要用于 capability 和 row counts，不足以防止这类数据损坏。

建议：

| 步骤 | 修复方向 |
|---|---|
| 1 | 所有 parse/decode 失败默认返回错误，错误信息包含表、列、源类型、目标类型、原始值摘要 |
| 2 | 如果确实需要容错，必须由显式 `lossy_mode` 或非 strict 降级开关控制，并在 summary 里记录跳过/降级数量 |
| 3 | 对 Kingbase、Shentong、DM8 pipeline 增加坏值回归测试，覆盖非法整数、非法二进制、超长文本、无效时间 |
| 4 | 导出文件头写入数据转换警告，避免用户只看到“导出成功” |

### 3.3 P1：Canonical 模型过粗，跨库 DDL 天然会丢语义

现象：通用跨库路径把源表结构降到 `CanonicalTable` 和 `CanonicalColumn`，但 Canonical 只保留名称、粗粒度逻辑类型、nullable、identity、主键。长度、精度、scale、字符语义、默认值、注释、唯一约束、外键、check、索引、触发器、序列等都不是 Canonical 一等信息。

证据：

| 位置 | 说明 |
|---|---|
| `backend/src/domain/canonical/mod.rs:7` 到 `backend/src/domain/canonical/mod.rs:58` | Canonical schema 只有粗粒度字段 |
| `backend/src/domain/canonical/mod.rs:71` 到 `backend/src/domain/canonical/mod.rs:107` | 类型推断依赖字符串包含和少量规则 |
| `backend/src/dialect/mysql_renderer.rs:88` 到 `backend/src/dialect/mysql_renderer.rs:168` | MySQL 通用 DDL 只渲染列和主键，String 固定 `VARCHAR(255)` |
| `backend/src/dialect/mod.rs:699` 到 `backend/src/dialect/mod.rs:712` | Kingbase 通用类型映射也把 String 固定为 `VARCHAR(255)` |
| `backend/src/dialect/mod.rs:885` 到 `backend/src/dialect/mod.rs:898` | DM8 通用类型映射同样使用粗粒度类型 |

影响：MySQL 到 Kingbase、MySQL 到 DM8、Kingbase 到 MySQL、Kingbase 到 DM8 等非 DM8 源路径即使能生成 DDL，也无法保证结构等价。典型丢失包括 `DECIMAL(18,2)` 变成 `DECIMAL(38,10)`、`VARCHAR(4000 CHAR)` 变成 `VARCHAR(255)`、默认表达式和注释丢失、check/trigger/sequence 丢失。

建议：

| 步骤 | 修复方向 |
|---|---|
| 1 | 扩展 Canonical 类型，增加 native type、length、precision、scale、char semantics、default expression、comment |
| 2 | 把 constraints、indexes、triggers、sequences 纳入 Canonical 或者明确分成高级对象模型 |
| 3 | 通用 renderer 不应在缺失长度时默默使用 `VARCHAR(255)`，至少需要 warning 或保守映射为 TEXT |
| 4 | DM8 专用映射中已有较多类型细节，可作为扩展 Canonical 的参考 |

### 3.4 P1：Shentong 大 LOB 追加写入假设第一列可以唯一定位

现象：Shentong renderer 的大 LOB 写入逻辑会先插入一行，再用第一列作为 `WHERE` 条件更新 LOB 分块。代码里明确用 `col_names.first()` 和 `column_values[0]` 作为定位条件。

证据：

| 位置 | 说明 |
|---|---|
| `backend/src/dialect/mod.rs:669` 到 `backend/src/dialect/mod.rs:672` | 使用第一列作为 PK/唯一定位列 |
| `backend/src/dialect/mod.rs:679` 到 `backend/src/dialect/mod.rs:685` | 大 BLOB 分块 UPDATE 使用该列作为 where 条件 |
| `backend/src/dialect/mod.rs:770` 到 `backend/src/dialect/mod.rs:835` | Kingbase 路径已有基于真实 primary key 的实现，可对照修复 |

影响：如果第一列不是主键、不是唯一键、可为空、或导出列顺序变化，LOB 分块 UPDATE 可能更新多行、更新错行，或者完全失败。该问题会直接影响 DM8 到 Shentong 数据导出，因为 `backend/src/export/dm8_to_shentong_poc.rs:867` 到 `backend/src/export/dm8_to_shentong_poc.rs:878` 启用了 Shentong 数据管线。

建议：

| 步骤 | 修复方向 |
|---|---|
| 1 | 复用 Kingbase 的 primary key predicate 方案 |
| 2 | 无主键且存在大 LOB 时直接报错或切换到数据库支持的临时键策略 |
| 3 | summary 中明确记录哪些表因大 LOB 且缺少 PK 被跳过 |

## 4. 性能问题

### 4.1 P0：`max_parallelism` 计算后没有被真正执行

现象：统一数据导出管线先按 FK layer 排序，然后在每一层内并行导出。代码计算了 `effective_parallelism = layer.len().min(max_parallelism)`，但实际只用它判断是否走串行分支；一旦进入并行分支，就对 layer 内所有表都 spawn 一个线程。

证据：

| 位置 | 说明 |
|---|---|
| `backend/src/export/pipeline.rs:117` 到 `backend/src/export/pipeline.rs:145` | FK layer 排序和 writer 初始化 |
| `backend/src/export/pipeline.rs:215` 到 `backend/src/export/pipeline.rs:236` | 构建连接字符串并计算 `effective_parallelism` |
| `backend/src/export/pipeline.rs:256` 到 `backend/src/export/pipeline.rs:337` | 实际对 layer 内所有表 spawn，没有按 `max_parallelism` 限流 |

影响：如果一个 FK layer 内有 100 张无依赖表，配置 `max_parallelism: 4` 仍可能同时创建 100 个线程、100 个 ODBC 连接和 100 个临时文件写入。这会放大数据库连接压力、驱动不稳定性、磁盘 IO 抖动，并可能导致导出失败。

建议：

| 步骤 | 修复方向 |
|---|---|
| 1 | 使用 bounded worker pool 或按 `max_parallelism` 对 layer 分 chunk 执行 |
| 2 | 每个 worker 复用连接或使用真实连接池，不要每张表创建新连接 |
| 3 | 按表大小估算调度，把大表和小表混排，避免尾部单大表拖慢 |
| 4 | summary 中记录实际并发数、连接数、每表耗时 |

### 4.2 P1：临时文件合并一次性读入内存

现象：并行导出每张表写入一个临时文件，然后主线程合并。合并时使用 `read_to_end(&mut buf)` 把整个单表 SQL 文件读入内存，再写入主输出文件。

证据：

| 位置 | 说明 |
|---|---|
| `backend/src/export/pipeline.rs:273` 到 `backend/src/export/pipeline.rs:277` | 每个 worker 创建单表临时文件 |
| `backend/src/export/pipeline.rs:339` 到 `backend/src/export/pipeline.rs:347` | 合并时对每个临时文件 `read_to_end` |

影响：一个大表生成 2GB INSERT SQL 时，合并阶段就可能尝试分配 2GB 内存。前面的导出阶段虽然用了批处理和文件缓冲，但这里会把流式优势抵消掉。

建议：

| 步骤 | 修复方向 |
|---|---|
| 1 | 使用 `std::io::copy` 或固定大小 buffer 流式复制临时文件 |
| 2 | 临时文件合并后及时删除，失败时也需要清理 |
| 3 | 对单表输出文件大小加统计，超阈值时降低 batch size 或提示用户 |

### 4.3 P1：include row counts 会触发逐表 `COUNT(*)`

现象：旧 DM8 数据导出路径在 include row counts 打开时，会对每张表执行真实 `COUNT(*)`。虽然表列表处使用了 `ALL_TABLES.NUM_ROWS` 估算行数，但数据导出的预扫描注释仍可能触发全表计数。

证据：

| 位置 | 说明 |
|---|---|
| `backend/src/db/schema.rs:127` 到 `backend/src/db/schema.rs:164` | 表列表使用统计行数，性能较好但可能不准 |
| `backend/src/db/schema.rs:439` 到 `backend/src/db/schema.rs:465` | `fetch_row_count` 执行真实 `COUNT(*)` |
| `backend/src/export/data.rs:494` 到 `backend/src/export/data.rs:503` | include row counts 时逐表调用 `fetch_row_count` |
| `backend/src/export/orchestrator.rs:136` 到 `backend/src/export/orchestrator.rs:138` | 目前只有 Legacy DM8 支持 include row counts |

影响：对大表执行 `COUNT(*)` 会产生明显延迟和数据库压力。前端默认关闭该选项是正确的，但用户一旦打开，导出前可能长时间无真实进度。

建议：

| 步骤 | 修复方向 |
|---|---|
| 1 | 把选项文案明确为“真实 COUNT，可能很慢”或改成估算行数 |
| 2 | 如果保留真实 COUNT，需要 per-table progress 和取消能力 |
| 3 | strict mode 不应影响性能型选项，capability 应清楚表达成本 |

### 4.4 P1：ODBC `ConnectionPool` 不是实际连接池

现象：`ConnectionPool` 封装了共享 ODBC Environment 和连接计数，但每次 `get_connection` 都创建新连接，没有复用连接对象。统一 pipeline 又自己创建新的 `Environment`，没有使用该 pool。

证据：

| 位置 | 说明 |
|---|---|
| `backend/src/db/pool.rs:28` 到 `backend/src/db/pool.rs:36` | pool 持有 Environment、配置和计数 |
| `backend/src/db/pool.rs:132` 到 `backend/src/db/pool.rs:147` | `get_connection` 每次创建连接 |
| `backend/src/export/pipeline.rs:215` 到 `backend/src/export/pipeline.rs:220` | pipeline 自己创建 ODBC Environment |

影响：命名为 pool 容易误导维护者；并行管线下每表新连接会进一步放大连接压力。ODBC 驱动对多线程和多连接的稳定性通常比纯 Rust native driver 更敏感。

建议：

| 步骤 | 修复方向 |
|---|---|
| 1 | 要么重命名为 `ConnectionFactory`，要么实现真实连接池 |
| 2 | pipeline 复用连接工厂和统一限流策略 |
| 3 | 对 DM8、Kingbase ODBC 驱动分别设置默认并发上限 |

### 4.5 P2：Shentong unicode safe 模式会把很多表降到 rowwise

现象：pipeline 的 `should_use_rowwise_export` 在 `unicode_safe_text` 打开且表存在字符列时返回 true。DM8 到 Shentong 配置了 `unicode_safe_text: true`，因此很多普通文本表会走逐行读取路径。

证据：

| 位置 | 说明 |
|---|---|
| `backend/src/export/pipeline.rs:437` 到 `backend/src/export/pipeline.rs:442` | 字符列加 unicode safe 会触发 rowwise |
| `backend/src/export/dm8_to_shentong_poc.rs:867` 到 `backend/src/export/dm8_to_shentong_poc.rs:878` | DM8 到 Shentong 开启 `unicode_safe_text` |
| `backend/src/export/pipeline.rs:524` 到 `backend/src/export/pipeline.rs:645` | rowwise 路径逐行读取并处理 LOB/text |

影响：正确性上更稳，但性能会明显下降。对于纯 `VARCHAR` 小文本表也进入 rowwise，可能比 batch fast path 慢很多。

建议：

| 步骤 | 修复方向 |
|---|---|
| 1 | 区分普通窄字符列、大文本列、可能乱码列，不要表级一刀切 |
| 2 | 优先尝试 TextRowSet 的宽字符或安全解码策略 |
| 3 | 在 summary 中提示哪些表因为 unicode safe 降级为 rowwise |

### 4.6 P2：前端进度不是实际导出进度

现象：前端对 DDL 和 Data 各算一个步骤，`exportDDL` 和 `exportData` 都是请求完成后才更新。大数据导出期间没有每表、每行、每阶段进度。

证据：

| 位置 | 说明 |
|---|---|
| `frontend/src/services/api.ts:609` | `exportData` 使用单次 HTTP 请求 |
| `frontend/src/components/ExportConfig.tsx:384` | 数据导出请求完成后才处理结果 |
| `frontend/src/components/ExportConfig.tsx:501` 到 `frontend/src/components/ExportConfig.tsx:516` | UI 暴露 row count 和 batch size，但没有真实进度反馈 |

影响：大表导出会表现为“进度条卡住”，用户难以判断是慢、阻塞还是失败。性能问题会被感知为可靠性问题。

建议：

| 步骤 | 修复方向 |
|---|---|
| 1 | 后端导出任务改成 job，前端轮询 job progress |
| 2 | progress 至少包含当前表、已完成表数、已导出行数、当前阶段 |
| 3 | 提供取消导出能力，清理部分输出和临时文件 |

## 5. 数据库转换语义问题

### 5.1 DM8 到 MySQL

DM8 到 MySQL 是当前专用映射最完整的跨库方向之一。`backend/src/export/dm8_to_mysql_poc.rs:21` 到 `backend/src/export/dm8_to_mysql_poc.rs:214` 处理了 NUMBER、BLOB、CLOB、RAW、VARCHAR、默认值、identity、comment 等细节。`backend/src/export/dm8_to_mysql_poc.rs:306` 到 `backend/src/export/dm8_to_mysql_poc.rs:426` 还考虑了 MySQL 65535 row size、索引字节数和 VARCHAR 降级。

主要风险：

| 风险 | 说明 |
|---|---|
| 类型降级不可逆 | 超长 VARCHAR 降级 TEXT、LOB 默认值丢弃属于合理降级，但需要明确记录到 summary |
| 字符集和索引长度 | utf8mb4 下索引长度、row size、前缀索引策略仍需真实 MySQL 版本验证 |
| 数据格式 | MySQL 数据路径把非二进制列 `CAST(... AS CHAR)`，时间、JSON、decimal 格式依赖驱动输出 |

### 5.2 DM8 到 Kingbase

DM8 到 Kingbase 在类型接近度上较好，映射在 `backend/src/export/dm8_to_kingbase_poc.rs:28` 到 `backend/src/export/dm8_to_kingbase_poc.rs:260`。DDL 路径覆盖表、主键、唯一约束、check、索引、外键、序列、触发器，见 `backend/src/export/dm8_to_kingbase_poc.rs:334` 到 `backend/src/export/dm8_to_kingbase_poc.rs:448`。

主要风险：

| 风险 | 说明 |
|---|---|
| Oracle 兼容模式依赖 | 输出里提示需要 Kingbase Oracle 兼容模式，但代码无法强制目标库模式 |
| trigger/sequence 语义 | 生成语法不代表行为完全等价，需要 golden case 验证 |
| 并行管线问题 | 数据导出复用统一 pipeline，因此受到无界并发和临时文件内存问题影响 |

### 5.3 DM8 到 Shentong

DM8 到 Shentong 有专用映射，位置在 `backend/src/export/dm8_to_shentong_poc.rs:27` 到 `backend/src/export/dm8_to_shentong_poc.rs:130`。它处理了 Shentong varchar 字节限制、索引列 fallback、identity 整数约束、触发器适配等。

主要风险：

| 风险 | 说明 |
|---|---|
| 字符长度语义需要复核 | `dm8_type_to_shentong` 注释和代码对 byte/char semantics 的描述存在不一致迹象 |
| 索引 LOB fallback 会改变列容量 | `backend/src/export/dm8_to_shentong_poc.rs:212` 到 `backend/src/export/dm8_to_shentong_poc.rs:237` 会把部分索引 LOB 降级为较短 VARCHAR/VARBINARY |
| 非 PK identity 可能失败 | `backend/src/export/dm8_to_shentong_poc.rs:261` 到 `backend/src/export/dm8_to_shentong_poc.rs:281` 已有相关限制 |
| 数据路径更慢 | unicode safe 触发行级路径，且大 LOB 写入有第一列定位风险 |

### 5.4 MySQL 作为源库

MySQL 源库数据导出使用 `sqlx::query(...).fetch` 流式读取，见 `backend/src/export/mysql_poc.rs:402` 到 `backend/src/export/mysql_poc.rs:426`。这比整表加载更合理。连接层还设置了 `REPEATABLE READ` 和一致性快照，见 `backend/src/export/mysql_poc.rs:109` 到 `backend/src/export/mysql_poc.rs:112`。

主要风险：

| 风险 | 说明 |
|---|---|
| DDL 语义有限 | `inspect_canonical_table` 只生成 CanonicalTable，见 `backend/src/export/mysql_poc.rs:298` 到 `backend/src/export/mysql_poc.rs:371` |
| 高级对象覆盖不足 | MySQL renderer 声明多个对象为 None 或 Partial，见 `backend/src/dialect/mysql_renderer.rs:21` 到 `backend/src/dialect/mysql_renderer.rs:70` |
| 类型字符串化 | `select_expr` 对非二进制列使用 `CAST(... AS CHAR)`，见 `backend/src/export/mysql_poc.rs:493` 到 `backend/src/export/mysql_poc.rs:499` |

### 5.5 Kingbase 作为源库

当前 Kingbase 数据路径已使用 `query_raw`，见 `backend/src/export/kingbase_poc.rs:261` 到 `backend/src/export/kingbase_poc.rs:273` 和 `backend/src/export/kingbase_to_other_poc.rs:387` 到 `backend/src/export/kingbase_to_other_poc.rs:399`。这意味着历史上“整表加载”的风险在当前代码中看起来已经缓解。

剩余主要风险：

| 风险 | 说明 |
|---|---|
| 多 schema 错表 | 见本报告 3.1 |
| 解码错误变 NULL | 见本报告 3.2 |
| FK 元数据错误被吞掉 | `backend/src/export/kingbase_to_other_poc.rs:91` 到 `backend/src/export/kingbase_to_other_poc.rs:94` 和 `backend/src/export/kingbase_to_other_poc.rs:223` 到 `backend/src/export/kingbase_to_other_poc.rs:226` 使用 `unwrap_or_default` |

### 5.6 Shentong 作为源库

Shentong 当前只支持 Shentong 到 Shentong 的部分路径。数据导出用单连接查询，见 `backend/src/export/shentong_poc.rs:86` 到 `backend/src/export/shentong_poc.rs:95` 和 `backend/src/export/shentong_poc.rs:252` 到 `backend/src/export/shentong_poc.rs:281`。

主要风险：

| 风险 | 说明 |
|---|---|
| 能力覆盖明显不足 | source adapter 中 indexes、unique、FK、check、triggers、sequences 多数为 None |
| 查询是否真正流式依赖驱动 | 代码层是迭代 rows，但是否整表 materialize 需要真实驱动验证 |
| 解码错误变 NULL | `row.get(...).unwrap_or(None)` 会吞掉读取错误 |

## 6. 编码与可维护性问题

### 6.1 中文 mojibake 已经影响产品可用性

多个 Rust 和 TypeScript 文件中出现乱码中文字符串，包括能力说明、错误提示、UI 文案、生成 SQL 注释。典型位置如下：

| 位置 | 说明 |
|---|---|
| `backend/src/models/mod.rs:285` | include row counts 默认说明乱码 |
| `backend/src/export/orchestrator.rs:68` 到 `backend/src/export/orchestrator.rs:76` | 不支持路径提示乱码 |
| `backend/src/export/capability.rs:83` 到 `backend/src/export/capability.rs:136` | capability note 乱码 |
| `backend/src/export/data.rs:519` 到 `backend/src/export/data.rs:579` | 生成 SQL 文件头和注释乱码 |
| `frontend/src/components/ExportConfig.tsx:158` 到 `frontend/src/components/ExportConfig.tsx:210` | 导出能力和 row count 提示乱码 |
| `frontend/src/services/api.ts` | Tauri 目录选择错误提示乱码 |

影响：用户无法理解错误原因，研发也难以通过生成文件排障。对于跨库工具，这类提示实际上是产品能力的一部分。

建议：

| 步骤 | 修复方向 |
|---|---|
| 1 | 统一仓库文件编码为 UTF-8 |
| 2 | 批量替换用户可见乱码文案 |
| 3 | 增加最小文案快照检查，防止再次提交 mojibake |

### 6.2 方言渲染和执行路径边界不够清晰

当前有三类实现混合存在：专用 PoC、通用 Canonical renderer、legacy DM8。它们各自处理一部分元数据、类型转换和数据导出，导致 capability、实际行为和 UI 暴露能力不容易对齐。

建议把执行注册表作为唯一事实来源：

| 模块 | 建议职责 |
|---|---|
| Source introspector | 只负责读取源库元数据和数据流 |
| Canonical model | 明确表示可迁移语义和损失信息 |
| Dialect planner | 决定目标库 DDL、约束、降级策略和 warning |
| Data writer | 只负责稳定流式写 INSERT、LOB、事务控制 |
| Capability registry | 从真实 path 和 planner 能力生成，不手写重复声明 |

## 7. 建议修复路线

### 7.1 第一阶段：先阻止错表、错数据和资源失控

建议优先修复以下事项：

| 顺序 | 修复项 | 理由 |
|---|---|---|
| 1 | Kingbase/TableIdentifier 全链路携带 schema | 防止导出错表 |
| 2 | Kingbase、Shentong、DM8 数据解析失败改为显式错误 | 防止静默数据损坏 |
| 3 | pipeline 并发改成真正 bounded parallelism | 防止大 schema 导出打爆资源 |
| 4 | 临时文件合并改成流式复制 | 防止大表合并 OOM |
| 5 | Shentong 大 LOB 改成真实 PK 定位 | 防止更新错行 |
| 6 | 修复用户可见 mojibake | 提升可用性和排障效率 |

### 7.2 第二阶段：提升跨库语义保真

建议按对象类型逐步补齐，而不是一次性重写：

| 对象 | 建议 |
|---|---|
| Column type | 增加 raw type、length、precision、scale、char semantics |
| Default | 区分 literal default、function default、sequence/identity default |
| Constraint | 统一 primary key、unique、foreign key、check 表达模型 |
| Index | 增加索引类型、表达式索引、前缀长度、排序、函数索引降级策略 |
| Trigger/Sequence | 明确哪些目标库能保留，哪些只能注释输出 |
| Warning | 每次语义降级都写入 summary 和 SQL 文件头 |

### 7.3 第三阶段：建立验证矩阵

建议建立最小但高价值的 golden case：

| 类别 | 用例 |
|---|---|
| 类型 | NUMBER precision/scale、VARCHAR byte/char semantics、DATE/TIMESTAMP、CLOB/BLOB、RAW |
| 约束 | PK、UK、FK、CHECK、索引、触发器、序列、identity |
| 数据 | NULL、中文、emoji、单引号、反斜杠、二进制、超大 LOB、非法日期 |
| Schema | 多 schema 同名表、跨 schema FK |
| 性能 | 100 张无依赖表、深 FK 链、单大表、LOB 表、混合大小表 |
| UI | 多 schema 选择、capability 禁用、batch size、include row counts 警告 |

## 8. 重点代码参考

| 文件 | 关注点 |
|---|---|
| `backend/src/export/orchestrator.rs` | 跨库执行路径注册和运行时能力约束 |
| `backend/src/export/capability.rs` | capability 合成和 include row counts 支持判断 |
| `backend/src/export/pipeline.rs` | 并行数据导出、批处理、LOB、临时文件合并 |
| `backend/src/export/data.rs` | Legacy DM8 数据导出、行数预扫描、FK 排序 |
| `backend/src/domain/canonical/mod.rs` | Canonical 模型和通用类型推断 |
| `backend/src/dialect/mod.rs` | DM8、Kingbase、Shentong 方言渲染和 LOB 写入 |
| `backend/src/dialect/mysql_renderer.rs` | MySQL 通用 DDL 和 INSERT 渲染 |
| `backend/src/export/dm8_to_mysql_poc.rs` | DM8 到 MySQL 专用 DDL 和数据管线入口 |
| `backend/src/export/dm8_to_kingbase_poc.rs` | DM8 到 Kingbase 专用 DDL 和数据管线入口 |
| `backend/src/export/dm8_to_shentong_poc.rs` | DM8 到 Shentong 专用 DDL 和数据管线入口 |
| `backend/src/export/mysql_poc.rs` | MySQL 源数据流式导出 |
| `backend/src/export/kingbase_poc.rs` | Kingbase 同库数据导出和 schema 解析问题 |
| `backend/src/export/kingbase_to_other_poc.rs` | Kingbase 跨库数据导出和解码问题 |
| `backend/src/export/shentong_poc.rs` | Shentong 同库 PoC 数据导出 |
| `backend/src/db/schema.rs` | DM8 元数据读取、统计行数、真实 COUNT |
| `backend/src/db/pg_native.rs` | Kingbase native 元数据、schema/dbname 混用 |
| `backend/src/db/pool.rs` | ODBC 连接工厂和伪 pool |
| `frontend/src/components/SchemaExplorer.tsx` | 前端表选择只按 name keyed |
| `frontend/src/components/ExportConfig.tsx` | 导出请求构造、capability 检查、进度展示 |
| `frontend/src/services/api.ts` | 前端缓存、批量表详情、导出 API 调用 |

## 9. 最终判断

项目当前适合继续以 DM8 为主源库推进，尤其 DM8 到 MySQL、DM8 到 Kingbase、DM8 到 Shentong 已经有较多专用映射基础。但如果要把它定位为可靠的通用跨数据库转换工具，需要先处理 P0 问题，否则很容易出现“导出成功但数据或表对象不正确”的情况。

性能方面，最大的问题不是单个 batch size，而是统一 pipeline 的并发限流失效和临时文件合并内存峰值。修复这两个点通常能显著降低大 schema 导出的失败率，并且改动边界相对清晰。

建议下一步优先实施第一阶段修复，并为 Kingbase 多 schema、解码失败、pipeline 并发限制、Shentong LOB 定位四类问题补回归测试。
