use base64::{prelude::BASE64_STANDARD, Engine};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use crate::errors::{ConetResult, Error};

fn default_mtu() -> u16 {
    1420
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub netid: String,
    pub nodeid: String,
    pub interface: String,
    pub listen_port: u16,
    pub address: Vec<IpNet>,
    #[serde(default = "default_mtu")]
    pub mtu: u16,
    pub private_key: String,
    pub peers: Option<Vec<PeerInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub nodeid: String,
    pub public_key: String,
    pub endpoint: Option<String>,
    pub allowed_ips: Vec<IpNet>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConfig {
    pub netid: String,
    pub nodes: Vec<PeerInfo>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub peers: Vec<PeerConfig>,
}

impl ConnectionConfig {
    pub fn validate(&self) -> ConetResult<()> {
        // Validate private key
        if self.private_key.is_empty() {
            return Err(Error::Err("Private key cannot be empty".to_string()));
        }

        if let Ok(key) = BASE64_STANDARD.decode(&self.private_key) {
            if key.len() != 32 {
                return Err(Error::Err("length of private_key must be 32".to_string()));
            }
        } else {
            return Err(Error::Err(
                "Private key must be valid base64 string".to_string(),
            ));
        }

        // Validate listen port
        if self.listen_port == 0 {
            return Err(Error::Err("Listen port cannot be 0".to_string()));
        }

        Ok(())
    }
}

impl RegistryConfig {
    pub fn validate(&self) -> ConetResult<()> {
        // Validate peers
        for peer_config in &self.peers {
            for peer in &peer_config.nodes {
                if peer.public_key.is_empty() {
                    return Err(Error::Err(format!("Peer {:?} has empty public key", peer)));
                }

                if let Ok(key) = BASE64_STANDARD.decode(&peer.public_key) {
                    if key.len() != 32 {
                        return Err(Error::Err(format!(
                            "length of private_key must be 32 in Peer {:?}",
                            peer
                        )));
                    }
                } else {
                    return Err(Error::Err(format!(
                        "Peer {:?} has invalid public key base64",
                        peer
                    )));
                }
            }
        }

        Ok(())
    }
}
