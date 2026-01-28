mod db;
mod export;
mod api;
mod models;
mod config_store;

#[tokio::main]
async fn main() {
    dm8_export_backend::init_tracing();
    match dm8_export_backend::start_server(None).await {
        Ok(addr) => {
            tracing::info!("Starting server on {}", addr);
            // Hold the process open.
            futures::future::pending::<()>().await;
        }
        Err(err) => {
            eprintln!("Failed to start server: {err:?}");
            std::process::exit(1);
        }
    }
}
