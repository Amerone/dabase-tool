#[tokio::main]
async fn main() {
    dm8_export_backend::init_tracing();
    match dm8_export_backend::start_server(None).await {
        Ok(addr) => {
            tracing::info!("Server listening on {}", addr);
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("Received shutdown signal, exiting");
        }
        Err(err) => {
            eprintln!("Failed to start server: {err:?}");
            std::process::exit(1);
        }
    }
}
