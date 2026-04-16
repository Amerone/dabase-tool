# KingBase PostgreSQL ODBC 降级方案

## 概述

由于 KingBase 原生 ODBC 驱动 (`kdbodbc30a.dll`) 依赖特定版本的 Visual C++ Redistributable，在某些环境中可能无法加载。本项目实现了使用 PostgreSQL ODBC 驱动作为降级方案，因为 KingBase 基于 PostgreSQL，两者协议兼容。

## 工作原理

### Windows 平台驱动选择优先级

1. **环境变量指定** (`KINGBASE_ODBC_DRIVER_PATH`)
2. **PostgreSQL ODBC 驱动**（推荐，更稳定）
   - `drivers/postgresql/windows/psqlodbc35w.dll`
   - `drivers/postgresql/windows/psqlodbc30a.dll`
3. **KingBase 原生驱动**（降级）
   - `drivers/kingbase/X64_Windows/odbc/x64_ANSI_Release/kdbodbc30a.dll`
4. **注册表驱动名称** (`KINGBASE_ODBC_DRIVER` 环境变量或默认名称)

### Linux 平台

Linux 平台优先使用 KingBase 原生驱动：
- `drivers/kingbase/libkdbodbcw.so`
- `drivers/kingbase/libkdbodbc.so`
- `drivers/kingbase/X64_Linux/odbc/kdbodbcw.so`

## PostgreSQL ODBC 驱动集成

### 驱动文件位置

```
drivers/postgresql/windows/
├── psqlodbc35w.dll          # Unicode 驱动（推荐）
├── psqlodbc30a.dll          # ANSI 驱动
├── libpq.dll                # PostgreSQL 客户端库
├── libssl-3-x64.dll         # OpenSSL
├── libcrypto-3-x64.dll      # OpenSSL 加密库
├── vcruntime140.dll         # VC++ 运行时
└── msvcp140.dll             # VC++ 标准库
```

### 自动注册

后端启动时会自动：
1. 检测 PostgreSQL ODBC 驱动文件
2. 将驱动目录添加到 `PATH` 环境变量
3. 注册驱动到 Windows 注册表（HKCU 或 HKLM）

## 已知限制

### Windows ODBC Driver Manager 限制

**问题**：ODBC Driver Manager 在某些应用程序上下文中只查找 `HKLM` 注册表，忽略 `HKCU`。

**影响**：
- 非管理员权限运行时，驱动注册到 `HKCU`
- 某些 ODBC 调用可能返回 `IM002` 错误（"未发现数据源名称并且未指定默认驱动程序"）

**解决方案**（3选1）：

#### 方案 1：以管理员身份运行后端（推荐）

```bash
# Windows PowerShell（右键"以管理员身份运行"）
cd E:\self\tool-database\backend
cargo run
```

驱动会自动注册到 `HKLM`，所有后续连接正常工作。

#### 方案 2：手动安装 PostgreSQL ODBC 驱动

```bash
# 以管理员身份运行
cd E:\self\tool-database\drivers\postgresql\windows
msiexec /i psqlodbc_x64.msi
```

官方安装程序会自动注册到 `HKLM`。

#### 方案 3：安装 VC++ Redistributable 使用 KingBase 原生驱动

下载并安装：[Microsoft Visual C++ Redistributable (x64)](https://aka.ms/vs/17/release/vc_redist.x64.exe)

安装后 KingBase 原生驱动即可正常加载。

## 连接字符串

### PostgreSQL ODBC 驱动连接 KingBase

```
DRIVER={PostgreSQL Unicode};SERVER=127.0.0.1;PORT=54321;UID=system;PWD=password;DATABASE=platform
```

### KingBase 原生驱动

```
DRIVER={KingbaseES 9 ODBC Driver ANSI};SERVER=127.0.0.1;PORT=54321;UID=system;PWD=password;DATABASE=platform
```

## 环境变量配置

### 强制使用特定驱动

```bash
# 使用 PostgreSQL ODBC 驱动
export KINGBASE_ODBC_DRIVER_PATH=/path/to/psqlodbc35w.dll

# 使用 KingBase 原生驱动
export KINGBASE_ODBC_DRIVER_PATH=/path/to/kdbodbc30a.dll

# 使用注册表中的驱动名称
export KINGBASE_ODBC_DRIVER="PostgreSQL Unicode"
```

## 验证驱动加载

### 检查驱动注册

```powershell
# 查看 HKCU 注册的驱动
reg query "HKCU\SOFTWARE\ODBC\ODBCINST.INI\ODBC Drivers"

# 查看 HKLM 注册的驱动（需要管理员权限）
reg query "HKLM\SOFTWARE\ODBC\ODBCINST.INI\ODBC Drivers"
```

### 测试 DLL 加载

```powershell
cd E:\self\tool-database\drivers\postgresql\windows
$env:PATH = "$PWD;$env:PATH"

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class DllTest {
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern IntPtr LoadLibraryEx(string lpFileName, IntPtr hFile, uint dwFlags);
}
"@

$h = [DllTest]::LoadLibraryEx("psqlodbc35w.dll", [IntPtr]::Zero, 0)
if ($h -ne [IntPtr]::Zero) {
    Write-Host "DLL loaded successfully"
} else {
    Write-Host "DLL load failed"
}
```

## 代码实现

### 驱动选择逻辑

参见 `backend/src/db/connection.rs`:

```rust
fn kingbase_driver_value() -> String {
    #[cfg(windows)]
    {
        // 优先使用 PostgreSQL ODBC 驱动
        let pg_candidates = [
            "drivers/postgresql/windows/psqlodbc35w.dll",
            "../drivers/postgresql/windows/psqlodbc35w.dll",
        ];
        if first_existing_path(&pg_candidates).is_some() {
            return wrap_driver("PostgreSQL Unicode");
        }

        // 降级到 KingBase 原生驱动
        // ...
    }
}
```

### 驱动注册

参见 `backend/src/lib.rs`:

```rust
#[cfg(windows)]
{
    register_bundled_driver(
        "PostgreSQL Unicode",
        "POSTGRESQL_ODBC_DRIVER_PATH",
        &[
            "drivers/postgresql/windows/psqlodbc35w.dll",
            "../drivers/postgresql/windows/psqlodbc35w.dll",
        ],
        false, // optional driver
    );
}
```

## 测试状态

- ✅ 代码实现完成（97/97 单元测试通过）
- ✅ PostgreSQL ODBC 驱动已集成
- ✅ 驱动 DLL 可正常加载
- ⚠️ 需要管理员权限完成 HKLM 注册（或手动安装驱动）

## 参考资料

- [PostgreSQL ODBC 官方文档](https://odbc.postgresql.org/)
- [KingBase 官方文档](https://help.kingbase.com.cn/)
- [Windows ODBC Driver Manager](https://docs.microsoft.com/en-us/sql/odbc/reference/develop-app/driver-manager)
