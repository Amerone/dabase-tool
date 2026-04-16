# 从 DM8 切到 MySQL、KingbaseES、神通，我最后发现最难迁移的不是表，而是迁语义

很多人第一次做数据库迁移，都会下意识把它理解成一件“体力活”：把源库的表结构导出来，在目标库执行；把数据导出来，再写进去。看起来像是一个标准工程流程，甚至有点像“数据库版搬家”。

但真正做过一次 DM8 切库之后，感受通常会完全变掉。

因为你很快就会发现，最难的部分往往不是表能不能建出来，也不是数据能不能导进去，而是原来在 DM8 里默认成立的那套语义，在目标库里并不一定成立。字段名没变，类型名看起来也差不多，脚本甚至能顺利执行，但业务层的含义已经悄悄变了。

这也是为什么同样是“从 DM8 切库”，切到 MySQL、KingbaseES、神通，体感会完全不同。MySQL 更像是换了一个世界，很多地方都得重新妥协；KingbaseES 与 DM8 相对接近，迁移体验会更平滑；神通虽然同样偏 Oracle 语系，但细节兼容要求并不低。

说到底，数据库迁移从来不是一次简单的类型替换，而是一场语义迁移。

## 同样是切库，为什么难度会差这么多

如果把 DM8 到不同目标库的迁移路线放在一起看，差异其实非常清楚。

### DM8 -> MySQL：不是兼容迁移，更像结构重设计

DM8 到 MySQL 往往是最痛的一条路线。问题不只是语法不同，而是很多基础假设都不一样。

比如，DM8 里的 `DATE` 是带时分秒的，但 MySQL 的 `DATE` 只有日期；DM8 里的 `NUMBER` 很灵活，但到了 MySQL 往往会被压缩成整数或定点数；再加上 InnoDB 的单行大小限制和索引长度限制，大量在 DM8 里定义自然、运行正常的结构，迁到 MySQL 之后很可能不得不“降配”。

很多项目做到最后会发现，DM8 到 MySQL 根本不是把原结构完整搬过去，而是先把结构重新梳理一遍，再决定哪些要保、哪些要改、哪些要放弃。换句话说，这更像一次结构重设计，而不是一次轻量迁移。

### DM8 -> KingbaseES：最像迁移的一条路线

如果三个目标里要选一个“最顺手”的，通常会是 KingbaseES。它和 DM8 的距离明显比 MySQL 小，很多对象和类型在迁移时不需要做过于激进的改造。尤其是数值类型、序列、触发器这类对象，保留的可能性会高很多。

但这并不意味着可以完全无脑迁。最典型的问题依然存在，比如 `DATE` 语义、默认值函数、触发器兼容模式等。这条路线的特点不是“零改造”，而是“改造可控”。如果 MySQL 路线更像重构，那么 KingbaseES 更像一次比较严肃但仍可控的工程迁移。

### DM8 -> 神通：同语系，不等于零摩擦

神通很容易给人一种直觉：它和 Oracle 很像，而 DM8 也偏 Oracle 风格，所以迁移应该会比较顺。

这个判断只对了一半。

它的确比 MySQL 更接近 DM8，在序列、触发器、对象模型上也更容易找到对应关系。但另一半也必须承认：越是这种“看起来相近”的目标库，越容易在细节上出坑。比如字符长度和字节长度的差异、自增列的配套约束、大字段写入策略、触发器里的细节写法，这些问题在项目初期不一定显现出来，但一旦碰到真实数据和真实业务场景，就会变得非常具体。

所以 DM8 到神通更像“同语系迁移”，而不是“零摩擦迁移”。

## 如果只想先拿规则，这 4 张表基本够用

很多文章写到最后，读者真正想收藏的其实不是观点，而是一套能直接参考的规则。下面这几张表已经能覆盖大多数 DM8 切库时的核心决策。

### 1. 数值类型映射速查表

