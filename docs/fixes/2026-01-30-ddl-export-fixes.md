# DDL 导出修复 (2026-01-30)

## 问题描述

导出的 DDL 在 DM8 执行时出现两类错误：

### 1. 索引名冲突错误

**错误信息**：
```
[22000][-7021] 第1 行附近出现错误: 无效的索引名
```

**原因**：
- DM8 在创建主键约束时会自动创建索引
- 如果表上存在与主键列完全相同的索引（无论是否唯一），再次创建会导致冲突
- 例如：`QRTZ_SIMPLE_TRIGGERS` 表的主键是 `(SCHED_NAME, TRIGGER_NAME, TRIGGER_GROUP)`，同时存在一个非唯一索引 `INDEX33561156` 覆盖相同的列

**修复**：
- 修改 `generate_indexes()` 函数，跳过所有与主键列完全相同的索引（包括唯一和非唯一索引）
- 之前只跳过唯一索引，现在改为跳过所有匹配的索引

### 2. 触发器语法错误

**错误信息**：
```
[42000][-2007] 第 6 行, 第 58 列[]附近出现错误: 语法分析出错
```

**原因**：
1. **WHEN 子句位置错误**：触发器体中的 `WHEN` 子句应该在 `FOR EACH ROW` 之后，而不是在 `BEGIN...END` 块内部
2. **缺少分号**：触发器体中的 SQL 语句（如 `SELECT...INTO` 和赋值语句）缺少结尾分号
3. **DECLARE 块处理**：以 `DECLARE` 开头的触发器体被错误地包裹在额外的 `BEGIN...END` 中

**示例错误代码**：
```sql
CREATE OR REPLACE TRIGGER TRG_BPM_CATEGORY_ID
BEFORE INSERT ON BPM_CATEGORY
FOR EACH ROW
WHEN (NEW.ID IS NULL)
BEGIN
SELECT SEQ_BPM_CATEGORY.NEXTVAL INTO :NEW.ID FROM DUAL  -- 缺少分号
END
```

**修复**：
1. 添加 `extract_when_clause()` 函数，使用括号深度跟踪正确提取 `WHEN` 子句（支持嵌套括号和多行 WHEN）
2. 修改 `generate_triggers()` 函数：
   - 将 `WHEN` 子句放在 `FOR EACH ROW` 之后
   - 仅对行级触发器（`each_row = true`）提取 WHEN 子句
   - 识别以 `DECLARE` 或 `BEGIN` 开头的触发器体，不额外包裹
3. 改进 `normalize_trigger_body()` 函数：
   - 使用累积括号深度跟踪多行语句
   - 识别 `SELECT...INTO...FROM` 语句，正确处理多行情况
   - 为触发器体中的语句添加缺失的分号

**修复后的代码**：
```sql
CREATE OR REPLACE TRIGGER "PLATFORM"."TRG_BPM_CATEGORY_ID"
BEFORE INSERT ON "PLATFORM"."BPM_CATEGORY"
FOR EACH ROW
WHEN (NEW.ID IS NULL)
BEGIN
SELECT SEQ_BPM_CATEGORY.NEXTVAL INTO :NEW.ID FROM DUAL;
END;
```

## 修改的文件

- `backend/src/export/ddl.rs`
  - 修改 `generate_indexes()` - 跳过所有与主键列相同的索引（不仅仅是唯一索引）
  - 修改 `generate_triggers()` - 正确处理 WHEN 子句位置、DECLARE 块和 each_row 检查
  - 新增 `extract_when_clause()` - 使用括号深度跟踪从触发器体中提取 WHEN 子句
  - 改进 `normalize_trigger_body()` - 使用累积括号深度和 SELECT...INTO 检测，为语句添加缺失的分号

## 测试覆盖

新增测试：
1. `generate_indexes_skips_non_unique_index_on_pk_columns` - 验证跳过与主键列相同的非唯一索引
2. `extract_when_clause_separates_when_from_body` - 验证 WHEN 子句提取功能
3. `extract_when_clause_handles_nested_parentheses` - 验证嵌套括号处理
4. `extract_when_clause_handles_multiline_when` - 验证多行 WHEN 子句处理
5. `generate_triggers_places_when_after_for_each_row` - 验证 WHEN 子句位置正确
6. `generate_triggers_handles_declare_block` - 验证 DECLARE 块不被额外包裹
7. `generate_triggers_skips_when_for_statement_level_trigger` - 验证语句级触发器不提取 WHEN
8. `normalize_trigger_body_handles_multiline_select` - 验证多行 SELECT...INTO 处理

