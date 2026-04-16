# 全局代码审阅报告（多视角）

- 审阅日期：2026-03-09
- 审阅范围：`backend`、`frontend`、`src-tauri`（基于当前工作树）
- 审阅方式：静态代码审阅 + 构建/测试验证
- 视角覆盖：架构师、测试、产品、公司管理层

## 1. 执行摘要

当前版本在“后端单测稳定性、前端可构建性”方面表现良好，但仍存在数个影响发布质量的关键风险：

1. MySQL 导出路径对标识符限制过严，可能导致合法表/列无法导出（高）。
2. 导出 UI 在“部分失败”场景下可能错误呈现“成功完成”（高）。
3. 默认连接读取策略在多数据库类型并存时存在歧义（中）。
4. 本地凭据加密键管理在 Windows 下权限控制不足（中）。
5. 集成测试全部 `ignore`，多数据库真实链路在常规回归中没有执行（中）。
6. 桌面端（Tauri）构建验证受环境权限和磁盘影响，当前不可作为发布就绪证据（中）。

## 2. 验证证据

已执行命令与结果：

- `backend: cargo test`
  - 结果：通过，`97 passed / 0 failed`
  - 但集成测试：`22 ignored`（未纳入常规回归）
- `frontend: npm run build`
  - 结果：通过
  - 产物告警：存在 `>500kB` 的大 chunk（`vendor-antd-core`）
- `frontend: npm run lint`
  - 结果：通过
- `src-tauri: cargo check`
  - 结果：失败
  - 原因：先是 `target` 路径权限拒绝，后在临时目标目录编译时出现“磁盘空间不足（os error 112）”

## 3. 关键发现（按严重度）

### 高：MySQL 导出对合法对象名兼容不足（架构/产品）

- 证据：
  - [backend/src/export/mysql_poc.rs:273](/E:/self/tool-database/backend/src/export/mysql_poc.rs:273)
  - [backend/src/export/mysql_poc.rs:396](/E:/self/tool-database/backend/src/export/mysql_poc.rs:396)
- 问题：
  - 导出时强制 `ensure_identifier` 只允许 ASCII 字母数字、`_`、`$`。
  - 实际 MySQL 合法对象名可通过反引号支持更多字符（如中文、连字符等）。
- 影响：
  - 对已有真实库的兼容性下降，出现“可浏览但不可导出”的产品故障。
- 建议：
  - 由“白名单字符校验”改为“严格引用 + 参数化元数据获取 + 黑名单最小化”策略。

### 高：导出结果状态在部分失败时可能误报成功（产品/管理层）

- 证据：
  - [frontend/src/components/ExportConfig.tsx:236](/E:/self/tool-database/frontend/src/components/ExportConfig.tsx:236)
  - [frontend/src/components/ExportConfig.tsx:263](/E:/self/tool-database/frontend/src/components/ExportConfig.tsx:263)
  - [frontend/src/components/ExportConfig.tsx:294](/E:/self/tool-database/frontend/src/components/ExportConfig.tsx:294)
  - [frontend/src/utils/exportProgress.ts:21](/E:/self/tool-database/frontend/src/utils/exportProgress.ts:21)
- 问题：
  - DDL 失败后，若数据导出成功，后续状态会被覆盖为 `success`，且页面会展示“导出完成”。
- 影响：
  - 用户误判导出质量，可能将不完整脚本投入生产使用。
- 建议：
  - 引入“任务级最终状态”聚合器（success/partial_failed/failed），禁止后续步骤覆盖失败态。

### 中：默认连接读取存在跨数据库类型歧义（架构/产品）

- 证据：
  - [backend/src/config_store/mod.rs:159](/E:/self/tool-database/backend/src/config_store/mod.rs:159)
  - [backend/src/config_store/mod.rs:218](/E:/self/tool-database/backend/src/config_store/mod.rs:218)
- 问题：
  - 保存默认连接按 `default-{db_type}` 分名；读取默认连接却使用 `LIKE 'default-%' ORDER BY updated_at DESC LIMIT 1`。