| DM8 类型 | MySQL 常见落点 | KingbaseES 常见落点 | 神通常见落点 | 备注 |
|---|---|---|---|---|
| `TINYINT` | `TINYINT` | `SMALLINT` | `TINYINT` | Kingbase 常上提一档 |
| `SMALLINT` | `SMALLINT` | `SMALLINT` | `NUMBER(5)` | 神通更偏 Oracle 数值表达 |
| `INTEGER` / `INT` | `INT` | `INT` / `INTEGER` | `NUMBER(10)` | 神通通常不保留 `INT` 名称 |
| `BIGINT` | `BIGINT` | `BIGINT` | `NUMBER(19)` | 神通按精度重写 |
| `BIT` | `TINYINT(1)` | `SMALLINT` | `BOOLEAN` | 三库语义最不统一 |
| `BOOLEAN` / `BOOL` | `TINYINT(1)` | `BOOLEAN` | `BOOLEAN` | MySQL 本质仍是数值表达 |
| `NUMBER(p,0)` 且 `p<=2` | `TINYINT` | `NUMERIC(p,0)` | `NUMBER(p,0)` | MySQL 会压缩类型 |
| `NUMBER(p,0)` 且 `p=3..4` | `SMALLINT` | `NUMERIC(p,0)` | `NUMBER(p,0)` | 同上 |
| `NUMBER(p,0)` 且 `p=5..6` | `MEDIUMINT` | `NUMERIC(p,0)` | `NUMBER(p,0)` | 同上 |
| `NUMBER(p,0)` 且 `p=7..9` | `INT` | `NUMERIC(p,0)` | `NUMBER(p,0)` | 同上 |
| `NUMBER(p,0)` 且 `p=10..18` | `BIGINT` | `NUMERIC(p,0)` | `NUMBER(p,0)` | 同上 |
| `NUMBER(p,0)` 且 `p>18` | `DECIMAL(p,0)` | `NUMERIC(p,0)` | `NUMBER(p,0)` | MySQL 回退 decimal |
| `NUMBER(p,s)` 且 `s>0` | `DECIMAL(p,s)` | `NUMERIC(p,s)` | `NUMBER(p,s)` | 三边差异较小 |
| `NUMBER` 无精度 | 常补成 `DECIMAL(38,10)` | `NUMERIC` | `NUMBER` | 这类字段最该人工确认 |
| `DECIMAL(p,s)` / `NUMERIC(p,s)` | `DECIMAL(p,s)` | `NUMERIC(p,s)` | `DECIMAL(p,s)` / `NUMBER(p,s)` | 一般可平移 |
| `DOUBLE` / `FLOAT` | `DOUBLE` | `DOUBLE PRECISION` | `DOUBLE PRECISION` | 名称不同，语义接近 |
| `REAL` | `FLOAT` | `REAL` | `REAL` | MySQL 类型名不同 |

### 2. 字符串、LOB、二进制映射速查表

| DM8 类型 | MySQL 常见落点 | KingbaseES 常见落点 | 神通常见落点 | 备注 |
|---|---|---|---|---|
| `VARCHAR(n)` 且 `n<=16383` | `VARCHAR(n)` | `VARCHAR(n)` | `VARCHAR(n)` 或按语义换算 | 神通要关注字符/字节语义 |
| `VARCHAR(n)` 且 `n>16383` | `LONGTEXT` | `TEXT` | `CLOB` | 长字段三边差异大 |
| `VARCHAR` 无长度 | `LONGTEXT` / `TEXT` | `TEXT` | `CLOB` | 无长度字段最好人工确认 |
| `NVARCHAR(n)` | `VARCHAR(n)` | `VARCHAR(n)` | 常按字符语义放大后转 `VARCHAR` / `CLOB` | 神通常需要长度换算 |
| `CHAR(n)` 且 `n<=255` | `CHAR(n)` | `CHAR(n)` | `CHAR(n)` 或按语义换算 | 神通仍要看字节/字符语义 |
| `CHAR(n)` 且 `n>255` | `VARCHAR(n)` | `CHAR(n)` | `CHAR(n)` 或转 `CLOB` | MySQL 与其他两库处理不同 |
| `NCHAR(n)` | `CHAR(n)` / `VARCHAR(n)` | `CHAR(n)` | 常按字符语义放大后转 `CHAR` / `CLOB` | 神通侧要防止超限 |
| `CLOB` / `NCLOB` | `LONGTEXT` | `TEXT` | `CLOB` | 文本大对象不宜只看类型名 |
| `TEXT` / `LONG` | `LONGTEXT` | `TEXT` | `CLOB` | MySQL 和神通差异明显 |
| `BLOB` | `LONGBLOB` | `BYTEA` | `BLOB` | 二进制字面量写法完全不同 |
| `LONGVARBINARY` | `LONGBLOB` | `BYTEA` | `BLOB` | 同上 |
| `RAW(n)` | `VARBINARY(n)` | `BYTEA` | `VARBINARY(n)` | Kingbase 更喜欢 `BYTEA` |
| `BINARY(n)` | `VARBINARY(n)` | `BYTEA` | `VARBINARY(n)` | MySQL/神通更保留长度概念 |
| `VARBINARY(n)` | `VARBINARY(n)` | `BYTEA` | `VARBINARY(n)` | 同上 |

