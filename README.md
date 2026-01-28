# DM8 数据库导出工具

一个现代化的全栈应用程序，用于导出 DM8（达梦）数据库模式和数据，配备精美的暗黑主题界面。

![技术栈](https://img.shields.io/badge/Rust-Axum-orange)
![技术栈](https://img.shields.io/badge/React-TypeScript-blue)
![技术栈](https://img.shields.io/badge/UI-Ant%20Design-1890ff)

## 🧭 设计思路与运行模式

- **分层设计**：前端 (React/Vite) 只负责 UI 与 API 调用；后端 (Rust/Axum) 负责 ODBC 访问与导出逻辑，接口清晰。
- **驱动内置**：仓库自带 DM8 ODBC 驱动及依赖（`drivers/dm8`），后端优先使用内置路径；环境变量 `DM8_DRIVER_PATH`/`LD_LIBRARY_PATH` 可覆盖。
- **运行模式**：
  - **HTTP 开发**：本地起后端 (3000) + Vite 前端 (5173/5174)，按下文命令启动。
  - **桌面封装（规划中）**：Tauri 打包 AppImage/Windows exe，随包携带驱动并自动注入搜索路径。
- **数据流**：前端经 `/api/*` → Axum 路由 → `db` 层 ODBC → `export` 生成 DDL/数据 → 返回前端。

## ✨ 功能特性

- 🔌 **数据库连接** - 通过 ODBC 连接 DM8 数据库，支持连接测试
- 📊 **模式浏览** - 浏览模式、表，查看详细的表结构
- 📝 **DDL 导出** - 导出表结构为 CREATE TABLE 语句
- 💾 **数据导出** - 导出表数据为 INSERT 语句，支持配置批量大小
- 🎨 **现代化界面** - 暗黑主题界面，配备动态粒子背景
- ⚡ **高性能** - Rust 后端实现快速数据处理
- 🌐 **基于 Web** - 无需安装，通过浏览器访问
- 🀄 **中文界面** - 前端全中文；表列表实时行数（缺省时回退 `COUNT(*)`），已选表侧栏展示行数/列数/主键列数

## 🏗️ 技术架构

### 后端（Rust）
- **框架**: Axum（异步 Web 框架）
- **数据库**: ODBC-API 用于 DM8 连接
- **日志**: Tracing 结构化日志
- **序列化**: Serde 处理 JSON

### 前端（React + TypeScript）
- **构建工具**: Vite
- **UI 库**: Ant Design 自定义暗黑主题
- **状态管理**: Zustand
- **数据请求**: React Query + Axios
- **动画**: Anime.js 实现平滑过渡
- **路由**: React Router v7

## 📁 项目结构

```
tool-database/
├── backend/                    # Rust API 服务器
│   ├── src/
│   │   ├── api/               # HTTP 路由处理器
│   │   │   ├── connection.rs  # 连接测试
│   │   │   ├── schema.rs      # 模式/表查询
│   │   │   └── export.rs      # 导出端点
│   │   ├── db/                # 数据库层
│   │   │   ├── dm8_adapter.rs # DM8 ODBC 操作
│   │   │   ├── connection.rs  # 连接管理
│   │   │   └── schema.rs      # 元数据查询
│   │   ├── export/            # 导出逻辑
│   │   │   ├── ddl.rs         # DDL 生成
│   │   │   └── data.rs        # INSERT 生成
│   │   ├── models/            # 数据模型
│   │   └── main.rs            # 应用入口
│   ├── Cargo.toml
│   └── .env.example
│
├── frontend/                   # React 应用
│   ├── src/
│   │   ├── components/        # UI 组件
│   │   │   ├── TechBackground.tsx    # 动态背景
│   │   │   ├── ConnectionForm.tsx    # 数据库连接表单
│   │   │   ├── TableSelector.tsx     # 表选择器
│   │   │   └── ExportConfig.tsx      # 导出选项
│   │   ├── pages/
│   │   │   └── ExportWizard.tsx      # 主向导页面
│   │   ├── layouts/
│   │   │   └── MainLayout.tsx        # 应用布局
│   │   ├── store/
│   │   │   └── useExportStore.ts     # 全局状态
│   │   ├── services/
│   │   │   └── api.ts                # API 客户端
│   │   ├── types/             # TypeScript 类型
│   │   └── App.tsx            # 根组件
│   ├── package.json
│   └── vite.config.ts
│
├── README.md
└── CLAUDE.md                   # 开发指南
```

## 🚀 快速开始

### 环境要求

- **Rust** 1.70 或更高版本
- **Node.js** 18 或更高版本
- **DM8 ODBC 驱动**（必须安装在系统中）
- **DM8 数据库**实例（用于测试）

### 安装步骤

#### 1. 克隆仓库

```bash
git clone <repository-url>
cd tool-database
```

#### 2. 后端设置

```bash
cd backend

# 安装依赖并构建
cargo build

# 创建环境配置文件
cp .env.example .env
# 编辑 .env 填入你的数据库凭据
```

创建 `backend/.env` 文件：

```env
DATABASE_HOST=localhost
DATABASE_PORT=5236
DATABASE_USERNAME=SYSDBA
DATABASE_PASSWORD=SYSDBA
DATABASE_SCHEMA=SYSDBA
SERVER_PORT=3000
```

#### 内置 DM8 ODBC 驱动
- 项目已在 `drivers/dm8` 下打包了 `libdodbc.so` 及其依赖（`libdmdpi.so`, `libdmfldr.so`），无需手动配置系统路径。
- 运行后端时使用脚本自动注入 `LD_LIBRARY_PATH` 和 `DM8_DRIVER_PATH`：
  ```bash
  cd backend
  ./run_with_dm8_driver.sh    # 等价 cargo run，但自动加载内置驱动
  ```
  如需自定义，`DM8_DRIVER_PATH` 指向驱动文件，`LD_LIBRARY_PATH` 包含驱动所在目录即可。

#### 3. 前端设置

```bash
cd frontend

# 安装依赖
npm install
```

### 运行应用

#### 开发模式

**终端 1 - 启动后端：**
```bash
cd backend
cargo run
```
后端将在 `http://localhost:3000` 启动

**终端 2 - 启动前端：**
```bash
cd frontend
npm run dev
```
前端将在 `http://localhost:5173` 启动

在浏览器中打开 `http://localhost:5173`

#### 生产构建

**后端：**
```bash
cd backend
cargo build --release
./target/release/dm8-export-backend
```

**前端：**
```bash
cd frontend
npm run build
npm run preview
```

## 📖 使用指南

### 步骤 1：配置连接
1. 输入 DM8 数据库连接信息：
   - 主机地址
   - 端口
   - 用户名
   - 密码
   - 模式名
2. 点击"测试连接"验证连通性

### 步骤 2：选择表
1. 浏览模式中的可用表
2. 查看表详情（列、类型、约束）
3. 选择要导出的表

### 步骤 3：配置导出
1. 选择导出选项：
   - 导出 DDL（表结构）
   - 导出数据（INSERT 语句）
   - 数据导出的批量大小
2. 点击"导出"生成 SQL 文件

### 步骤 4：下载
1. 查看生成的 SQL
2. 下载文件到本地

## 连接配置持久化设计（草案）

目标：把 .env 默认连接搬到页面，允许用户修改并持久化，后续可扩展多数据库类型。

存储：
- SQLite 文件：`~/.amarone/config.db`，服务启动时自动创建文件夹 `.amarone` 和库。
- 表 `connections`：`id`(PK)，`name`(默认 `default-dm8`)，`db_type`(`dm8`)，`host`，`port`，`username`，`password`，`schema`，`updated_at`。
- 密码本地明文存储，若需加密后续增加。

后端接口与优先级：
- `GET /api/config/connection`：返回默认连接（先查 SQLite，无则读取 `.env` fallback）。
- `POST /api/config/connection`：写入/覆盖默认连接记录。
- 连接和导出接口继续接受请求体内的配置；若前端选择“使用已保存”，则调用 GET 并填充表单。
- 未来支持多连接：预留 `GET/POST/DELETE /api/config/connections/:id` 与 `db_type` 扩展。

前端流程：
- 页面加载时调用 GET 默认连接填充表单；用户可编辑后“保存配置”（POST）和“测试连接”。
- 持久化按钮与测试按钮分开；保存成功后在状态区域提示来源（SQLite）及更新时间。
- 当前只展示 dm8 字段，日后根据 `db_type` 展示不同字段集。

运维注意：
- SQLite 存于当前用户 home，适合单机；若部署多实例需挂载共享或改为外部配置存储。
- `.env` 仍用于初次引导/无记录时的默认值。

### 后端架构与数据流
- 目录与启动：新增 `backend/src/config_store` 模块，启动时确保 `~/.amarone` 目录与 `config.db` 存在，表缺失则建表。
- 依赖：使用 `rusqlite` 做轻量同步访问，API handler 内短事务读写；抽象接口便于未来切换存储。
- 表：`connections(id INTEGER PK AUTOINCREMENT, name TEXT UNIQUE, db_type TEXT, host TEXT, port INTEGER, username TEXT, password TEXT, schema TEXT, updated_at TEXT)`，默认记录 `name='default-dm8'`。
- 接口：`GET /api/config/connection` 先查 SQLite，无则返回 `.env` fallback；`POST /api/config/connection` 校验后 UPSERT 默认记录并返回 `updated_at`；连接/导出接口继续接受请求体配置，如需默认则内部调用 `config_store::get_default()`。
- 错误处理：SQLite 失败返回 500，附错误信息；返回体加 `source` 字段标记 `sqlite` 或 `env`。

### 前端交互与状态同步
- 类型与状态：`ConnectionConfig` 增加可选 `source`、`updated_at`；`useExportStore` 增加 `loadedFrom`（`saved`/`manual`）用于提示来源。
- API：在 `services/api.ts` 新增 `getSavedConnection`（GET `/api/config/connection`）与 `saveConnection`（POST 同路径）。
- UI：`ConnectionForm` 新增“加载已保存”和“保存配置”按钮；加载成功填充表单并提示来源/时间，但仍需测试连接；保存成功提示并更新来源时间。未保存时进入下一步给出提示。
- 流程：测试连接仍走 `/api/connection/test`；若来源是 env fallback，提示“仅内存加载，建议保存到本地”。预留 `db_type` 选择框（默认 dm8，禁用其他）。

### 测试策略与落地步骤
- 后端测试：`config_store` 增查覆写单测（临时 SQLite），验证 `.env` fallback 与 `source`；集成测试覆盖 `GET/POST /api/config/connection` 与连接测试调用。
- 前端测试：组件测试验证加载/保存成功与失败时的 UI 状态；E2E 覆盖“加载→修改→保存→测试→下一步”主路径与未保存提示。
- 手动验收：空库返回 `.env`；保存后返回 SQLite；导出流程正常。
- 实施顺序：1) 引入 `rusqlite` 与 `config_store` 初始化；2) 实现配置接口；3) 前端扩展类型/store/API；4) 更新表单 UI；5) 补测试；6) 手动回归。