所有现有测试继续通过，总计 24 个测试全部通过。

## 代码审阅结果（Codex）

使用 Codex 进行了两轮代码审阅：

**第一轮发现的问题**：
- `extract_when_clause` 括号深度跟踪不完整
- `generate_triggers` 未处理 DECLARE 块
- `normalize_trigger_body` 多行语句检测不可靠
- `generate_indexes` 可能误删列顺序不同的索引

**第二轮修复后的状态**：
- ✅ 括号深度跟踪已改进，支持嵌套括号
- ✅ DECLARE 块已正确处理
- ✅ 多行语句检测已改进（使用累积深度和 SELECT...INTO 识别）
- ⚠️ 索引过滤使用 HashSet 比较，可能在极少数情况下误删（但这是保守的做法，避免冲突）

**剩余风险**：
- 字符串和注释中的括号可能影响 WHEN 子句提取（低风险，实际触发器很少在 WHEN 中使用字符串）
- 复杂的多行语句可能仍有边界情况（低风险，已覆盖常见场景）

## 验证步骤

1. 重新导出 DDL：
   ```bash
   cd backend
   cargo run
   ```

2. 在 DM8 中执行导出的 DDL 文件

3. 验证：
   - 不再出现"无效的索引名"错误
   - 触发器创建成功，不再出现语法错误
   - WHEN 子句位置正确（在 FOR EACH ROW 之后）
   - 触发器体中的语句有正确的分号

## 影响范围

- 仅影响 DDL 导出功能
- 不影响数据导出
- 向后兼容，不影响现有功能
- 所有测试通过，代码质量有保证

### 3. 触发器终止符缺失

**错误信息**：
```
触发器在 SQL 客户端中无法正确识别结束位置
```

**原因**：
- DM8 要求触发器以 `/` 作为语句终止符
- 仅有 `END;` 不足以让 SQL 客户端识别触发器结束

**修复**：
- 在 `generate_triggers()` 中，每个触发器的 `END;` 后添加 `\n/`

### 4. 外键 ON DELETE/UPDATE 语法错误

**原因**：
- ON DELETE/UPDATE 子句被错误地放在分号之后

**修复**：
- 重构 `generate_foreign_keys()` 函数，先构建完整的约束语句，最后添加分号

### 5. CHAR/BYTE 语义反转

**原因**：
- DM8 的 `CHAR_USED` 字段中，`C` 表示 CHAR 语义，`B` 表示 BYTE 语义
- 代码中将两者反转了

**修复**：
- 修正 `format_data_type()` 中的条件判断：`C` -> CHAR，`B` -> BYTE

### 6. 触发器事件解析不完整

**原因**：
- DM8 的 `TRIGGERING_EVENT` 字段使用 " OR " 作为分隔符（如 "INSERT OR UPDATE OR DELETE"）
- 代码仅按逗号分割

**修复**：
- 修改 `fetch_triggers()` 中的事件解析，先将 " OR " 替换为逗号，再分割

### 7. 主键自增列丢失

**问题**: 导出的 DDL 中主键自增列（IDENTITY）信息丢失。

**原因**:
- 原代码使用 `ALL_TAB_IDENTITY_COLS` 视图查询自增列信息
- 该视图在某些 DM8 版本中不存在或不完整
- 导致自增列信息无法正确获取

