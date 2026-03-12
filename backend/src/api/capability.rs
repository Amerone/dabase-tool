use axum::extract::Query;
use serde::Deserialize;

use crate::{
    api::response::{self, ApiResult},
    export::capability::build_runtime_export_capability_report,
    models::{DbType, ExportCapabilityReport},
};

#[derive(Debug, Deserialize)]
pub struct CapabilityQuery {
    #[serde(default)]
    pub source_db_type: DbType,
    pub target_dialect: Option<DbType>,
}

pub async fn get_export_capabilities(
    Query(query): Query<CapabilityQuery>,
) -> ApiResult<ExportCapabilityReport> {
    let source_db_type = query.source_db_type;
    let target_dialect = query
        .target_dialect
        .unwrap_or_else(|| source_db_type.clone());

    let report = build_runtime_export_capability_report(source_db_type, target_dialect);

    response::ok(report)
}