补一句神通侧的特殊点：

1. 如果源字段是字符语义，目标侧往往不能只照抄长度。
2. 多字节字符场景下，长度很容易在目标库被放大。
3. 一旦超过目标库限制，普通字符串列会直接退化成 `CLOB`。

### 3. 日期时间与默认值速查表

#### 3.1 日期时间类型

| DM8 类型 | MySQL 常见落点 | KingbaseES 常见落点 | 神通常见落点 | 风险提示 |
|---|---|---|---|---|
| `DATE` | `DATETIME` | `TIMESTAMP(0)` | `DATE` | 最容易丢时间语义 |
| `TIMESTAMP` | `DATETIME` | `TIMESTAMP` | `TIMESTAMP` | 三边较稳定 |
| `TIMESTAMP(n)` | `DATETIME(n)` | `TIMESTAMP(n)` | `TIMESTAMP(n)` | MySQL/神通通常只保留到 6 位 |
| `DATETIME(n)` | `DATETIME(n)` | `TIMESTAMP(n)` | `TIMESTAMP(n)` | 目标类型名可能不同 |
| `TIME` | `TIME` 或保持原样 | 多数可直通 | 多数可直通 | 要结合应用层使用方式确认 |

#### 3.2 默认值函数

| DM8 默认值 / 表达式 | MySQL 常见处理 | KingbaseES 常见处理 | 神通常见处理 | 风险提示 |
|---|---|---|---|---|
| `SYSDATE` / `SYSDATE()` | `CURRENT_TIMESTAMP` | `CURRENT_TIMESTAMP` | 直通或兼容改写 | 目标函数名不一定一致 |
| `SYSTIMESTAMP` | `CURRENT_TIMESTAMP` | `CURRENT_TIMESTAMP` | `CURRENT_TIMESTAMP` | 典型兼容改写点 |
| `CURRENT_TIMESTAMP()` | 常改成 `CURRENT_TIMESTAMP` | 多数可接受 | 常改成 `CURRENT_TIMESTAMP` | 有些库不接受空括号 |
| `NOW()` / `GETDATE()` | 常改成 `CURRENT_TIMESTAMP` | 需确认是否支持 | 需确认是否支持 | 不建议无脑照搬 |
| `CURRENT_DATE()` | 需结合列类型判断 | 多数可接受 | 常改成 `CURRENT_DATE` | 对 date/time 列约束不同 |
| `CURRENT_TIME()` | 常改成 `CURRENT_TIME` | 多数可接受 | 常改成 `CURRENT_TIME` | 同上 |
| 日期时间列上的 DM8 专有函数 | 很多要降级或重写 | 部分可保留 | 部分可保留 | 最容易导致 DDL 执行失败 |
| 文本 / BLOB / JSON 类字段默认值 | 通常不建议保留 | 看目标库能力 | 看目标库能力 | MySQL 尤其敏感 |

