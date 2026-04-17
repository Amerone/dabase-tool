//! 真实数据库集成测试
//!
//! 所有测试默认 `#[ignore]`，需显式开启：
//!   cargo test --test integration_db -- --ignored --nocapture
//!
//! DM8 环境变量（有默认值）：
//!   DM8_HOST / DM8_PORT / DM8_USER / DM8_PASS / DM8_SCHEMA
//!
//! MySQL / KingBase / 神通：对应 HOST 未设置时自动跳过。

use std::sync::{Arc, Once};

use axum::extract::{Json, State};
use dm8_export_backend::{
    api::{
        export::{export_data, export_ddl},
        AppState,
    },
    config_store::ConfigStore,
    db::service,
    models::{ConnectionConfig, DbType, ExportRequest, TableIdentifier},
};

// ─── 全局初始化 ──────────────────────────────────────────────────────────────

static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        dotenv::dotenv().ok();
        dm8_export_backend::init_tracing();
        #[cfg(windows)]
        register_odbc_drivers();
    });
}

#[cfg(windows)]
fn register_odbc_drivers() {
    use dm8_export_backend::db::odbc_register;
    for (name, candidates) in [
        (
            odbc_register::DM8_DRIVER_NAME,
            odbc_register::DM8_DRIVER_CANDIDATES,
        ),
        (
            odbc_register::KINGBASE_DRIVER_NAME,
            odbc_register::KINGBASE_DRIVER_CANDIDATES,
        ),
        (
            odbc_register::SHENTONG_DRIVER_NAME,
            odbc_register::SHENTONG_DRIVER_CANDIDATES,
        ),
    ] {
        for path in candidates {
            if std::path::Path::new(path).exists() {
                let _ = odbc_register::ensure_odbc_driver_registered(name, path);
                break;
            }
        }
    }
}

// ─── 辅助：从 ApiResult 中取文件路径字符串 ───────────────────────────────────

fn file_path_from_ok(
    r: &axum::Json<
        dm8_export_backend::models::ApiResponse<dm8_export_backend::models::ExportResponse>,
    >,
) -> &str {
    r.0.data
        .as_ref()
        .and_then(|d| d.file_path.as_deref())
        .unwrap_or("-")
}

// ─── 配置 ────────────────────────────────────────────────────────────────────

