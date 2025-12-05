use std::{cell::RefCell, sync::Arc};

use config::{ConnectionConfig, RegistryConfig};
use device::{Device, Message, MessageChannel, MessageType};
use tokio_util::sync::CancellationToken;

use crate::errors::ConetResult;

pub mod config;
pub mod device;
pub mod tun;
pub mod udp;

pub struct ConnectHandle {
    device: Arc<Device>,
}

impl ConnectHandle {
    pub async fn new(
        config: ConnectionConfig,
        cancel_token: CancellationToken,
    ) -> ConetResult<Self> {
        let device = Arc::new(Device::new(config, cancel_token).await?);

        Ok(Self { device })
    }

    async fn worker(device: Arc<Device>) -> ConetResult<()> {
        log::debug!("new worker started");

        loop {
            let message = device.message_channel.receiver.recv().await?;
            match message.t {
                MessageType::Stop => return Ok(()),
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
            tokio::spawn(Self::worker(self.device.clone()));
        }

        self.device.event_loop().await
    }

    pub async fn update_registry(&self, registry: RegistryConfig) -> ConetResult<()> {
        self.device.update_registry(registry).await
    }
}