### 4. 对象模型与大字段处理速查表

#### 4.1 自增、序列、触发器

| 维度 | MySQL | KingbaseES | 神通 | 风险提示 |
|---|---|---|---|---|
| 自增模型 | `AUTO_INCREMENT` | `GENERATED BY DEFAULT AS IDENTITY` | `AUTO_INCREMENT` | 不是同一套对象模型 |
| 是否适合保留序列主路径 | 否 | 是 | 是 | MySQL 通常要改思路 |
| 原有 `SEQ.NEXTVAL` 习惯 | 需要重构 | 多数可保留或兼容改写 | 多数可保留或兼容改写 | 业务主键链路要重点验证 |
| Oracle 风格触发器 | 不适合直接照搬 | 兼容性较好 | 兼容性较好但细节要改 | 不能因为“支持触发器”就默认兼容 |
| `OLD/NEW` / `WHEN` 等触发器细节 | 需重写概率高 | 中等 | 中等到高 | 要做逐条校对 |

#### 4.2 BLOB/CLOB 与二进制字面量

| 目标库 | BLOB 类型常见落点 | CLOB 类型常见落点 | 二进制字面量常见写法 | 风险提示 |
|---|---|---|---|---|
| MySQL | `LONGBLOB` | `LONGTEXT` | `X'HEX'` | 大字段多时容易再触发行/索引限制联动问题 |
| KingbaseES | `BYTEA` | `TEXT` | `'\\xhex'::BYTEA` | 读写通常较自然，但仍要测大样本 |
| 神通 | `BLOB` | `CLOB` | `TO_BLOB(HEXTORAW('HEX'))` | 大 BLOB 往往不能简单内联，需要分块处理 |

## 只看表还不够，真正落地时通常按这几条规则处理

如果前面的表更像“速查”，那这一节更像“落地原则”。真正做迁移时，很多决定并不是看见一个类型就机械替换，而是要先判断它在业务里到底扮演什么角色。

### 1. `NUMBER` 先分三类，再决定怎么落

第一类是明显的整数型字段，比如主键、状态码、计数器，这类字段如果精度边界明确，可以在 MySQL 里压到 `TINYINT`、`INT`、`BIGINT`，在 KingbaseES 和神通里则更适合保留成 `NUMERIC(p,0)` 或 `NUMBER(p,0)`。

第二类是金额、比例、单价、税额这类字段。这类字段不要因为它也是 `NUMBER` 就去压整数，通常应该直接保留成 `DECIMAL(p,s)`、`NUMERIC(p,s)` 或 `NUMBER(p,s)`。

第三类是“历史上偷懒定义”的泛数值字段，比如直接写成 `NUMBER`、或者精度定义非常宽泛。这类字段最不适合自动映射，最稳妥的方式是人工确认业务含义，再决定落成整数、定点数还是继续保留宽泛表达。

### 2. `DATE` 不要看名字，要看它在代码里是怎么被用的

如果应用层把 DM8 的 `DATE` 当成时间戳在用，比如映射成 `LocalDateTime`、用于排序、审计、增量同步，那它迁到 MySQL 时几乎都应该落到 `DATETIME`，迁到 KingbaseES 时更适合 `TIMESTAMP`，而不是继续落 `DATE`。

只有在你能明确证明这个字段本来就只有年月日语义时，才应该把它迁成真正的 date-only 类型。否则，最容易发生的问题不是 DDL 执行失败，而是“时间静悄悄丢了”。

### 3. 字符串字段要同时看长度、字符集、索引参与度

`VARCHAR2(2000 CHAR)` 这类字段，在 DM8 里看起来只是一个普通长字符串，但一旦迁到 MySQL，就要同时评估三件事：

1. 按目标字符集折算后的实际字节成本。
2. 它是否参与主键、唯一键、普通索引。
3. 它是否可能与其他大字段叠加后触发行大小限制。

如果这三个条件里有两个同时偏高，就不能只做“类型替换”，而应该进一步决定是缩短长度、改索引设计，还是退化为 `TEXT` / `CLOB`。