## 🛠️ 开发

### 后端命令

```bash
cd backend

# 开发模式运行
cargo run

# 运行测试
cargo test

# 检查代码（不构建）
cargo check

# 格式化代码
cargo fmt

# 运行代码检查
cargo clippy
```

### 前端命令

```bash
cd frontend

# 开发服务器
npm run dev

# 生产构建
npm run build

# 预览生产构建
npm run preview

# 代码检查
npm run lint

# 自动修复 lint 问题
npm run lint:fix

# 格式化代码
npm run format
```

## 🔌 API 端点

| 方法 | 端点 | 描述 |
|------|------|------|
| GET | `/api/health` | 健康检查 |
| POST | `/api/connection/test` | 测试数据库连接 |
| GET | `/api/config/connection` | 获取默认连接（优先 SQLite，无则 .env） |
| POST | `/api/config/connection` | 保存默认连接到本地 SQLite |
| GET | `/api/schemas` | 列出所有模式 |
| GET | `/api/tables` | 列出模式中的表 |
| GET | `/api/tables/:table/details` | 获取表详情 |
| POST | `/api/export/ddl` | 导出表 DDL |
| POST | `/api/export/data` | 导出表数据 |

### API 请求示例

```bash
# 测试连接
curl -X POST http://localhost:3000/api/connection/test \
  -H "Content-Type: application/json" \
  -d '{
    "host": "localhost",
    "port": 5236,
    "username": "SYSDBA",
    "password": "SYSDBA",
    "schema": "SYSDBA"
  }'
```

