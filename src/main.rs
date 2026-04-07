use anyhow::Result;
use clap::{Parser, Subcommand};
use cloudid::cache::AppState;
use cloudid::config::Config;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, error, info};

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

            let state = AppState::new(config.clone()).await;

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

            // Spawn container watcher (namespace ownership -> SSH keys)
            let container_state = Arc::clone(&state);
            tokio::spawn(async move {
                cloudid::watcher::container::start(container_state).await;
            });

            // Spawn metadata route manager (discovers data networks from mkube,
            // ensures DHCP option 121 routes 169.254.169.254 via gateway)
            let mkube_url = config.mkube.url.clone();
            tokio::spawn(async move {
                cloudid::metadata_route::start(mkube_url).await;
            });

            // Spawn RouterOS NAT manager (ensures IMDS DST-NAT rule exists)
            if let Some(ros_config) = config.routeros.clone() {
                let listen_addr: SocketAddr = config.server.metadata_addr.parse()
                    .expect("invalid metadata_addr");
                let port = listen_addr.port().to_string();
                tokio::spawn(async move {
                    let cloudid_ip = match &ros_config.to_address {
                        Some(ip) => ip.clone(),
                        None => discover_local_ip(&ros_config.rest_url),
                    };
                    info!(cloudid_ip = %cloudid_ip, cloudid_port = %port, "RouterOS NAT manager starting");
                    cloudid::routeros_nat::start(ros_config, cloudid_ip, port).await;
                });
            } else {
                debug!("no [routeros] config, skipping NAT management");
            }

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

/// Discover this host's IP by parsing the router host from the REST URL
/// and opening a UDP socket toward it. The OS picks the correct source IP.
fn discover_local_ip(rest_url: &str) -> String {
    // Parse host:port from the URL (e.g. "http://192.168.200.1/rest" -> "192.168.200.1:80")
    let url: url::Url = rest_url.parse().expect("invalid routeros.rest_url");
    let host = url.host_str().expect("routeros.rest_url has no host");
    let port = url.port_or_known_default().unwrap_or(80);
    let target = format!("{}:{}", host, port);

    let sock = std::net::UdpSocket::bind("0.0.0.0:0").expect("failed to bind UDP socket");
    sock.connect(&target).expect("failed to connect UDP socket");
    let local = sock.local_addr().expect("failed to get local addr");
    local.ip().to_string()
}