### 4. 默认值表达式不要原样搬，要按目标库的可执行语义重写

迁移时真正应该保留的不是“源 SQL 文本”，而是“默认值行为”。比如：

1. `SYSDATE`、`SYSTIMESTAMP` 更重要的是“插入时取当前时间”，不是一定要保留原函数名。
2. `CURRENT_TIMESTAMP()` 这种带括号写法，在一些目标库里要改成不带括号。
3. 文本、大字段、JSON 一类列的默认值能力在不同数据库里差异很大，MySQL 尤其要单独核查。

所以默认值迁移的正确姿势不是“函数名替换”，而是“行为对齐”。

### 5. 序列、触发器、自增列要作为一套对象模型整体处理

很多文章会把这些对象拆开讲，但真到项目里，它们通常是一套联动机制。

如果源库是“序列 + before insert 触发器 + `:NEW.ID` 赋值”这一套，迁到 MySQL 时大概率要整体切成 `AUTO_INCREMENT` 或应用层发号；迁到 KingbaseES、神通时，则可以选择继续保留 sequence 路线，但也要同步校对触发器语法、默认值函数、对象依赖关系。

判断标准不是“目标库支不支持触发器”，而是“这条主键生成链路能不能在目标库里稳定重建”。

## 真正值得展开讲的 5 个坑

如果要写一篇真正能让读者产生共鸣的数据库迁移文章，我认为下面这 5 个坑最值得展开。它们都不是冷门语法点，而是非常容易在真实项目里出现的问题。

### 1. 同叫 `DATE`，语义却完全不同

这是最适合当文章开头的坑，因为几乎任何技术角色都能立刻理解它的严重性。

| 数据库 | `DATE` 语义 |
|---|---|
| DM8 | 含时分秒 |
| MySQL | 只有日期 |
| KingbaseES | 只有日期 |
| 神通 | 更接近 Oracle 语义，通常保留时间信息 |

这个问题最麻烦的地方在于，它不是那种“执行时报错”的问题，而是那种“执行通过，但结果悄悄变了”的问题。

你把 DM8 的 `DATE` 直接迁成 MySQL 的 `DATE`，脚本可能照样执行，数据也能导进去，但时间部分已经没了。到了业务层，Java 原来按 `LocalDateTime` 读，现在只能读出日期；接口字段明明没改，返回值却少了时间；报表、审计、排序逻辑都可能被连带影响。

所以在这类文章里，可以非常明确地给出一个结论：DM8 的 `DATE` 迁移到 MySQL 或 Kingbase 时，本质上更像在迁移 `DATETIME` 或 `TIMESTAMP`，而不是 `DATE`。

### 2. `NUMBER` 到底该不该压成整数

如果说 `DATE` 的坑在于“同名不同义”，那 `NUMBER` 的坑就在于“语义过于灵活”。DM8 的 `NUMBER` 可以承载很多不同含义：有时候它本质上是整数，有时候它是金额，有时候它只是一个泛数值容器。但到了目标库，这种灵活性通常会被迫收缩。

在 MySQL 路线里，这个问题尤其明显。很多 `NUMBER(p,0)` 最后会被压成 `INT` 或 `BIGINT`，这在技术上看似合理，但从业务语义上未必正确。因为一旦你把它压成整数，就等于默认它以后永远只会是整数型字段；如果后续业务演进需要小数、需要更大范围、需要保留更宽泛的数值定义，成本就会回到应用层或者二次迁移层面。

所以 `NUMBER` 不是一个简单的映射问题，而是一个选择问题：你到底要保留语义弹性，还是换取目标库上的局部“自然化”。

### 3. MySQL 真正麻烦的不是语法，而是限制

很多人会低估 MySQL 的迁移难度，是因为他们把关注点放在了“语法像不像”上。实际上，DM8 到 MySQL 最大的问题，往往不是写法，而是限制。

最典型的两个限制就是：

1. InnoDB 单行大小限制。
2. InnoDB 索引长度限制。

