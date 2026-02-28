# DM8 Export Tool 打包与服务化代码审阅（2026-02-06）

## 1. 审阅目标

- 目标：为后续封装 `deb`、`exe`（桌面安装包/服务化部署）提供发布前问题清单与修复路径。
- 结论：当前架构方向正确，但存在若干 **发布阻塞项（P0）**，需先修复再进入正式打包。

## 2. 审阅范围

- 后端：`backend/`（Axum 服务、连接/导出、配置存储）
- 前端：`frontend/`（Vite + React 构建与 API 接入）
- 桌面封装：`src-tauri/`（驱动发现、资源打包、目标平台）
- 文档与仓库配置：`docs/`、`.gitignore`

## 3. 实测结果（关键命令）

- `cd backend && cargo test`：通过（45 tests passed）。
- `cd frontend && npm run lint`：通过（有 warning，无 error）。
- `cd frontend && npm run build`：失败（TypeScript 编译错误）。
- `cd src-tauri && cargo test --test driver_discovery`：失败（缺少 Linux 系统依赖 `libsoup-2.4`）。

---

## 4. 问题清单与解决方案

### P0（阻塞发布，需优先处理）

#### P0-1 前端构建失败，阻塞 Tauri 打包链路

- **现象**：`npm run build` 报 TS 错误，导致 `src-tauri/tauri.conf.json` 的 `beforeBuildCommand` 无法通过。
- **证据**：`frontend/src/components/ExportConfig.tsx:24`、`frontend/src/components/ExportConfig.tsx:117`。
- **影响**：无法生成前端 `dist`，进而无法产出桌面安装包。
- **修复建议**：
  1. 移除未使用状态（`hasError`）或在 UI 中真正使用；
  2. 为 `calcProgress` 增加 TypeScript 类型定义（建议将 `frontend/src/utils/exportProgress.js` 迁移为 `exportProgress.ts`）；
  3. 让 `status` 返回值收敛为 `'normal' | 'active' | 'success' | 'exception'`。
- **验收标准**：`cd frontend && npm run build` 成功。

#### P0-2 Linux Tauri 构建依赖缺失

- **现象**：`soup2-sys` 编译失败，提示未找到 `libsoup-2.4.pc`。
- **证据**：`cd src-tauri && cargo test --test driver_discovery` 失败日志。
- **影响**：Linux 下无法构建桌面端（AppImage/deb 均会受影响）。
- **修复建议**：
  1. 在构建机安装 Tauri Linux 依赖（含 `libwebkit2gtk`、`libsoup2.4`、`pkg-config` 等）；
  2. 将依赖安装写入统一脚本（如 `scripts/bootstrap-tauri-linux.sh`）或 CI workflow。
- **验收标准**：`cd src-tauri && cargo tauri build --target x86_64-unknown-linux-gnu` 可执行到产物阶段。

#### P0-3 当前未配置 `deb` 打包目标

- **现象**：Tauri 仅配置 `appimage` 和 `nsis`。
- **证据**：`src-tauri/tauri.conf.json:17`。
- **影响**：无法直接产出 Debian 包。
- **修复建议**：
  1. 在 `bundle.targets` 中补充 `deb`；
  2. 补充 `deb` 元信息（依赖、分类、说明、图标等）。
- **验收标准**：构建输出目录中出现 `.deb` 产物。

#### P0-4 Windows 驱动资源未就绪，且路径约定不一致

- **现象**：Windows 驱动目录仍是占位说明；代码查找路径与文档描述不一致。
- **证据**：`drivers/dm8/windows/README.md:3`，`src-tauri/src/driver.rs:61`。
- **影响**：即使生成 `.exe`，也可能在目标机无法连接 DM8。
- **修复建议**：
  1. 确认并落地真实 `dmodbc.dll` 及依赖；
  2. 统一驱动目录策略（推荐明确支持 `drivers/dm8/windows/`，并在代码中同步）；
  3. 增加启动时自检与错误提示（缺少 DLL 时给出明确文件名）。
