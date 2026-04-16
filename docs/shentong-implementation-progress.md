# 神通数据库（Shentong OSCAR）支持 — 实施进度与问题排查

> 最后更新：2026-03-11

## 目录

1. [概述](#概述)
2. [已完成的功能](#已完成的功能)
3. [ODPI-C 层修复](#odpi-c-层修复)
4. [数据导出核心问题：表名解析](#数据导出核心问题表名解析)
5. [问题排查详细记录](#问题排查详细记录)
6. [当前未解决问题](#当前未解决问题)
7. [待清理项](#待清理项)
8. [相关文件清单](#相关文件清单)

---

## 概述

本项目通过 Shentong OSCAR 的原生 ACI（Application Call Interface，类似 Oracle OCI）接口实现对神通数据库的 DDL/数据导出支持。ACI 通过 ODPI-C（Oracle Database Programming Interface for C）封装层访问，底层使用 `aci.dll` 动态库。

**关键技术背景：**
- 神通 OSCAR 是一个基于 PostgreSQL 内核的国产数据库，提供 Oracle 兼容的 ACI 接口
- ACI 接口模仿 Oracle OCI，但并非完全兼容，存在多处行为差异
- 连接字符串格式：`host:port`（如 `192.168.3.34:2003`），不带数据库名
- `ACIClientVersion` 返回 `1.0.19.19.30`（版本号 1，非 Oracle 的 12+）
- 测试环境：`192.168.3.34:2003`，用户 `SYSDBA`，密码 `szoscar55`

---

## 已完成的功能

| 功能 | 状态 | 说明 |
|------|------|------|
| ACI 原生连接 | ✅ 正常 | `Connection::connect(user, pass, "host:port")` |
| 连接测试 | ✅ 正常 | `SELECT 1 FROM DUAL` |
| Schema 列表 | ✅ 正常 | `SELECT username FROM all_users` |
| 表列表 | ✅ 正常 | `SELECT owner, table_name FROM all_tables WHERE owner = :1` |
| 表详情（列+主键） | ✅ 正常 | `all_tab_columns` + `all_constraints`/`all_cons_columns` |
| DDL 导出 | ✅ 正常 | 通过 canonical model + DialectRenderer 生成 CREATE TABLE |
| 数据导出 | ❌ **未通过** | `SELECT * FROM table` 报 "Relation does not exist" |
| 索引/外键/触发器导出 | ⏳ 未实现 | 当前返回空数组 |

---

## ODPI-C 层修复

### 修复 1：returnCode 缓冲区零初始化（dpiVar.c）

**问题：** `DPI-1037: column at array position X fetched with error 65535/74`

**原因：** ODPI-C 为变长类型（VARCHAR2 等）分配 `returnCode` 缓冲区时使用 `malloc`（不清零）。Oracle OCI 会在 `ACIStmtFetch2` 后正确写入 returnCode，但 Shentong ACI **不写入** returnCode 缓冲区。未初始化的内存中的垃圾值（如 65535、74）被误判为 fetch 错误。

**修复：** 将 `dpiUtils__allocateMemory` 的 `clearMemory` 参数从 `0` 改为 `1`，使用 `calloc` 代替 `malloc`：

```c
// drivers/shentong/rust-shentong/odpi/src/dpiVar.c 约第 939 行
if (dpiUtils__allocateMemory(buffer->maxArraySize, sizeof(uint16_t), 1,  // 改为 1（清零）
        "allocate return code", (void**) &buffer->returnCode, error) < 0)
```

**状态：** ✅ 已修复，所有列查询（all_tab_columns 等）正常工作。

---

### 修复 2：serverStatus 垃圾值导致连接假死（dpiError.c）

**问题：** `DPI-1080: connection was closed by ORA-XXXXXXX`

**原因：** 当任何 SQL 执行出错后，ODPI-C 调用 `ACIAttrGet(DPI_OCI_ATTR_SERVER_STATUS)` 检查连接健康状态。Oracle OCI 返回 `0`（未连接）或 `1`（正常），但 Shentong ACI 返回垃圾值（如 `32758`）。原始逻辑 `if (serverStatus != 1) conn->deadSession = 1` 会把连接标记为已死，后续所有操作都会失败。

**修复：** 仅在 `attrGet` 调用本身失败或 `serverStatus == 0` 时标记连接死亡：

```c
// drivers/shentong/rust-shentong/odpi/src/dpiError.c 约第 168-177 行
int ssRc = dpiOci__attrGet(conn->serverHandle, DPI_OCI_HTYPE_SERVER,
        &serverStatus, NULL, DPI_OCI_ATTR_SERVER_STATUS,
        "get server status", error);
if (ssRc < 0 || serverStatus == 0) {
    conn->deadSession = 1;
}
```

**状态：** ✅ 已修复。连接不再因 SQL 错误而被错误标记为死亡。

---

### 修复 3：STMT_IS_RETURNING 属性不支持（dpiStmt.c）

**问题：** `conn.execute("SET search_path TO ...")` 失败，非 SELECT 语句无法执行。

**原因：** ODPI-C 在执行语句后查询 `DPI_OCI_ATTR_STMT_IS_RETURNING`（属性 218）来判断是否有 `RETURNING INTO` 子句。Shentong ACI 不支持此属性，返回错误。

**修复：** 默认设 `isReturning = 0`，传 `NULL` error handle 使失败静默忽略：

```c
// drivers/shentong/rust-shentong/odpi/src/dpiStmt.c 约第 914-921 行
stmt->isReturning = 0;
dpiOci__attrGet(stmt->handle, DPI_OCI_HTYPE_STMT,
        (void*) &stmt->isReturning, 0, DPI_OCI_ATTR_STMT_IS_RETURNING,
        NULL, error);  // NULL = 忽略失败
```

**状态：** ✅ 已修复。`INSERT/UPDATE/DELETE/CREATE/DROP/SET` 语句正常执行。

---

## 数据导出核心问题：表名解析

### 问题描述

通过 ACI 连接后：
- `all_tables`, `user_tables`, `dba_tables` 等 **Oracle 兼容视图可以正常查询**，能看到 `TEST_01` 表
- 通过 ACI **新建的表**（如 `CREATE TABLE _CMP_TEST_ ...`）可以从新连接正常查询
- 但 **已有的表** `TEST_01`（可能通过其他接口创建）无论用什么语法都无法 SELECT

### 已尝试的所有查询模式

| 语法 | 结果 | 错误信息 |
|------|------|----------|
| `SELECT * FROM TEST_01` | ❌ | `Relation "TEST_01" does not exist` |
| `SELECT * FROM SYSDBA.TEST_01` | ❌ | `Relation "SYSDBA"."TEST_01" does not exist` |
| `SELECT * FROM "SYSDBA"."TEST_01"` | ❌ | `Relation "SYSDBA"."TEST_01" does not exist` |
| `SELECT * FROM "SYSDBA"."test_01"` | ❌ | `Relation "SYSDBA"."test_01" does not exist` |
| `SELECT * FROM "sysdba"."test_01"` | ❌ | `SCHEMA不存在, Namespace "sysdba" does not exist` |
| `SELECT * FROM test_01` | ❌ | `Relation "TEST_01" does not exist` |
| `SELECT * FROM sysdba.test_01` | ❌ | `Relation "SYSDBA"."TEST_01" does not exist` |
| `SET search_path TO SYSDBA` → `SELECT * FROM TEST_01` | ❌ | search_path 设置成功但后续查询仍失败 |
| `OSRDB.SYSDBA.TEST_01`（三段式） | ❌ | `名字格式不对` |
| **`CREATE TABLE _X_ (...)` → `SELECT * FROM _X_`** | ✅ | **成功！** 同一连接或新连接均可 |

### 关键观察

1. **ACI 标识符处理规则**：Shentong ACI 将未加引号的标识符**自动转为大写并加双引号**传给底层引擎
   - `SELECT * FROM test_01` → 底层实际执行 `SELECT * FROM "TEST_01"`
   - `SELECT * FROM pg_class` → 底层实际执行 `SELECT * FROM "PG_CLASS"` → 失败（PostgreSQL 系统表是小写）

2. **PostgreSQL 系统表不可访问**：`pg_class`, `pg_namespace`, `pg_database`, `information_schema.tables` 都因大写转换而访问失败

3. **Oracle 兼容视图可访问**：`all_tables`, `user_tables`, `dba_tables`, `all_tab_columns`, `all_constraints`, `v$database`, `DUAL` 均正常 — 这些可能是 ACI 层内部特殊处理的

4. **数据库名**：`current_database()` 返回 `OSRDB`，`v$database` 也返回 `OSRDB`

5. **表空间相同**：`TEST_01` 和通过 ACI 新建的 `_CMP_CHECK_` 都在 `SYSTEM` 表空间、`SYSDBA` owner 下

6. **连接串不支持数据库名**：`host:port/OSRDB` 等格式都报 "无法解析指定的连接标识符"

### 假设分析

| 假设 | 可能性 | 说明 |
|------|--------|------|
| **TEST_01 通过非 ACI 接口（如 JDBC）创建** | 🔴 高 | Shentong 有 JDBC 端口（PostgreSQL wire protocol v2.0），通过 JDBC 创建的表可能在不同的内部命名空间 |
| **ACI 和 JDBC 使用不同的内部存储层** | 🟡 中 | Oracle 兼容视图是跨层的元数据汇总，但 SQL 执行只在当前层内解析 |
| **多数据库问题** | 🟡 中 | OSCAR 支持多数据库，TEST_01 可能在另一个数据库中 |
| **search_path 不持久** | 🟢 低 | SET search_path 执行成功（returned: 0），但可能不影响后续语句的解析路径 |

---

## 问题排查详细记录

### 阶段一：returnCode 错误（已解决）

```
错误：DPI-1037: column at array position 0 fetched with error 74
排查：添加 fprintf 调试，发现 returnCode 缓冲区未初始化
修复：clearMemory=1（calloc 代替 malloc）
验证：all_tab_columns 等查询正常返回
```

### 阶段二：连接假死（已解决）

```
错误：DPI-1080: connection was closed by ORA-840958026
排查：发现 SQL 错误后 serverStatus 返回垃圾值 32758
修复：只在 serverStatus==0 或 attrGet 失败时标记死亡
验证：SQL 错误后连接存活，可继续执行新语句
```

### 阶段三：非 SELECT 语句失败（已解决）

```
错误：STMT_IS_RETURNING 属性查询失败导致 execute() 返回错误
排查：Shentong ACI 不支持属性 218
修复：默认 isReturning=0，NULL error handle 忽略失败
验证：SET, CREATE, INSERT, DROP 等语句正常执行
```

### 阶段四：数据导出表不可访问（未解决）

```
错误：Relation "TEST_01" does not exist
排查：
  1. 对比测试 — 新建表可查询，已有表不可查询
  2. 所有大小写/引号/schema 组合都尝试过
  3. pg_class 不可访问（被自动大写）
  4. 连接串不支持数据库名后缀
  5. 两个表的 tablespace 和 owner 完全相同
状态：未解决 — 需要进一步调查 TEST_01 的创建方式
```

---

## 当前未解决问题

### P0：数据导出无法查询已有表

- **症状**：`SELECT * FROM TEST_01` 报 "Relation does not exist"
- **影响**：数据导出功能完全不可用（DDL 导出正常因为只用 Oracle 兼容视图获取元数据）
- **下一步排查方向**：
  1. 确认 `TEST_01` 的创建方式（ACI vs JDBC vs isql）
  2. 尝试通过 JDBC/PSQL 端口连接确认表是否可访问
  3. 通过 ACI 创建同名表后测试（先确认 DROP 是否能找到已有表）
  4. 检查 Shentong 是否有多个 SQL 执行引擎（Oracle 层 vs PostgreSQL 层）
  5. 联系 Shentong 技术支持确认 ACI 接口的表名解析机制

### P1：索引/外键/触发器导出未实现

- `get_table_details` 和 `inspect_table_details` 中 indexes、unique_constraints、foreign_keys、check_constraints、triggers 全部返回空数组
- 需要查询 `all_indexes`, `all_ind_columns`, `all_constraints` (R/U/C types), `all_triggers` 等

### P2：ODPI-C 调试日志未清理

- `dpiError.c`, `dpiOci.c`, `dpiVar.c`, `dpiStmt.c` 中都有 `fprintf(stderr, "SHENTONG_DEBUG: ...")` 调试输出
- 生产发布前需要移除或用条件编译包裹

---

## 待清理项

1. **移除 ODPI-C 调试日志** — 所有 `SHENTONG_DEBUG` 和 `ODPI-DBG` 的 fprintf 调用
2. **移除测试二进制** — `backend/src/bin/shentong_query_test.rs` 包含硬编码凭据
3. **消除代码重复** — `db/shentong.rs` 和 `export/shentong_poc.rs` 中的 `fetch_columns`/`fetch_primary_keys` 逻辑完全相同
4. **移除未使用函数** — `shentong_poc.rs` 中的 `quote_ident()` 未使用
5. **移除未使用参数** — `export_table_rows` 中的 `config` 参数未使用

---

## 相关文件清单

### 后端 Rust 代码

| 文件 | 用途 |
|------|------|
| `backend/src/db/shentong.rs` | 连接管理 + 元数据查询（open, test, schemas, tables, details） |
| `backend/src/export/shentong_poc.rs` | DDL + 数据导出 POC |
| `backend/src/bin/shentong_query_test.rs` | 诊断测试二进制（非生产代码） |

### ODPI-C 修改（C 代码）

| 文件 | 修改内容 |
|------|----------|
| `drivers/shentong/rust-shentong/odpi/src/dpiVar.c` | returnCode 零初始化 + 调试日志 |
| `drivers/shentong/rust-shentong/odpi/src/dpiError.c` | serverStatus 垃圾值处理 + 调试日志 |
| `drivers/shentong/rust-shentong/odpi/src/dpiStmt.c` | STMT_IS_RETURNING 静默失败 + 调试日志 |
| `drivers/shentong/rust-shentong/odpi/src/dpiOci.c` | 调试日志（stmtExecute, stmtPrepare2, defineByPos, clientVersion） |

### 驱动库

| 文件 | 说明 |
|------|------|
| `drivers/shentong/aci.dll` | Shentong ACI 客户端动态库 |
| `drivers/shentong/rust-shentong/` | Rust 封装层（shentong crate），基于 rust-oracle 修改 |
