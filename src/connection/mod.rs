use std::collections::HashMap;
use std::sync::Arc;

use config::{ConnectionConfig, PeerConfig, RegistryConfig};
use device::{Device, MessageType};
use tokio_util::sync::CancellationToken;

use crate::errors::ConetResult;

pub mod config;
pub mod device;
pub mod peer;
pub mod tun;
pub mod udp;

pub struct ConnectHandle {
    device: Arc<Device>,
    registry: Arc<tokio::sync::RwLock<HashMap<String, PeerConfig>>>,
    cancel_token: CancellationToken,
}

impl ConnectHandle {
    pub async fn new(
        config: ConnectionConfig,
        cancel_token: CancellationToken,
    ) -> ConetResult<Self> {
        let device = Arc::new(Device::new(config, cancel_token.clone()).await?);
        let registry = Arc::new(tokio::sync::RwLock::new(HashMap::new()));

        Ok(Self {
            device,
            registry,
            cancel_token,
        })
    }

    async fn worker(device: Arc<Device>, cancel_token: CancellationToken) -> ConetResult<()> {
        log::debug!("new worker started");

        loop {
            let message = tokio::select! {
                m = device.message_channel.receiver.recv() => {
                    if let Err(e) = m {
                        log::debug!("workers failed to receive message from device: {e}");
                        return Ok(());
                    }
                    m?
                },
                _ = cancel_token.cancelled() => {
                    return Ok(());
                }
            };

            match message.t {
                MessageType::FromTun => {
                    if let Err(e) = device.handle_tun_packets(message.data).await {
                        log::warn!("handle tun packet failed: {e}");
                    }
                }
                MessageType::FromUdp => {
                    let src_addr = match message.src_addr {
                        Some(a) => a,
                        None => {
                            log::warn!("receive udp packet but not source address");
                            continue;
                        }
                    };
                    if let Err(e) = device.handle_udp_packets(src_addr, message.data).await {
                        log::warn!("handle udp packet failed: {e}");
                    }
                }
            }
        }
    }

    pub async fn event_loop(&self) -> ConetResult<()> {
        log::info!("Starting event loop of device");

        // launch workers
        for _ in 0..num_cpus::get() {
            tokio::spawn(Self::worker(self.device.clone(), self.cancel_token.clone()));
        }

        let (r1, r2) = tokio::join!(self.device.event_loop(), self.device.timer_loop());

        r1?;
        r2?;
        Ok(())
    }

    pub async fn update_registry(&self, registry: RegistryConfig) -> ConetResult<()> {
        // Store the registry
        let mut registry_map = self.registry.write().await;

        // Remove peers that are not in the new registry
        let mut netids_to_remove = Vec::new();
        for (netid, _) in registry_map.iter() {
            if !registry.peers.iter().any(|p| &p.netid == netid) {
                netids_to_remove.push(netid.clone());
            }
        }

        // Remove old peers from device
        for netid in netids_to_remove {
            registry_map.remove(&netid);
            let removed_count = self.device.remove_peers_by_netid(&netid).await?;
            log::info!("Removed {} peers for netid: {}", removed_count, netid);
        }

        // Add or update peers
        for peer_config in registry.peers {
            for node in &peer_config.nodes {
                // Add peer to device
                self.device.add_peer(node, &peer_config.netid).await?;
            }

            // Store in registry
            registry_map.insert(peer_config.netid.clone(), peer_config);
        }

        Ok(())
    }
}