## 🎨 UI 设计系统

应用采用**暗黑科技风格主题**，特点如下：

- **色彩体系**：
  - 主色调：赛博绿（`#00b96b`）
  - 背景：深蓝黑渐变（`#001529` → `#000000`）
  - 强调色：亮青色用于高亮

- **视觉效果**：
  - 带连接线的动态粒子背景
  - 玻璃拟态效果（毛玻璃效果）的卡片
  - 锐利边角（2px 圆角）营造科技感
  - 使用 anime.js 实现平滑过渡

- **排版**：
  - 等宽字体用于代码和数据
  - 清晰的无衬线字体用于 UI 文本

## 🐛 故障排查

### 后端无法启动

**问题**：找不到 ODBC 驱动
```
解决方案：确保 DM8 ODBC 驱动已正确安装
- Linux：检查 /etc/odbcinst.ini
- Windows：检查 ODBC 数据源管理器
```

**问题**：连接被拒绝
```
解决方案：验证 DM8 数据库正在运行且可访问
- 检查 .env 中的主机和端口
- 使用 DM8 客户端工具测试连接
```

### 前端无法连接后端

**问题**：API 请求失败，出现 CORS 错误
```
解决方案：后端 CORS 已配置为宽松模式
- 确保后端运行在 3000 端口
- 检查 vite.config.ts 中的代理配置
```