这两个限制为什么可怕？因为它们非常容易在真实结构里叠加出现。单独一个大字段也许没问题，但多个大 `VARCHAR` 列放在一起就可能超行大小；单独一个索引列也许没问题，但复合主键、复合外键一叠加，utf8mb4 下索引长度马上就会膨胀。很多原本在 DM8 里定义自然、语义完整的结构，迁到 MySQL 时必须被迫缩短长度、改变键设计，甚至把字符串列降级成 `TEXT`。而一旦降成 `TEXT`，新的问题又会立刻出现：它还能不能参与索引？还能不能参与键？

所以 DM8 到 MySQL 很少是“字段逐个映射”的过程，而更像一个不断做妥协和折中的过程。

### 4. 默认值、序列、触发器，才是真正的业务迁移难点

很多迁移项目会卡在一个看似悖论的阶段：表都建好了，数据也导进去了，但系统还是不能跑。真正的问题，往往就藏在默认值、序列和触发器里。

这是因为这三类对象决定的不是“表长什么样”，而是“业务怎么活起来”。

一个最典型的例子是主键生成：在 DM8 里，也许是 `SEQ_NAME.NEXTVAL + 触发器` 这套组合在工作；迁到 MySQL 之后，这套组合天然就不存在了，你必须改成 `AUTO_INCREMENT`，或者改成应用生成主键。再比如默认值函数，`SYSDATE`、`SYSTIMESTAMP` 在源库里用得很自然，到了目标库后却可能根本不接受原样写法。又比如触发器，`OLD/NEW`、`WHEN` 子句、体内函数、终止符规则，都可能和目标库并不相同。

所以如果一篇迁移文章只讲字段映射，不讲默认值、序列、触发器，那篇文章大概率还没讲到真正的痛点。

### 5. BLOB / CLOB 往往不是第一天暴露的问题

LOB 是特别典型的一类“前期看起来没事，后期才炸”的问题。

为什么它常常后期才暴雷？因为小样本测试看不出来。测试库里字段值不大、数据量也不大，读写链路看起来都没问题；一到真实环境，问题就会集中出现。有人会在大字段读取时遇到驱动缓冲区截断，有人会在二进制字面量拼装时踩语法坑，有人会在大对象写入策略上发现目标库根本不能按自己预想的方式处理。尤其是那些更接近 Oracle 风格的目标库，在 BLOB 处理细节上常常不能靠想当然来判断。

所以如果项目里有文档、图片、附件、富文本、大 JSON 这类内容，LOB 问题一定要被单独拎出来做验证，而不能当成“普通类型映射”的附属部分。

## 一个真实迁移场景，几乎能把所有坑串起来

如果只讲规则，文章还是容易显得像手册。真正能让读者代入的，往往是一个足够具体的例子。

假设你在 DM8 里有这样一张业务表：

```sql
CREATE TABLE ORDER_LOG (
    ID            NUMBER(19,0)      NOT NULL,
    BIZ_TIME      DATE              NOT NULL,
    AMOUNT        NUMBER(18,2)      NOT NULL,
    STATUS        NUMBER(1,0)       DEFAULT 0,
    TITLE         VARCHAR2(2000 CHAR),
    ATTACHMENT    BLOB,
    CREATED_AT    DATE              DEFAULT SYSDATE,
    PRIMARY KEY (ID)
);

CREATE SEQUENCE SEQ_ORDER_LOG START WITH 1 INCREMENT BY 1;

CREATE OR REPLACE TRIGGER TRG_ORDER_LOG_BI
BEFORE INSERT ON ORDER_LOG
FOR EACH ROW
BEGIN
    IF :NEW.ID IS NULL THEN
        SELECT SEQ_ORDER_LOG.NEXTVAL INTO :NEW.ID FROM DUAL;
    END IF;
END;
```

这张表看起来很普通，但它几乎把 DM8 切库里最常见的坑都带上了：