- **验收标准**：Windows 干净环境可启动并通过连接测试。

---

### P1（高优先级，建议发布前修复）

#### P1-1 桌面模式后端仍监听 `0.0.0.0`

- **证据**：`backend/src/lib.rs:35`。
- **风险**：桌面应用本地后端对外暴露，增加攻击面。
- **修复建议**：
  - 默认改为 `127.0.0.1`；仅在明确需要时通过配置开放外网监听。
- **验收标准**：桌面模式下仅本机可访问后端端口。

#### P1-2 CORS 过宽（`permissive`）

- **证据**：`backend/src/api/mod.rs:35`。
- **风险**：任意来源可调用 API（结合开放监听风险更高）。
- **修复建议**：
  - 按运行模式区分：桌面模式限制为本地来源，Web 部署模式按白名单配置。
- **验收标准**：非白名单来源请求被拒绝。

#### P1-3 `SET SCHEMA` 直接字符串拼接

- **证据**：`backend/src/db/connection.rs:113`。
- **风险**：模式名异常时可能触发注入或执行错误。
- **修复建议**：
  - 对 schema 名做白名单校验（字母/数字/下划线）并统一转义策略；
  - 不通过校验则拒绝执行。
- **验收标准**：非法 schema 入参返回校验错误，不进入 SQL 执行。

#### P1-4 导出路径构造缺乏收敛

- **证据**：`backend/src/api/export.rs:36`。
- **风险**：schema 名包含特殊字符时，可能导致路径异常或跨平台兼容问题。
- **修复建议**：
  - 文件名仅允许安全字符集（如 `[A-Za-z0-9_-]`）；
  - 导出目录改为固定基目录（如用户目录下 `Exports`），避免依赖运行时 CWD。
- **验收标准**：异常 schema 名也能稳定生成合法导出路径。

#### P1-5 敏感信息（数据库密码）明文保存在 SQLite

- **证据**：`backend/src/config_store/mod.rs:116`。
- **风险**：主机被读取时可直接获得数据库凭证。
- **修复建议**：
  - 使用系统密钥链（Windows Credential Manager / Linux Secret Service）或最少做对称加密并妥善管理密钥；
  - 支持“仅本次会话保存，不落盘”。
- **验收标准**：本地配置库不再明文存储密码。

---

### P2（工程质量与长期维护）

#### P2-1 忽略 Rust `Cargo.lock` 影响可复现构建

- **证据**：`.gitignore:3`、`.gitignore:5`。
- **风险**：不同时间构建结果可能不一致，CI/本地依赖漂移。
- **修复建议**：
  - 对应用仓库提交 `backend/Cargo.lock` 与 `src-tauri/Cargo.lock`。
- **验收标准**：依赖版本可锁定复现。

#### P2-2 缺少“发布门禁”脚本

- **现象**：当前验证步骤分散，缺少一键发布前检查。
- **修复建议**：
  - 增加 `scripts/release-check.sh`，串联 `cargo test`、`npm run build`、`cargo tauri build`（按平台）等。
- **验收标准**：执行一次脚本即可判断是否可发布。

---

## 5. 建议实施顺序（最短路径）

1. 修复前端 TS 构建问题（P0-1）；
2. 配齐 Linux 构建依赖并验证 Tauri 构建（P0-2）；
3. 完成 `deb` target 与 metadata（P0-3）；
4. 落地 Windows DM8 驱动资源并统一路径策略（P0-4）；
5. 收紧安全边界（P1-1~P1-4）；
6. 处理凭证存储与可复现构建（P1-5、P2-1）；
7. 建立发布门禁脚本与 CI（P2-2）。

## 6. 打包目标达成定义（DoD）

- 可稳定产出并安装：`deb`、`exe`。
- 干净环境（无系统 DM8 驱动）可启动并连接数据库。
- 导出主流程（DDL + Data）可用，配置加载/保存可用。
- 本地后端默认不对外暴露，CORS 按白名单控制。
- 凭证不明文落盘，构建可复现。