**问题**：API 调用返回 404 错误
```
解决方案：验证后端正在运行
- 检查后端日志中的错误
- 确保 API 路由已正确注册
```

### 导出失败

**问题**：大表导出超时
```
解决方案：调整批量大小或超时设置
- 在导出配置中减小批量大小
- 增加 frontend/src/services/api.ts 中的 axios 超时时间
```

## 🤝 贡献

欢迎贡献！请遵循以下指南：

1. Fork 本仓库
2. 创建特性分支（`git checkout -b feature/amazing-feature`）
3. 提交更改（`git commit -m 'Add amazing feature'`）
4. 推送到分支（`git push origin feature/amazing-feature`）
5. 开启 Pull Request

### 代码风格

- **Rust**：遵循标准 Rust 规范，使用 `cargo fmt`
- **TypeScript**：遵循 ESLint 规则，使用 Prettier 格式化
- **提交**：使用约定式提交消息

## 📄 许可证

本项目采用 MIT 许可证 - 详见 LICENSE 文件。

## 🙏 致谢

- [Axum](https://github.com/tokio-rs/axum) - Rust Web 框架
- [Ant Design](https://ant.design/) - React UI 库
- [DM8](https://www.dameng.com/) - 达梦数据库
- [Vite](https://vitejs.dev/) - 下一代前端构建工具

## 📞 支持

如有问题和疑问：
- 在 GitHub 上提交 issue
- 查看 [CLAUDE.md](./CLAUDE.md) 文件获取开发指导

---

使用 ❤️ 和 Rust + React 构建
