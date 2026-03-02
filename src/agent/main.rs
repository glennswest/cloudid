use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cloudid-agent", about = "CloudId host agent for SSH key management")]
struct Cli {
    /// Metadata endpoint URL
    #[arg(
        long,
        env = "CLOUDID_METADATA_URL",
        default_value = "http://169.254.169.254"
    )]
    metadata_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch and install SSH keys for all users from metadata endpoint
    Refresh,

    /// Query authorized keys for a specific user (for sshd AuthorizedKeysCommand)
    AuthorizedKeys {
        /// System username to query keys for
        username: String,
    },

    /// Show metadata that would be served for this host
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    match cli.command {
        Commands::Refresh => refresh(&client, &cli.metadata_url).await?,
        Commands::AuthorizedKeys { username } => {
            authorized_keys(&client, &cli.metadata_url, &username).await?
        }
        Commands::Status => status(&client, &cli.metadata_url).await?,
    }

    Ok(())
}

async fn refresh(client: &reqwest::Client, base_url: &str) -> Result<()> {
    // Get the public keys index
    let index_url = format!("{}/latest/meta-data/public-keys/", base_url);
    let resp = client.get(&index_url).send().await?;

    if !resp.status().is_success() {
        eprintln!("metadata endpoint returned {}", resp.status());
        return Ok(());
    }

    let body = resp.text().await?;

    for line in body.lines() {
        if line.is_empty() {
            continue;
        }

        // Format: "0=root"
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() != 2 {
            continue;
        }

        let index = parts[0];
        let ssh_user = parts[1];

        // Fetch the keys for this index
        let key_url = format!(
            "{}/latest/meta-data/public-keys/{}/openssh-key",
            base_url, index
        );
        let key_resp = client.get(&key_url).send().await?;

        if !key_resp.status().is_success() {
            eprintln!("failed to fetch keys for {}: {}", ssh_user, key_resp.status());
            continue;
        }

        let keys = key_resp.text().await?;

        if keys.trim().is_empty() {
            continue;
        }

        // Write keys to authorized_keys.d/cloudid
        let home = if ssh_user == "root" {
            PathBuf::from("/root")
        } else {
            PathBuf::from(format!("/home/{}", ssh_user))
        };

        let ssh_dir = home.join(".ssh");
        let keys_dir = ssh_dir.join("authorized_keys.d");
        let keys_file = keys_dir.join("cloudid");

        // Create directories if they don't exist
        if let Err(e) = std::fs::create_dir_all(&keys_dir) {
            eprintln!("failed to create {}: {}", keys_dir.display(), e);
            continue;
        }

        // Write keys atomically (write to temp, rename)
        let tmp_file = keys_dir.join(".cloudid.tmp");
        match std::fs::File::create(&tmp_file) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(keys.as_bytes()) {
                    eprintln!("failed to write keys for {}: {}", ssh_user, e);
                    let _ = std::fs::remove_file(&tmp_file);
                    continue;
                }
                // Set permissions to 0600
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &tmp_file,
                        std::fs::Permissions::from_mode(0o600),
                    );
                }
                if let Err(e) = std::fs::rename(&tmp_file, &keys_file) {
                    eprintln!("failed to install keys for {}: {}", ssh_user, e);
                    let _ = std::fs::remove_file(&tmp_file);
                    continue;
                }
                println!("updated keys for {} ({} keys)", ssh_user, keys.lines().count());
            }
            Err(e) => {
                eprintln!("failed to create temp file for {}: {}", ssh_user, e);
            }
        }
    }

    Ok(())
}

async fn authorized_keys(client: &reqwest::Client, base_url: &str, username: &str) -> Result<()> {
    // Get the public keys index to find which index maps to this username
    let index_url = format!("{}/latest/meta-data/public-keys/", base_url);
    let resp = client.get(&index_url).send().await?;

    if !resp.status().is_success() {
        // Self-healing: if endpoint unreachable, try local cached keys
        try_local_keys(username);
        return Ok(());
    }

    let body = resp.text().await?;

    for line in body.lines() {
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() != 2 {
            continue;
        }

        let index = parts[0];
        let ssh_user = parts[1];

        if ssh_user != username {
            continue;
        }

        let key_url = format!(
            "{}/latest/meta-data/public-keys/{}/openssh-key",
            base_url, index
        );
        let key_resp = client.get(&key_url).send().await?;

        if key_resp.status().is_success() {
            let keys = key_resp.text().await?;
            print!("{}", keys);
        }

        return Ok(());
    }

    // Username not found in metadata, try local cache
    try_local_keys(username);
    Ok(())
}

fn try_local_keys(username: &str) {
    // Self-healing: if metadata endpoint is unreachable, serve from local cache.
    // Never delete working keys.
    let home = if username == "root" {
        PathBuf::from("/root")
    } else {
        PathBuf::from(format!("/home/{}", username))
    };

    let keys_file = home.join(".ssh/authorized_keys.d/cloudid");
    if let Ok(keys) = std::fs::read_to_string(&keys_file) {
        print!("{}", keys);
    }
}

async fn status(client: &reqwest::Client, base_url: &str) -> Result<()> {
    let endpoints = [
        ("instance-id", "/latest/meta-data/instance-id"),
        ("hostname", "/latest/meta-data/hostname"),
        ("local-hostname", "/latest/meta-data/local-hostname"),
        ("local-ipv4", "/latest/meta-data/local-ipv4"),
        (
            "availability-zone",
            "/latest/meta-data/placement/availability-zone",
        ),
        ("public-keys", "/latest/meta-data/public-keys/"),
    ];

    for (label, path) in &endpoints {
        let url = format!("{}{}", base_url, path);
        match client.get(&url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    let body = resp.text().await?;
                    for line in body.lines() {
                        println!("{}: {}", label, line);
                    }
                } else {
                    println!("{}: (HTTP {})", label, resp.status());
                }
            }
            Err(e) => {
                println!("{}: (error: {})", label, e);
            }
        }
    }

    Ok(())
}