1. `ID` 是 `NUMBER(19,0)`，而且依赖“序列 + 触发器”发号。
2. `BIZ_TIME` 和 `CREATED_AT` 都是 DM8 `DATE`，包含时间语义。
3. `AMOUNT` 是金额型字段，不能被错误压成整数。
4. `STATUS` 带默认值。
5. `TITLE` 是 `VARCHAR2(2000 CHAR)`，带字符语义。
6. `ATTACHMENT` 是 `BLOB`。

### 迁到 MySQL 会发生什么

如果目标是 MySQL，这张表基本不可能“直接照抄”。

第一，`ID` 这套“序列 + 触发器”模型要重做。最常见的选择是改成 `AUTO_INCREMENT`，也就是说主键生成机制本身变了。

第二，`BIZ_TIME` 和 `CREATED_AT` 不能继续按 `DATE` 去理解，否则时间部分就会丢。更稳妥的落点通常是 `DATETIME`。

第三，`AMOUNT` 这种字段必须明确保留成 `DECIMAL(18,2)`，不能因为它也是 `NUMBER` 就顺手压成整数类。

第四，`STATUS NUMBER(1,0) DEFAULT 0` 在 MySQL 里大概率会变成 `TINYINT` 或 `TINYINT(1)`，这一步如果业务里还混用了布尔语义，就要提前判断到底它是“状态码”还是“真假值”。

第五，`TITLE VARCHAR2(2000 CHAR)` 看起来不大，但在 utf8mb4 场景下它的行内成本和索引成本都要重新评估。如果这个字段还参与索引，风险会进一步放大。

第六，`ATTACHMENT BLOB` 虽然理论上可以落到 `LONGBLOB`，但真正要关注的是导入导出链路是不是能稳定处理大二进制对象。

换句话说，这张表迁到 MySQL 时，真正变化的不是“表名和列名”，而是整套字段语义、默认值行为和主键生成链路。

如果把它写成一个更接近落地结果的示意，大致会变成这样：

```sql
CREATE TABLE ORDER_LOG (
    ID            BIGINT NOT NULL AUTO_INCREMENT,
    BIZ_TIME      DATETIME NOT NULL,
    AMOUNT        DECIMAL(18,2) NOT NULL,
    STATUS        TINYINT DEFAULT 0,
    TITLE         VARCHAR(2000),
    ATTACHMENT    LONGBLOB,
    CREATED_AT    DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (ID)
);
```

注意这里最关键的不是语法，而是背后的处理选择：

1. `NUMBER(19,0)` 没继续保留成 `DECIMAL(19,0)`，而是按主键语义压到了 `BIGINT`。
2. `DATE` 没按名称平移，而是按时间语义落到了 `DATETIME`。
3. `SYSDATE` 没做函数名照搬，而是改成了 MySQL 可执行的 `CURRENT_TIMESTAMP`。
4. 原有 sequence + trigger 被整体替换成 `AUTO_INCREMENT`。

### 迁到 KingbaseES 会发生什么

如果目标是 KingbaseES，整体会顺很多，但仍然不是无脑平移。

`ID` 理论上可以继续走序列，也可以改成 identity，这取决于你想保留原有发号链路，还是借机换成目标库原生方式。这里的关键不是“能不能实现”，而是团队想不想继续维护 sequence + trigger 这种老模式。

`BIZ_TIME` 和 `CREATED_AT` 的核心问题仍然存在，因为 Kingbase 的 `DATE` 通常也是 date-only 语义，所以这类列更适合作为 `TIMESTAMP` 来承接。

`AMOUNT` 基本可以平稳落成 `NUMERIC(18,2)`，这部分通常不会像 MySQL 那样出现明显压缩。

`TITLE` 也通常比 MySQL 更容易保留原有长度逻辑，但如果你的应用严重依赖 Oracle/DM8 风格的字符语义，最好还是做一次边界验证。

也就是说，KingbaseES 路线的核心不是“重做结构”，而是“校准语义”。

更接近落地的示意可以写成：

