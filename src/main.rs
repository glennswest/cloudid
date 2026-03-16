use anyhow::Result;
use clap::{Parser, Subcommand};
use cloudid::cache::AppState;
use cloudid::config::Config;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "cloudid", about = "Afterburn-compatible metadata service")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the metadata server
    Serve {
        /// Path to config file
        #[arg(short, long, default_value = "/etc/cloudid/config.toml")]
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cloudid=info,tower_http=info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { config: config_path } => {
            let config = Config::load(&config_path)?;
            info!(
                addr = %config.server.metadata_addr,
                nats = %config.amo.nats_url,
                mkube = %config.mkube.url,
                "starting cloudid"
            );

            let state = AppState::new(config.clone());

            // Spawn AMO NATS watcher
            let amo_state = Arc::clone(&state);
            tokio::spawn(async move {
                cloudid::watcher::amo::start(amo_state).await;
            });

            // Spawn BMH watcher
            let bmh_state = Arc::clone(&state);
            tokio::spawn(async move {
                cloudid::watcher::bmh::start(bmh_state).await;
            });

            // Spawn metadata route manager (discovers data networks from mkube,
            // ensures DHCP option 121 routes 169.254.169.254 via gateway)
            let mkube_url = config.mkube.url.clone();
            tokio::spawn(async move {
                cloudid::metadata_route::start(mkube_url).await;
            });

            // Start HTTP server
            let app = cloudid::metadata::router(Arc::clone(&state));

            let addr: SocketAddr = config.server.metadata_addr.parse()?;
            info!(%addr, "metadata endpoint listening");

            let listener = tokio::net::TcpListener::bind(addr).await?;
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .map_err(|e| {
                error!(error = %e, "server error");
                anyhow::anyhow!("server error: {}", e)
            })?;
        }
    }

    Ok(())
}