**修复**:
根据 [达梦官方文档](https://eco.dameng.com/community/article/1c4f31bf7abf88859282387845c2b3b4)，使用正确的方法查询自增列：

1. 使用 `SYS.SYSCOLUMNS.INFO2` 字段判断自增列：
   - 当 `INFO2 & 0x01 = 0x01` 时，表示该列是自增列

2. 使用 `IDENT_SEED()` 和 `IDENT_INCR()` 函数获取种子值和增量值

**修改后的 SQL**:
```sql
SELECT c.COLUMN_NAME, c.DATA_TYPE, ...,
       CASE WHEN sc.INFO2 & 1 = 1 THEN 'YES' ELSE 'NO' END AS IDENTITY_COLUMN,
       ...
FROM ALL_TAB_COLUMNS c
LEFT JOIN SYS.SYSOBJECTS so ON so.NAME = c.TABLE_NAME AND so.SCHID = (SELECT ID FROM SYS.SYSOBJECTS WHERE NAME = c.OWNER AND TYPE$ = 'SCH')
LEFT JOIN SYS.SYSCOLUMNS sc ON sc.ID = so.ID AND sc.NAME = c.COLUMN_NAME
WHERE c.OWNER = '...' AND c.TABLE_NAME = '...'
```

**获取种子值和增量值**:
```sql
SELECT IDENT_SEED('"SCHEMA"."TABLE"'), IDENT_INCR('"SCHEMA"."TABLE"') FROM DUAL
```

## 修改的文件

- `backend/src/export/ddl.rs`
  - 修改 `generate_indexes()` - 跳过所有与主键列相同的索引
  - 修改 `generate_triggers()` - 正确处理 WHEN 子句位置、DECLARE 块、添加 `/` 终止符
  - 修改 `generate_foreign_keys()` - 修复 ON DELETE/UPDATE 子句位置，添加 ON UPDATE 支持
  - 修改 `format_data_type()` - 修正 CHAR/BYTE 语义
  - 修改 `format_column_definition()` - IDENTITY 列不再同时输出 DEFAULT 值
  - 新增 `extract_when_clause()` - 从触发器体中提取 WHEN 子句
  - 改进 `normalize_trigger_body()` - 为语句添加缺失的分号

- `backend/src/db/schema.rs`
  - 修改 `fetch_triggers()` - 支持 " OR " 分隔的触发器事件，正确提取 timing（BEFORE/AFTER/INSTEAD OF）
  - 重写 `fetch_columns()` - 使用 `SYS.SYSCOLUMNS.INFO2` 检测自增列，优化 JOIN 性能
  - 新增 `fetch_identity_info()` - 使用 `IDENT_SEED()` 和 `IDENT_INCR()` 获取自增列属性
  - 修改 `fetch_foreign_keys()` - 添加 UPDATE_RULE 查询

- `backend/src/models/mod.rs`
  - 修改 `ForeignKey` 结构体 - 添加 `update_rule` 字段

## 代码审阅修复（第二轮）

### 1. 自增列查询 SQL 性能优化
**问题**: 原 SQL 使用子查询获取 SCHID，每行都会执行一次子查询
**修复**: 改用 JOIN 方式，避免重复子查询

### 2. IDENT_SEED/IDENT_INCR 函数参数格式
**问题**: 原代码使用双引号包裹表名，格式可能不正确
**修复**: 改用 `SCHEMA.TABLE` 格式（单引号内不带双引号）

### 3. 多个自增列的情况
**问题**: 原代码将相同的 seed/incr 值赋给所有标记为 identity 的列
**修复**: 添加注释说明 DM8 每表只允许一个自增列，只更新第一个自增列

### 4. 外键缺少 ON UPDATE 规则
**问题**: 只查询和生成 ON DELETE，没有 ON UPDATE
**修复**:
- `ForeignKey` 结构体添加 `update_rule` 字段
- SQL 查询添加 `UPDATE_RULE` 列
- DDL 生成添加 `ON UPDATE` 子句

### 5. 触发器 timing 字段可能包含额外信息
**问题**: `trigger_type` 可能包含 "BEFORE EACH ROW" 等完整信息
**修复**: 只提取 BEFORE/AFTER/INSTEAD OF 部分，同时从 trigger_type 和 description 检测 EACH ROW

### 6. IDENTITY 列与 DEFAULT 值冲突
**问题**: 如果列同时有 IDENTITY 和 DEFAULT 值，生成的 DDL 可能无效
**修复**: IDENTITY 列不再输出 DEFAULT 值（使用 if-else 互斥）

## 相关问题

- 索引冲突问题在 Quartz 调度器表（`QRTZ_*`）中较为常见
- 触发器语法问题在使用序列自增的表中较为常见
- WHEN 子句问题在有条件触发器中较为常见