```sql
CREATE TABLE ORDER_LOG (
    ID            NUMERIC(19,0) NOT NULL,
    BIZ_TIME      TIMESTAMP(0) NOT NULL,
    AMOUNT        NUMERIC(18,2) NOT NULL,
    STATUS        NUMERIC(1,0) DEFAULT 0,
    TITLE         VARCHAR(2000),
    ATTACHMENT    BYTEA,
    CREATED_AT    TIMESTAMP(0) DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (ID)
);
```

如果团队不想继续维护 sequence + trigger，也可以顺势改成 identity；但如果已有大量 SQL、存储过程、触发器依赖 `NEXTVAL`，保留序列链路通常更平滑。

### 迁到神通会发生什么

如果目标是神通，这张表的迁移逻辑会介于前两者之间。

`ID` 这条链路通常还有机会保留序列与触发器思路，但自增模型、触发器细节和约束配套仍然要认真确认，不能因为“都偏 Oracle 风格”就默认兼容。

`BIZ_TIME` 和 `CREATED_AT` 一般比迁到 MySQL、Kingbase 时更自然一些，但仍然需要确认应用层是不是把它们当成“时间字段”来使用。

`TITLE VARCHAR2(2000 CHAR)` 在神通侧更需要警惕字符语义与字节语义的转换，一旦目标库按字节计长，而源字段按字符计长，就可能出现长度放大甚至退化为 `CLOB` 的情况。

`ATTACHMENT BLOB` 也不能只看类型名一致。真正的风险在于大 BLOB 的导出格式、导入写法和执行方式，尤其是在大对象场景下，往往需要额外的分块处理策略。

所以同样一张表，迁到神通时不一定像 MySQL 那样大改结构，但会更考验你对对象细节的校对能力。

如果按“尽量保留源库风格”的思路落地，示意写法会更接近这样：

```sql
CREATE TABLE ORDER_LOG (
    ID            NUMBER(19,0) NOT NULL,
    BIZ_TIME      DATE NOT NULL,
    AMOUNT        NUMBER(18,2) NOT NULL,
    STATUS        NUMBER(1,0) DEFAULT 0,
    TITLE         VARCHAR(2000),
    ATTACHMENT    BLOB,
    CREATED_AT    DATE DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (ID)
);
```

但神通这条路线最值得强调的一点是：DDL 看起来越像“原样迁移”，越不能掉以轻心。因为真正的工作量往往藏在长度语义、触发器细节、BLOB 写入策略这些不那么显眼的地方。

## 如果想放一张总表，这张最够用

一篇文章里如果要放一张足够直观的对比表，下面这张已经足够支撑全文。

| 维度 | MySQL | KingbaseES | 神通（OSCAR） |
|---|---|---|---|
| 与 DM8 类型语义距离 | 远 | 较近 | 较近 |
| `DATE` 迁移风险 | 高 | 高 | 中 |
| `NUMBER` 处理复杂度 | 高 | 中 | 中 |
| 行大小/索引长度压力 | 很高 | 低 | 中 |
| 序列兼容 | 低 | 高 | 高 |
| 触发器兼容 | 低 | 中 | 中到高 |
| 大字段迁移复杂度 | 中 | 中 | 高 |
| 总体迁移改造量 | 大 | 中 | 中 |

如果还想配一句总结语，可以直接写：

> MySQL 的问题主要是限制多，KingbaseES 的问题主要是语义校准，神通的问题主要是细节适配。

## 最后怎么收这篇文章

写到最后，其实不需要再堆太多技术细节。最自然的收束方式，是把问题重新拉回到一条主线上：

DM8 切库，表面上看是在做数据库替换，实际上是在处理三件事：

1. 数据类型语义迁移。
2. 对象模型迁移。
3. 数据写入策略迁移。

如果目标是 MySQL，重点应该放在“结构重塑”和“限制规避”上；如果目标是 KingbaseES，重点应该放在“时间语义”和“兼容模式”上；如果目标是神通，重点则应该放在“触发器、序列、自增、大字段”这些细节上。

真正做过一次数据库迁移的人，最后通常都会同意一句话：

> 数据库迁移从来不是类型替换，而是语义迁移。
