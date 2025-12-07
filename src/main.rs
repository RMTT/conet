use clap::Parser;
use conet::connection::ConnectHandle;
use conet::connection::config::RegistryConfig;
use conet::connection::{config::ConnectionConfig, device::Device};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process;
use std::{env, fs};
use tokio_util::sync::CancellationToken;

#[derive(Parser, Debug)]
#[command(version, about = "An overlay network with customization")]
struct Args {
    /// path to configuration file
    #[arg(short, long)]
    config: PathBuf,
    /// path to registry file
    #[arg(short, long)]
    registry: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    connection: ConnectionConfig,
}

#[tokio::main(worker_threads = 4)]
async fn main() {
    if env::var("RUST_LOG").is_err() {
        unsafe {
            env::set_var("RUST_LOG", "info");
        }
    }
    env_logger::init();

    let args = Args::parse();

    // Read configuration file
    let config_content = match fs::read_to_string(&args.config) {
        Ok(content) => content,
        Err(e) => {
            eprintln!(
                "Failed to read config file {}: {}",
                args.config.display(),
                e
            );
            process::exit(1);
        }
    };

    let registry_content = match fs::read_to_string(&args.registry) {
        Ok(content) => content,
        Err(e) => {
            eprintln!(
                "Failed to read registry file {}: {}",
                args.config.display(),
                e
            );
            process::exit(1);
        }
    };

    // Parse configuration
    let app_config: AppConfig = match toml::from_str(&config_content) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to parse config file: {}", e);
            process::exit(1);
        }
    };
    let registry_config: RegistryConfig = match toml::from_str(&registry_content) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to parse config file: {}", e);
            process::exit(1);
        }
    };

    // Validate configuration
    if let Err(e) = app_config.connection.validate() {
        eprintln!("Configuration error: {}", e);
        process::exit(1);
    }
    // Validate registry
    if let Err(e) = registry_config.validate() {
        eprintln!("Registry error: {}", e);
        process::exit(1);
    }

    let cancel_token = CancellationToken::new();
    // Create and run device
    match ConnectHandle::new(app_config.connection, cancel_token.clone()).await {
        Ok(handle) => {
            if let Err(e) = handle.update_registry(registry_config).await {
                log::error!("Device error: {}", e);
                process::exit(1);
            }

            if let Err(e) = handle.event_loop().await {
                log::error!("Device error: {}", e);
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Failed to create device: {}", e);
            process::exit(1);
        }
    }
}
