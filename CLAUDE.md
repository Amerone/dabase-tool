# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

这是一个用于导出 DM8（达梦）数据库模式和数据的全栈应用程序。后端使用 Rust + Axum + ODBC，前端使用 React + TypeScript + Vite，采用暗黑科技风格的 UI 设计。

## 运行模式与驱动策略

- **HTTP 模式（当前验证方式）**：本地启动后端 (3000) + Vite 前端 (5173/5174)。启动后端时优先用仓库内驱动：
  ```bash
  cd backend
  ./run_with_dm8_driver.sh  # 推荐：自动设置驱动路径并运行
  # 或手动设置：
  export LD_LIBRARY_PATH=$(pwd)/../drivers/dm8:$LD_LIBRARY_PATH
  export DM8_DRIVER_PATH=$(pwd)/../drivers/dm8/libdodbc.so
  cargo run
  ```
  前端：`cd frontend && npm run dev`
- **驱动优先级**：内置驱动目录 (`drivers/dm8`) → `DM8_DRIVER_PATH` 指定 → 系统 ODBC 配置。连接串会自动带上驱动路径。
- **桌面封装（规划/进行中）**：Tauri 打包 AppImage/Windows exe，随包携带驱动并在启动时注入 `LD_LIBRARY_PATH`/`PATH` 与 `DM8_DRIVER_PATH`。

## 开发环境要求

- Rust 1.70+
- Node.js 18+
- DM8 ODBC 驱动（项目内置于 `drivers/dm8`，也可使用系统安装的驱动）
- DM8 数据库实例（用于测试）

## 常用命令

### 后端开发

```bash
cd backend

# 运行开发服务器（推荐，自动加载内置驱动）
./run_with_dm8_driver.sh

# 运行开发服务器（手动方式）
cargo run

# 运行测试
cargo test

# 检查代码（不构建）
cargo check

# 格式化代码
cargo fmt

# 运行 Clippy 代码检查
cargo clippy

# 生产构建
cargo build --release
```

### 前端开发

```bash
cd frontend

# 安装依赖
npm install

# 运行开发服务器（监听 5173 端口）
npm run dev

# 构建生产版本
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

### 配置文件

后端需要 `backend/.env` 文件（可从 `.env.example` 复制）：

```env
DATABASE_HOST=localhost
DATABASE_PORT=5236
DATABASE_USERNAME=SYSDBA
DATABASE_PASSWORD=SYSDBA
DATABASE_SCHEMA=SYSDBA
SERVER_PORT=3000  # 可选，默认 3000
```

**注意**：`.env` 文件仅用于初始默认值。应用启动后会创建 `~/.amarone/config.db` SQLite 数据库用于持久化连接配置，用户可在前端界面保存和加载连接配置。

## 架构设计

### 后端架构（Rust）

**核心模块结构：**

- `main.rs` - 应用入口，初始化 tracing 日志和 Axum 服务器
- `lib.rs` - 库入口，提供 `start_server()` 和 `init_tracing()` 公共接口
- `api/` - HTTP API 路由层
  - `mod.rs` - 路由定义、CORS 配置和 `AppState`（包含 `ConfigStore`）
  - `connection.rs` - 数据库连接测试接口
  - `schema.rs` - 模式和表信息查询接口
  - `export.rs` - DDL 和数据导出接口
  - `config.rs` - 连接配置持久化接口（GET/POST `/api/config/connection`）
- `db/` - 数据库访问层
  - `connection.rs` - ODBC 连接管理
  - `dm8_adapter.rs` - DM8 数据库适配器（核心数据库操作）
  - `schema.rs` - 模式元数据查询（表、列、索引、约束、触发器、序列）
- `export/` - 导出逻辑层
  - `ddl.rs` - DDL（表结构）生成，支持 DROP IF EXISTS、IDENTITY、DEFAULT、注释
  - `data.rs` - INSERT 语句生成，支持 TRUNCATE/DELETE 清空、批量插入、行数统计
- `models/` - 数据模型定义（`ConnectionConfig`、`Table`、`Column`、`TableDetails` 等）
- `config_store/` - 本地配置存储（SQLite）
  - `mod.rs` - `ConfigStore` 实现，管理 `~/.amarone/config.db`

**关键技术点：**

- 使用 `odbc-api` crate 通过 ODBC 连接 DM8 数据库
- Axum 框架提供异步 HTTP 服务
- `tower-http` 提供 CORS 中间件支持跨域请求
- `tracing` 用于结构化日志记录
- `rusqlite` 用于本地配置持久化（`~/.amarone/config.db`）
- `chrono` 用于时间戳生成（导出文件名、配置更新时间）

### 前端架构（React + TypeScript）

**核心模块结构：**

- `App.tsx` - 应用根组件，配置 Ant Design 暗黑主题和 React Query
- `main.tsx` - 应用入口
- `router/` - React Router 路由配置
- `layouts/` - 布局组件
  - `MainLayout.tsx` - 主布局，包含科技背景和内容区域
- `pages/` - 页面组件
  - `ExportWizard.tsx` - 主要业务页面，多步骤导出向导（使用 anime.js 动画）
- `components/` - UI 组件
  - `TechBackground.tsx` - Canvas 动态粒子背景（核心视觉组件）
  - `ConnectionForm.tsx` - 数据库连接表单（支持加载/保存配置）
  - `TableSelector.tsx` - 表选择器
  - `SchemaExplorer.tsx` - 模式浏览器
  - `ExportConfig.tsx` - 导出配置（DDL/数据、批量大小、drop/truncate 选项）
  - `common/` - 通用组件（`TechCard`、`TechButton`、`SectionHeader`）
- `store/` - 状态管理
  - `useExportStore.ts` - Zustand 全局状态（连接配置、选中表、向导步骤、配置来源）
- `services/` - API 客户端
  - `api.ts` - Axios 封装的后端 API 调用
- `types/` - TypeScript 类型定义

**关键技术点：**

- **状态管理**：使用 Zustand 管理全局状态，避免复杂的 props 传递
- **数据请求**：React Query 处理异步数据获取和缓存
- **路由**：React Router v7 用于页面导航
- **UI 主题**：Ant Design 暗黑模式 + 自定义 Token（主色调 `#00b96b`，圆角 2px）
- **动画**：anime.js 处理页面切换和元素过渡动画
- **代理配置**：Vite 开发服务器代理 `/api` 请求到后端 `http://localhost:3000`
- **路径别名**：`@/` 指向 `src/` 目录

