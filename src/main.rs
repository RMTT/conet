use clap::Parser;
use conet::connection::{config::ConnectionConfig, device::Device};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process;

#[derive(Parser, Debug)]
#[command(version, about = "An overlay network with customization")]
struct Args {
    #[arg(short, long)]
    config: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    connection: ConnectionConfig,
}

#[tokio::main]
async fn main() {
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

    // Parse configuration
    let app_config: AppConfig = match toml::from_str(&config_content) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to parse config file: {}", e);
            process::exit(1);
        }
    };

    println!("{:?}", app_config);
    // Validate configuration
    if let Err(e) = app_config.connection.validate() {
        eprintln!("Configuration error: {}", e);
        process::exit(1);
    }

    // Create and run device
    match Device::new(app_config.connection).await {
        Ok(device) => {
            if let Err(e) = device.run().await {
                eprintln!("Device error: {}", e);
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Failed to create device: {}", e);
            process::exit(1);
        }
    }
}