fn dm8_config() -> ConnectionConfig {
    ConnectionConfig {
        db_type: DbType::Dm8,
        host: std::env::var("DM8_HOST").unwrap_or_else(|_| "localhost".into()),
        port: std::env::var("DM8_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5236),
        username: std::env::var("DM8_USER").unwrap_or_else(|_| "SYSDBA".into()),
        password: std::env::var("DM8_PASS").unwrap_or_else(|_| "SYSDBA001".into()),
        schema: std::env::var("DM8_SCHEMA").unwrap_or_else(|_| "PLATFORM".into()),
        export_schema: None,
        database: None,
    }
}

fn mysql_config() -> Option<ConnectionConfig> {
    let host = std::env::var("MYSQL_HOST").ok()?;
    Some(ConnectionConfig {
        db_type: DbType::Mysql,
        host,
        port: std::env::var("MYSQL_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3306),
        username: std::env::var("MYSQL_USER").unwrap_or_else(|_| "root".into()),
        password: std::env::var("MYSQL_PASS").unwrap_or_default(),
        schema: std::env::var("MYSQL_SCHEMA").unwrap_or_else(|_| "test".into()),
        export_schema: None,
        database: None,
    })
}

fn kingbase_config() -> Option<ConnectionConfig> {
    let host = std::env::var("KB_HOST").ok()?;
    Some(ConnectionConfig {
        db_type: DbType::Kingbase,
        host,
        port: std::env::var("KB_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(54321),
        username: std::env::var("KB_USER").unwrap_or_else(|_| "system".into()),
        password: std::env::var("KB_PASS").unwrap_or_default(),
        schema: std::env::var("KB_SCHEMA").unwrap_or_else(|_| "public".into()),
        export_schema: None,
        database: None,
    })
}

fn shentong_config() -> Option<ConnectionConfig> {
    let host = std::env::var("ST_HOST").ok()?;
    Some(ConnectionConfig {
        db_type: DbType::Shentong,
        host,
        port: std::env::var("ST_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2003),
        username: std::env::var("ST_USER").unwrap_or_else(|_| "sysdba".into()),
        password: std::env::var("ST_PASS").unwrap_or_default(),
        schema: std::env::var("ST_SCHEMA").unwrap_or_else(|_| "sysdba".into()),
        export_schema: None,
        database: std::env::var("ST_DATABASE").ok(),
    })
}

fn base_export_req(config: ConnectionConfig) -> ExportRequest {
    ExportRequest {
        export_schema: Some(config.schema.clone()),
        config,
        target_dialect: None,
        export_directory: None,
        export_compat: Some("script".into()),
        tables: vec![],
        include_ddl: true,
        include_data: false,
        batch_size: Some(100),
        drop_existing: true,
        include_row_counts: false,
        strict_mode: false,
        identifier_case: None,
    }
}

fn test_state() -> (AppState, tempfile::TempDir) {
    let dir = tempfile::TempDir::new().unwrap();
    let store = ConfigStore::new_with_path(dir.path().join("config.db")).unwrap();
    (
        AppState {
            config_store: Arc::new(store),
        },
        dir,
    )
}

/// Convert a list of table name strings into `TableIdentifier` with the given schema.
fn table_ids(schema: &str, names: Vec<String>) -> Vec<TableIdentifier> {
    names
        .into_iter()
        .map(|name| TableIdentifier {
            schema: schema.to_string(),
            name,
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════
//  DM8
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn dm8_01_connection_test() {
    setup();
    let result = service::test_connection(&dm8_config()).await;
    assert!(result.is_ok(), "DM8 连接失败: {:?}", result.err());
    println!("[DM8] 连接测试 ✓");
}

#[tokio::test]
#[ignore]
async fn dm8_02_list_schemas() {
    setup();
    let schemas = service::list_schemas(&dm8_config())
        .await
        .expect("list_schemas 失败");
    println!(
        "[DM8] {} 个 schema: {:?}",
        schemas.len(),
        &schemas[..schemas.len().min(8)]
    );
    assert!(!schemas.is_empty(), "未返回任何 schema");
}

#[tokio::test]
#[ignore]
async fn dm8_03_list_tables() {
    setup();
    let cfg = dm8_config();
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    println!("[DM8] schema={} → {} 张表", cfg.schema, tables.len());
    for t in tables.iter().take(5) {
        println!("  {} (rows={:?})", t.name, t.row_count);
    }
    assert!(!tables.is_empty(), "schema 中未找到任何表");
}

#[tokio::test]
#[ignore]
async fn dm8_04_table_details() {
    setup();
    let cfg = dm8_config();
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    assert!(!tables.is_empty(), "无表可查");

    let name = &tables[0].name;
    let d = service::get_table_details(&cfg, &cfg.schema, name)
        .await
        .expect("get_table_details 失败");
    println!(
        "[DM8] {} → {} 列 / {} PK / {} 索引 / {} 触发器",
        name,
        d.columns.len(),
        d.primary_keys.len(),
        d.indexes.len(),
        d.triggers.len()
    );
    assert!(!d.columns.is_empty(), "表 {} 应有列信息", name);
}

#[tokio::test]
#[ignore]
async fn dm8_05_export_ddl_self() {
    setup();
    let cfg = dm8_config();
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    assert!(!tables.is_empty(), "无表可导出");

    let names: Vec<String> = tables.iter().take(2).map(|t| t.name.clone()).collect();
    let mut req = base_export_req(cfg.clone());
    req.target_dialect = Some(DbType::Dm8);
    req.tables = table_ids(&cfg.schema, names);

    let (state, _dir) = test_state();
    let result = export_ddl(State(state), Json(req)).await;
    match &result {
        Ok(r) => println!("[DM8→DM8] DDL 导出 ✓  file={}", file_path_from_ok(r)),
        Err(e) => println!("[DM8→DM8] DDL 导出失败 status={:?} msg={:?}", e.0, e.1),
    }
    assert!(result.is_ok(), "DM8 自身 DDL 导出失败");
}

#[tokio::test]
#[ignore]
async fn dm8_06_export_data_self() {
    setup();
    let cfg = dm8_config();
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    assert!(!tables.is_empty(), "无表可导出");

    let names: Vec<String> = tables.iter().take(1).map(|t| t.name.clone()).collect();
    let mut req = base_export_req(cfg.clone());
    req.target_dialect = Some(DbType::Dm8);
    req.tables = table_ids(&cfg.schema, names);
    req.include_ddl = false;
    req.include_data = true;
    req.include_row_counts = true;

    let (state, _dir) = test_state();
    let result = export_data(State(state), Json(req)).await;
    match &result {
        Ok(r) => println!("[DM8→DM8] 数据导出 ✓  file={}", file_path_from_ok(r)),
        Err(e) => println!("[DM8→DM8] 数据导出失败 status={:?}", e.0),
    }
    assert!(result.is_ok(), "DM8 数据导出失败");
}

#[tokio::test]
#[ignore]
async fn dm8_07_export_ddl_to_mysql() {
    setup();
    let cfg = dm8_config();
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    assert!(!tables.is_empty(), "无表可导出");

    let names: Vec<String> = tables.iter().take(2).map(|t| t.name.clone()).collect();
    let mut req = base_export_req(cfg.clone());
    req.target_dialect = Some(DbType::Mysql);
    req.export_compat = None;
    req.tables = table_ids(&cfg.schema, names);

    let (state, _dir) = test_state();
    let result = export_ddl(State(state), Json(req)).await;
    match &result {
        Ok(r) => println!("[DM8→MySQL] DDL 导出 ✓  file={}", file_path_from_ok(r)),
        Err(e) => println!("[DM8→MySQL] DDL 导出失败 status={:?}", e.0),
    }
    assert!(result.is_ok(), "DM8→MySQL DDL 导出失败");
}

#[tokio::test]
#[ignore]
async fn dm8_08_export_ddl_to_kingbase() {
    setup();
    let cfg = dm8_config();
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    assert!(!tables.is_empty(), "无表可导出");

    let names: Vec<String> = tables.iter().take(2).map(|t| t.name.clone()).collect();
    let mut req = base_export_req(cfg.clone());
    req.target_dialect = Some(DbType::Kingbase);
    req.tables = table_ids(&cfg.schema, names);

    let (state, _dir) = test_state();
    let result = export_ddl(State(state), Json(req)).await;
    match &result {
        Ok(r) => println!("[DM8→KingBase] DDL 导出 ✓  file={}", file_path_from_ok(r)),
        Err(e) => println!("[DM8→KingBase] DDL 导出失败 status={:?}", e.0),
    }
    assert!(result.is_ok(), "DM8→KingBase DDL 导出失败");
}

// ═══════════════════════════════════════════════════════════════════════════
//  MySQL
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn mysql_01_connection_test() {
    setup();
    let Some(cfg) = mysql_config() else {
        println!("[MySQL] MYSQL_HOST 未设置，跳过");
        return;
    };
    let result = service::test_connection(&cfg).await;
    assert!(result.is_ok(), "MySQL 连接失败: {:?}", result.err());
    println!("[MySQL] 连接测试 ✓");
}

#[tokio::test]
#[ignore]
async fn mysql_02_list_schemas() {
    setup();
    let Some(cfg) = mysql_config() else {
        println!("[MySQL] 跳过");
        return;
    };
    let schemas = service::list_schemas(&cfg)
        .await
        .expect("list_schemas 失败");
    println!(
        "[MySQL] {} 个 schema: {:?}",
        schemas.len(),
        &schemas[..schemas.len().min(8)]
    );
    assert!(!schemas.is_empty());
}

#[tokio::test]
#[ignore]
async fn mysql_03_list_tables() {
    setup();
    let Some(cfg) = mysql_config() else {
        println!("[MySQL] 跳过");
        return;
    };
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    println!("[MySQL] schema={} → {} 张表", cfg.schema, tables.len());
    for t in tables.iter().take(5) {
        println!("  {} (comment={:?})", t.name, t.comment);
    }
}

#[tokio::test]
#[ignore]
async fn mysql_04_table_details() {
    setup();
    let Some(cfg) = mysql_config() else {
        println!("[MySQL] 跳过");
        return;
    };
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    if tables.is_empty() {
        println!("[MySQL] 无表，跳过");
        return;
    }
    let name = &tables[0].name;
    let d = service::get_table_details(&cfg, &cfg.schema, name)
        .await
        .expect("get_table_details 失败");
    println!(
        "[MySQL] {} → {} 列 / {} PK / {} 索引",
        name,
        d.columns.len(),
        d.primary_keys.len(),
        d.indexes.len()
    );
    assert!(!d.columns.is_empty());
}

#[tokio::test]
#[ignore]
async fn mysql_05_export_ddl_self() {
    setup();
    let Some(cfg) = mysql_config() else {
        println!("[MySQL] 跳过");
        return;
    };
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    if tables.is_empty() {
        println!("[MySQL→MySQL] 无表，跳过");
        return;
    }

    let names: Vec<String> = tables.iter().take(2).map(|t| t.name.clone()).collect();
    let mut req = base_export_req(cfg.clone());
    req.target_dialect = Some(DbType::Mysql);
    req.export_compat = None;
    req.tables = table_ids(&cfg.schema, names);

    let (state, _dir) = test_state();
    let result = export_ddl(State(state), Json(req)).await;
    match &result {
        Ok(r) => println!("[MySQL→MySQL] DDL 导出 ✓  file={}", file_path_from_ok(r)),
        Err(e) => println!("[MySQL→MySQL] DDL 导出失败 status={:?}", e.0),
    }
    assert!(result.is_ok(), "MySQL→MySQL DDL 导出失败");
}

#[tokio::test]
#[ignore]
async fn mysql_06_export_ddl_to_dm8() {
    setup();
    let Some(cfg) = mysql_config() else {
        println!("[MySQL→DM8] 跳过");
        return;
    };
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    if tables.is_empty() {
        println!("[MySQL→DM8] 无表，跳过");
        return;
    }

    let names: Vec<String> = tables.iter().take(2).map(|t| t.name.clone()).collect();
    let mut req = base_export_req(cfg.clone());
    req.target_dialect = Some(DbType::Dm8);
    req.tables = table_ids(&cfg.schema, names);

    let (state, _dir) = test_state();
    let result = export_ddl(State(state), Json(req)).await;
    match &result {
        Ok(r) => println!("[MySQL→DM8] DDL 导出 ✓  file={}", file_path_from_ok(r)),
        Err(e) => println!("[MySQL→DM8] DDL 导出失败 status={:?}", e.0),
    }
    assert!(result.is_ok(), "MySQL→DM8 DDL 导出失败");
}

// ═══════════════════════════════════════════════════════════════════════════
//  KingBase
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn kingbase_01_connection_test() {
    setup();
    let Some(cfg) = kingbase_config() else {
        println!("[KingBase] KB_HOST 未设置，跳过");
        return;
    };
    let result = service::test_connection(&cfg).await;
    assert!(result.is_ok(), "KingBase 连接失败: {:?}", result.err());
    println!("[KingBase] 连接测试 ✓");
}

#[tokio::test]
#[ignore]
async fn kingbase_02_list_tables() {
    setup();
    let Some(cfg) = kingbase_config() else {
        println!("[KingBase] 跳过");
        return;
    };
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    println!("[KingBase] schema={} → {} 张表", cfg.schema, tables.len());
    for t in tables.iter().take(5) {
        println!("  {}", t.name);
    }
}

#[tokio::test]
#[ignore]
async fn kingbase_03_table_details() {
    setup();
    let Some(cfg) = kingbase_config() else {
        println!("[KingBase] 跳过");
        return;
    };
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    if tables.is_empty() {
        println!("[KingBase] 无表，跳过");
        return;
    }
    let name = &tables[0].name;
    let d = service::get_table_details(&cfg, &cfg.schema, name)
        .await
        .expect("get_table_details 失败");
    println!(
        "[KingBase] {} → {} 列 / {} PK",
        name,
        d.columns.len(),
        d.primary_keys.len()
    );
    assert!(!d.columns.is_empty());
}

#[tokio::test]
#[ignore]
async fn kingbase_04_export_ddl_self() {
    setup();
    let Some(cfg) = kingbase_config() else {
        println!("[KingBase] 跳过");
        return;
    };
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    if tables.is_empty() {
        println!("[KingBase→KingBase] 无表，跳过");
        return;
    }

    let names: Vec<String> = tables.iter().take(2).map(|t| t.name.clone()).collect();
    let mut req = base_export_req(cfg.clone());
    req.target_dialect = Some(DbType::Kingbase);
    req.tables = table_ids(&cfg.schema, names);

    let (state, _dir) = test_state();
    let result = export_ddl(State(state), Json(req)).await;
    match &result {
        Ok(r) => println!(
            "[KingBase→KingBase] DDL 导出 ✓  file={}",
            file_path_from_ok(r)
        ),
        Err(e) => println!("[KingBase→KingBase] DDL 导出失败 status={:?}", e.0),
    }
    assert!(result.is_ok(), "KingBase 自身 DDL 导出失败");
}

#[tokio::test]
#[ignore]
async fn kingbase_05_export_ddl_to_mysql() {
    setup();
    let Some(cfg) = kingbase_config() else {
        println!("[KingBase→MySQL] 跳过");
        return;
    };
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    if tables.is_empty() {
        println!("[KingBase→MySQL] 无表，跳过");
        return;
    }

    let names: Vec<String> = tables.iter().take(2).map(|t| t.name.clone()).collect();
    let mut req = base_export_req(cfg.clone());
    req.target_dialect = Some(DbType::Mysql);
    req.export_compat = None;
    req.tables = table_ids(&cfg.schema, names);

    let (state, _dir) = test_state();
    let result = export_ddl(State(state), Json(req)).await;
    match &result {
        Ok(r) => println!("[KingBase→MySQL] DDL 导出 ✓  file={}", file_path_from_ok(r)),
        Err(e) => println!("[KingBase→MySQL] DDL 导出失败 status={:?}", e.0),
    }
    assert!(result.is_ok(), "KingBase→MySQL DDL 导出失败");
}

// ═══════════════════════════════════════════════════════════════════════════
//  神通 (Shentong)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn shentong_01_connection_test() {
    setup();
    let Some(cfg) = shentong_config() else {
        println!("[神通] ST_HOST 未设置，跳过");
        return;
    };
    let result = service::test_connection(&cfg).await;
    assert!(result.is_ok(), "神通连接失败: {:?}", result.err());
    println!("[神通] 连接测试 ✓");
}

#[tokio::test]
#[ignore]
async fn shentong_02_list_tables() {
    setup();
    let Some(cfg) = shentong_config() else {
        println!("[神通] 跳过");
        return;
    };
    let tables = service::list_tables(&cfg)
        .await
        .expect("神通 list_tables 失败");
    println!("[神通] schema={} → {} 张表", cfg.schema, tables.len());
    for t in tables.iter().take(5) {
        println!("  {}", t.name);
    }
}

#[tokio::test]
#[ignore]
async fn shentong_03_table_details() {
    setup();
    let Some(cfg) = shentong_config() else {
        println!("[神通] 跳过");
        return;
    };
    let tables = service::list_tables(&cfg).await.expect("list_tables 失败");
    if tables.is_empty() {
        println!("[神通] 无表，跳过");
        return;
    }
    let name = &tables[0].name;
    let d = service::get_table_details(&cfg, &cfg.schema, name)
        .await
        .expect("get_table_details 失败");
    println!(
        "[神通] {} → {} 列 / {} PK",
        name,
        d.columns.len(),
        d.primary_keys.len()
    );
    assert!(!d.columns.is_empty());
}