### 数据流

1. 前端通过 `services/api.ts` 发送请求到 `/api/*`
2. Vite 开发服务器代理请求到后端 `localhost:3000`
3. Axum 路由器分发到对应的 API 处理器（`api/` 模块）
4. API 处理器调用 `db/dm8_adapter.rs` 执行 ODBC 操作
5. `export/` 模块生成 DDL 或 INSERT 语句，写入带时间戳的文件（`exports/<schema>_ddl_YYYYMMDD_HHMMSS.sql`）
6. 结果返回前端，前端使用 Zustand 更新状态并渲染 UI

### 配置持久化机制

- **存储位置**：`~/.amarone/config.db`（SQLite 数据库）
- **表结构**：`connections` 表存储连接配置（`name`、`db_type`、`host`、`port`、`username`、`password`、`schema`、`updated_at`）
- **优先级**：前端可加载已保存配置（SQLite）或使用 `.env` fallback
- **API 接口**：
  - `GET /api/config/connection` - 获取默认连接（优先 SQLite，无则返回 `.env`）
  - `POST /api/config/connection` - 保存默认连接到 SQLite
- **前端交互**：`ConnectionForm` 组件提供"加载已保存"和"保存配置"按钮，显示配置来源和更新时间

### 导出功能特性

- **DDL 导出**：
  - 生成 `CREATE TABLE` 语句，包含列定义、主键、索引、约束、触发器
  - 支持 `DROP TABLE IF EXISTS`（可选）
  - 支持 `IDENTITY` 列和 `DEFAULT` 值
  - 包含表和列注释（`COMMENT ON`）
  - 文件名格式：`exports/<schema>_ddl_YYYYMMDD_HHMMSS.sql`
- **数据导出**：
  - 生成 `INSERT` 语句，支持批量插入
  - 支持 `TRUNCATE TABLE` 或 `DELETE FROM`（可选）
  - 统计每个表和总行数
  - 文件名格式：`exports/<schema>_data_YYYYMMDD_HHMMSS.sql`
- **文件头部**：包含生成时间、表数量、总行数、警告信息

### UI 设计系统

**暗黑科技风格特征：**

- **色彩**：主色调 Cyber Green (`#00b96b`)，背景深蓝黑渐变 (`#001529` → `#000000`)
- **视觉效果**：
  - `TechBackground.tsx` 使用 Canvas 绘制动态网格和粒子连接
  - 玻璃拟态效果（半透明背景 + `backdrop-filter: blur`）
  - 锐利边角（`borderRadius: 2px`）
- **交互动画**：anime.js 实现淡入、位移、缩放等过渡效果

## 开发注意事项

### 后端开发

