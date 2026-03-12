use anyhow::Result;

use crate::{
    export::{mysql_poc, orchestrator::LegacyExportPlan},
    models::{ConnectionConfig, DbType},
};

pub async fn export_mysql_to_dm8_ddl(
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
) -> Result<()> {
    mysql_poc::export_mysql_to_target_ddl(config, plan, DbType::Dm8).await
}

pub async fn export_mysql_to_dm8_data(
    config: &ConnectionConfig,
    plan: &LegacyExportPlan,
    batch_size: usize,
) -> Result<usize> {
    mysql_poc::export_mysql_to_target_data(config, plan, DbType::Dm8, batch_size).await
}
