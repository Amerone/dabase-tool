use crate::{
    db::pool::ConnectionPool,
    export::{
        ddl::TriggerTerminator,
        dm8_to_kingbase_poc, dm8_to_mysql_poc, dm8_to_shentong_poc, kingbase_poc,
        kingbase_to_other_poc, mysql_poc, mysql_to_dm8_poc, mysql_to_kingbase_poc,
        orchestrator::{ExportExecutionPath, LegacyExportOrchestrator, LegacyExportPlan},
        shentong_poc,
    },
    models::ConnectionConfig,
};

fn run_with_odbc_connection<T, F>(config: &ConnectionConfig, task: F) -> anyhow::Result<T>
where
    F: FnOnce(&odbc_api::Connection<'_>) -> anyhow::Result<T>,
{
    let pool = ConnectionPool::new(config.clone())
        .map_err(|e| anyhow::anyhow!("Failed to create connection: {}", e))?;
    let connection = pool
        .get_connection()
        .map_err(|e| anyhow::anyhow!("Failed to get connection: {}", e))?;
    task(&connection)
}

async fn run_blocking_export_task<T, F>(task_name: &'static str, task: F) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|e| anyhow::anyhow!("{} failed to join: {}", task_name, e))?
}

pub(super) async fn execute_ddl_by_path(
    execution_path: ExportExecutionPath,
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
    drop_existing: bool,
    trigger_terminator: TriggerTerminator,
    identifier_case: Option<String>,
) -> anyhow::Result<()> {
    // Warn if identifier_case is provided but the path doesn't support it
    if identifier_case.is_some() {
        match execution_path {
            ExportExecutionPath::Dm8ToKingbasePoc
            | ExportExecutionPath::Dm8ToShentongPoc
            | ExportExecutionPath::Dm8ToMysqlPoc => {}
            _ => {
                tracing::warn!(
                    "identifier_case is set but not supported for DDL path {:?}; ignoring",
                    execution_path
                );
            }
        }
    }

    match execution_path {
        ExportExecutionPath::LegacyDm8 => {
            let config = config.clone();
            let plan = plan.clone();
            run_blocking_export_task("Legacy DM8 DDL export task", move || {
                run_with_odbc_connection(&config, |conn| {
                    LegacyExportOrchestrator::export_ddl(
                        conn,
                        &plan,
                        drop_existing,
                        trigger_terminator,
                    )
                })
            })
            .await
        }
        ExportExecutionPath::Dm8ToMysqlPoc => {
            let config = config.clone();
            let plan = plan.clone();
            let id_case = identifier_case.unwrap_or_else(|| "preserve".to_string());
            run_blocking_export_task("DM8 -> MySQL DDL export task", move || {
                run_with_odbc_connection(&config, |conn| {
                    dm8_to_mysql_poc::export_dm8_to_mysql_ddl(conn, &plan, &id_case)
                })
            })
            .await
        }
        ExportExecutionPath::Dm8ToKingbasePoc => {
            let config = config.clone();
            let plan = plan.clone();
            let id_case = identifier_case.unwrap_or_else(|| "preserve".to_string());
            run_blocking_export_task("DM8 -> KingBase DDL export task", move || {
                run_with_odbc_connection(&config, |conn| {
                    dm8_to_kingbase_poc::export_dm8_to_kingbase_ddl(conn, &plan, &id_case)
                })
            })
            .await
        }
        ExportExecutionPath::KingbasePoc => {
            kingbase_poc::export_kingbase_to_kingbase_ddl(config, plan).await
        }
        ExportExecutionPath::KingbaseToMysqlPoc => {
            kingbase_to_other_poc::export_kingbase_to_mysql_ddl(config, plan).await
        }
        ExportExecutionPath::KingbaseToDm8Poc => {
            kingbase_to_other_poc::export_kingbase_to_dm8_ddl(config, plan).await
        }
        ExportExecutionPath::MysqlPoc => mysql_poc::export_mysql_to_mysql_ddl(config, plan).await,
        ExportExecutionPath::MysqlToDm8Poc => {
            mysql_to_dm8_poc::export_mysql_to_dm8_ddl(config, plan).await
        }
        ExportExecutionPath::MysqlToKingbasePoc => {
            mysql_to_kingbase_poc::export_mysql_to_kingbase_ddl(config, plan).await
        }
        ExportExecutionPath::ShentongPoc => {
            let config = config.clone();
            let plan = plan.clone();
            run_blocking_export_task("Shentong -> Shentong DDL export task", move || {
                shentong_poc::export_shentong_to_shentong_ddl(&config, &plan)
            })
            .await
        }
        ExportExecutionPath::Dm8ToShentongPoc => {
            let config = config.clone();
            let plan = plan.clone();
            let id_case = identifier_case.unwrap_or_else(|| "preserve".to_string());
            run_blocking_export_task("DM8 -> Shentong DDL export task", move || {
                run_with_odbc_connection(&config, |conn| {
                    dm8_to_shentong_poc::export_dm8_to_shentong_ddl(conn, &plan, &id_case)
                })
            })
            .await
        }
    }
}

pub(super) async fn execute_data_by_path(
    execution_path: ExportExecutionPath,
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
    batch_size: usize,
    include_row_counts: bool,
    compat: crate::export::ddl::TriggerTerminator,
    identifier_case: Option<String>,
) -> anyhow::Result<usize> {
    // Warn if identifier_case is provided but the path doesn't support it
    if identifier_case.is_some() {
        match execution_path {
            ExportExecutionPath::Dm8ToKingbasePoc
            | ExportExecutionPath::Dm8ToShentongPoc
            | ExportExecutionPath::Dm8ToMysqlPoc => {}
            _ => {
                tracing::warn!(
                    "identifier_case is set but not supported for data path {:?}; ignoring",
                    execution_path
                );
            }
        }
    }

    match execution_path {
        ExportExecutionPath::LegacyDm8 => {
            let config = config.clone();
            let plan = plan.clone();
            run_blocking_export_task("Legacy DM8 data export task", move || {
                run_with_odbc_connection(&config, |conn| {
                    LegacyExportOrchestrator::export_data(
                        conn,
                        &plan,
                        batch_size,
                        include_row_counts,
                        compat,
                    )
                })
            })
            .await
        }
        ExportExecutionPath::Dm8ToMysqlPoc => {
            let config = config.clone();
            let plan = plan.clone();
            let id_case = identifier_case.unwrap_or_else(|| "preserve".to_string());
            run_blocking_export_task("DM8 -> MySQL data export task", move || {
                run_with_odbc_connection(&config, |conn| {
                    dm8_to_mysql_poc::export_dm8_to_mysql_data(
                        conn, &config, &plan, batch_size, &id_case,
                    )
                })
            })
            .await
        }
        ExportExecutionPath::Dm8ToKingbasePoc => {
            let config = config.clone();
            let plan = plan.clone();
            let id_case = identifier_case.unwrap_or_else(|| "preserve".to_string());
            run_blocking_export_task("DM8 -> KingBase data export task", move || {
                run_with_odbc_connection(&config, |conn| {
                    dm8_to_kingbase_poc::export_dm8_to_kingbase_data(
                        conn, &config, &plan, batch_size, &id_case,
                    )
                })
            })
            .await
        }
        ExportExecutionPath::KingbasePoc => {
            kingbase_poc::export_kingbase_to_kingbase_data(config, plan, batch_size).await
        }
        ExportExecutionPath::KingbaseToMysqlPoc => {
            kingbase_to_other_poc::export_kingbase_to_mysql_data(config, plan, batch_size).await
        }
        ExportExecutionPath::KingbaseToDm8Poc => {
            kingbase_to_other_poc::export_kingbase_to_dm8_data(config, plan, batch_size).await
        }
        ExportExecutionPath::MysqlPoc => {
            mysql_poc::export_mysql_to_mysql_data(config, plan, batch_size).await
        }
        ExportExecutionPath::MysqlToDm8Poc => {
            mysql_to_dm8_poc::export_mysql_to_dm8_data(config, plan, batch_size).await
        }
        ExportExecutionPath::MysqlToKingbasePoc => {
            mysql_to_kingbase_poc::export_mysql_to_kingbase_data(config, plan, batch_size).await
        }
        ExportExecutionPath::ShentongPoc => {
            let config = config.clone();
            let plan = plan.clone();
            run_blocking_export_task("Shentong -> Shentong data export task", move || {
                shentong_poc::export_shentong_to_shentong_data(&config, &plan, batch_size)
            })
            .await
        }
        ExportExecutionPath::Dm8ToShentongPoc => {
            let config = config.clone();
            let plan = plan.clone();
            let id_case = identifier_case.unwrap_or_else(|| "preserve".to_string());
            run_blocking_export_task("DM8 -> Shentong data export task", move || {
                run_with_odbc_connection(&config, |conn| {
                    dm8_to_shentong_poc::export_dm8_to_shentong_data(
                        conn, &config, &plan, batch_size, &id_case,
                    )
                })
            })
            .await
        }
    }
}