- **ODBC 驱动**：优先使用 `./run_with_dm8_driver.sh` 启动，自动加载内置驱动
- **连接池**：当前实现为每次请求创建新连接，大规模使用时考虑添加连接池
- **错误处理**：使用 `anyhow` 和 `thiserror` 处理错误，API 返回统一的 JSON 格式
- **日志级别**：通过环境变量 `RUST_LOG` 控制，默认 `dm8_export_backend=debug,tower_http=debug`
- **配置存储**：`ConfigStore` 在 `~/.amarone/config.db` 存储连接配置，启动时自动初始化
- **导出文件**：生成的 SQL 文件存储在 `backend/exports/` 目录，文件名包含时间戳

### 前端开发

- **类型安全**：所有 API 响应都有对应的 TypeScript 类型定义（`types/index.ts`）
- **状态管理**：优先使用 Zustand store，避免 prop drilling
- **动画性能**：`TechBackground.tsx` 使用 `requestAnimationFrame`，注意性能优化
- **代码风格**：项目使用 ESLint Flat Config 和 Prettier，提交前运行 `npm run lint`
- **配置来源**：`useExportStore` 跟踪配置来源（`saved` 或 `manual`）和更新时间

### 关键文件说明

- `backend/src/db/dm8_adapter.rs` - 所有 DM8 数据库操作的核心实现
- `backend/src/db/schema.rs` - 元数据查询（表、列、索引、约束、触发器、序列）
- `backend/src/export/ddl.rs` - DDL 生成逻辑，包含 DROP、IDENTITY、DEFAULT 支持
- `backend/src/export/data.rs` - 数据导出逻辑，包含 TRUNCATE、批量插入、行数统计
- `backend/src/config_store/mod.rs` - SQLite 配置存储实现
- `frontend/src/store/useExportStore.ts` - 应用状态的唯一真实来源
- `frontend/src/components/TechBackground.tsx` - 视觉效果的核心，包含 Canvas 绘图逻辑
- `frontend/src/pages/ExportWizard.tsx` - 主要业务流程，协调各个组件
- `frontend/src/components/ConnectionForm.tsx` - 连接表单，支持加载/保存配置

## API 端点

- `GET /api/health` - 健康检查
- `POST /api/connection/test` - 测试数据库连接
- `GET /api/config/connection` - 获取默认连接配置（优先 SQLite，无则 `.env`）
- `POST /api/config/connection` - 保存默认连接配置到 SQLite
- `GET /api/schemas` - 列出所有模式
- `GET /api/tables` - 列出指定模式的所有表（包含行数）
- `GET /api/tables/:table/details` - 获取表详细信息（列、索引、约束、触发器）
- `POST /api/export/ddl` - 导出表结构（DDL），支持 `drop_existing` 选项
- `POST /api/export/data` - 导出表数据（INSERT 语句），支持 `batch_size` 和 `include_row_counts` 选项

## 故障排查

### 后端无法启动

- 检查 DM8 ODBC 驱动是否正确安装或使用 `./run_with_dm8_driver.sh`
- 验证 `.env` 文件配置是否正确
- 查看日志输出中的错误信息（`RUST_LOG=debug`）
- 确认 `~/.amarone/` 目录有写权限（用于 SQLite 配置存储）

### 前端无法连接后端

- 确认后端服务运行在 `localhost:3000`
- 检查 Vite 代理配置（`vite.config.ts`）
- 使用浏览器开发者工具查看网络请求
- 确认 CORS 配置正确（后端使用 `CorsLayer::permissive()`）

### 导出失败

- 检查数据库连接是否正常（使用"测试连接"功能）
- 验证表名和模式名是否正确
- 查看后端日志中的 ODBC 错误信息
- 确认 `backend/exports/` 目录存在且有写权限
- 对于大表，调整 `batch_size` 参数避免内存溢出

### 配置持久化问题

- 确认 `~/.amarone/config.db` 文件存在且可读写
- 检查后端日志中的 SQLite 错误信息
- 如果配置损坏，删除 `~/.amarone/config.db` 重新初始化
- 前端显示配置来源（`sqlite` 或 `env`）和更新时间

## 测试

### 后端测试

```bash
cd backend
cargo test                    # 运行所有测试
cargo test config_store       # 运行配置存储测试
cargo test --lib              # 仅运行库测试
```

### 前端测试

```bash
cd frontend
npm run lint                  # 运行 ESLint 检查
npm run format                # 格式化代码
```

## 最近实现的功能

参考 `docs/plans/2026-01-28-ddl-data-export-fixes.md` 了解最近的导出功能增强：
- 列元数据增强（`default_value`、`identity`、`identity_start`、`identity_increment`）
- DDL 生成支持 `DROP TABLE IF EXISTS`、`IDENTITY`、`DEFAULT` 值
- 数据导出支持 `TRUNCATE TABLE`、行数统计
- 文件名包含时间戳和元数据头部