- 影响：
  - 当用户维护多种数据库默认连接时，可能加载到“最新更新但非当前期望类型”的配置。
- 建议：
  - `GET /api/config/connection` 增加 `db_type` 参数并按类型精确读取。

### 中：本地加密键管理在 Windows 权限控制不足（架构/管理层）

- 证据：
  - [backend/src/config_store/mod.rs:64](/E:/self/tool-database/backend/src/config_store/mod.rs:64)
  - [backend/src/config_store/mod.rs:77](/E:/self/tool-database/backend/src/config_store/mod.rs:77)
- 问题：
  - `.key` 与 `config.db` 同目录存放；权限收敛仅在 `unix` 生效，Windows 下无 ACL 收敛逻辑。
- 影响：
  - 凭据“加密”更接近“同机可逆混淆”，对合规审计（尤其企业环境）解释成本高。
- 建议：
  - Windows 接入 DPAPI/Credential Manager；至少补充 ACL 限制与密钥轮换方案。

### 中：集成测试默认全部忽略，真实链路未进入常规回归（测试/管理层）

- 证据：
  - [backend/tests/integration_db.rs:3](/E:/self/tool-database/backend/tests/integration_db.rs:3)
  - [backend/tests/integration_db.rs:153](/E:/self/tool-database/backend/tests/integration_db.rs:153)
- 问题：
  - `cargo test` 默认不跑任何真实数据库链路（本次验证中 22 项全部 ignored）。
- 影响：
  - 多源多方言改动发生回归时，发现时点后移到人工联调阶段。
- 建议：
  - 建立最小可运行的 CI 冒烟矩阵（至少 1 条 DM8、1 条 MySQL、1 条 Kingbase）。

### 中：桌面端构建验证当前不具备发布可信度（管理层/测试）

- 证据：
  - `src-tauri cargo check` 实测失败：权限拒绝 + 磁盘空间不足
- 问题：
  - 当前环境下无法形成“桌面端可编译”证据链。
- 影响：
  - 桌面发行路径存在不可预期交付风险。
- 建议：
  - 单独建立“可复现构建机”与磁盘水位门禁（例如 <15% 剩余空间阻止构建）。

## 4. 架构、测试、产品、管理层视角结论

### 架构师视角

- 优点：后端分层已初步形成（api/db/export/source/dialect），可演进性比早期版本更好。
- 风险：导出路径中重复实现较多（多文件重复 `parse_* / inspect_*` 逻辑），后续跨方言修复容易漏改。

### 测试视角

- 优点：后端单元测试覆盖面较广，核心路径有回归保护。
- 风险：真实数据库链路自动化不足，回归检测仍依赖人工环境。

### 产品视角

- 优点：多数据库类型与能力探测已具备，基础流程可走通。
- 风险：导出状态呈现与实际执行结果可能不一致；特殊命名对象兼容性不足会直接影响可用性。

### 公司管理层视角

- 当前状态：可继续开发迭代，不建议直接作为“跨库导出稳定版”对外承诺。
- 主要投入点：
  - 质量投入：补齐集成测试常态化执行；
  - 稳定性投入：修复导出状态聚合与命名兼容；
  - 发布投入：桌面端可复现构建环境治理。

## 5. 建议的整改优先级（两周窗口）

1. P0（本周）：修复“部分失败被显示为成功”的状态机逻辑。
2. P0（本周）：放宽 MySQL 导出标识符策略（改为严格引用而非过度限制）。
3. P1（下周）：默认连接按 `db_type` 精确读取。
4. P1（下周）：Windows 端本地密钥安全策略（DPAPI/ACL）。
5. P1（并行）：建立最小集成测试 CI 冒烟链路。
6. P2：导出模块去重重构，降低后续维护成本。

## 6. 审阅结论

本轮代码质量较上次有明显进展，后端和前端基础可用性已具备；但从“可发布、可承诺、可规模化维护”的标准看，仍需先处理上述高/中风险项，尤其是导出状态一致性与真实链路测试覆盖。
