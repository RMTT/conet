use crate::errors::ConetResult;
use crate::{connection::config::ConnectionConfig, errors::Error};
use async_channel::{Receiver, Sender};
use base64::{prelude::BASE64_STANDARD, Engine};
use boringtun::x25519::{PublicKey, StaticSecret};
use bytes::BytesMut;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use std::usize;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use super::tun::TunDevice;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub public_key: String,
    pub endpoint: Option<SocketAddr>,
    pub allowed_ips: Vec<String>,
}

pub struct Device {
    config: ConnectionConfig,
    tun: TunDevice,
    udp4: Option<tokio::net::UdpSocket>,
    udp6: tokio::net::UdpSocket,
    private_key: StaticSecret,
    worker_channel: WorkerChannel,
    // peers: Arc<RwLock<HashMap<PublicKey, PeerInfo>>>,
}

struct WorkerChannel {
    sender: Sender<Message>,
    receiver: Receiver<Message>,
}
enum MessageType {
    Stop,
    FromTun,
    FromUdp,
}
// Message for workers
struct Message {
    t: MessageType,
    data: BytesMut,
}

impl Device {
    pub async fn new(config: ConnectionConfig) -> ConetResult<Self> {
        // parse private_key, the validate of configuration shoule guarantee private_key can be
        // decoded
        let key_result: Result<[u8; 32], Vec<u8>> =
            BASE64_STANDARD.decode(&config.private_key)?.try_into();
        let key = key_result.map_err(|_| Error::Err("cannot parse private_key".to_string()))?;

        // create tun device
        let tun = TunDevice::new(config.interface.as_str())?;
        tun.set_mtu(config.mtu)?;
        for addr in &config.address {
            tun.add_address(addr)?;
        }

        // create udp sockets
        #[cfg(not(target_os = "linux"))]
        let udp4 =
            Some(tokio::net::UdpSocket::bind(format!("0.0.0.0:{}", config.listen_port)).await?);

        // in Linux, bind [::] will bind ipv4 as well
        #[cfg(target_os = "linux")]
        let udp4 = None;

        let udp6 = tokio::net::UdpSocket::bind(format!("[::]:{}", config.listen_port)).await?;

        // create channels for passing packets
        let (sender, receiver) = async_channel::bounded(2048);
        let worker_channel = WorkerChannel { sender, receiver };

        Ok(Self {
            config,
            private_key: StaticSecret::from(key),
            worker_channel,
            tun,
            udp4,
            udp6,
            // peers: todo!(),
        })
    }

    async fn worker(receiver: Receiver<Message>) -> ConetResult<()> {
        loop {
            let message = receiver.recv().await?;
            match message.t {
                MessageType::Stop => return Ok(()),
                MessageType::FromTun => {
                    if let Err(e) = Self::handle_tun_packets().await {
                        log::warn!("handle tun packet failed: {e}");
                    }
                }
                MessageType::FromUdp => {
                    if let Err(e) = Self::handle_udp_packets().await {
                        log::warn!("handle udp packet failed: {e}");
                    }
                }
            }
        }
    }

    async fn handle_tun_packets() -> ConetResult<()> {
        Ok(())
    }

    async fn handle_udp_packets() -> ConetResult<()> {
        Ok(())
    }

    pub async fn run(&self) -> ConetResult<()> {
        log::info!("Starting conet device {}", self.config.interface);

        // launch workers
        for _ in 0..num_cpus::get() {
            let receiver = self.worker_channel.receiver.clone();
            tokio::spawn(Self::worker(receiver));
        }

        let buf = RefCell::new(BytesMut::with_capacity(65527));
        let buf_ref = buf.borrow();
        loop {
            tokio::select! {
                r = async {self.tun.recv(&mut buf.borrow_mut()).await}=> {
                    match r {
                        Err(e) => log::warn!("failed to receive data from tun device: {e}"),
                        Ok(n) => {
                            let mut b = BytesMut::new();
                            b.extend_from_slice(&buf_ref[..n]);
                            let r = self.worker_channel.sender.send(Message{
                                t: MessageType::FromTun,
                                data: b
                            }).await;

                            if let Err(e) = r  {
                                log::warn!("send message to worker failed: {e}");
                            }
                        }
                    }
                },
                r = async {self.udp6.recv(&mut buf.borrow_mut()).await} => {
                    match r {
                        Err(e) => log::warn!("failed to receive data from tun device: {e}"),
                        Ok(n) => {
                            let mut b = BytesMut::new();
                            b.extend_from_slice(&buf_ref[..n]);
                            let r = self.worker_channel.sender.send(Message{
                                t: MessageType::FromUdp,
                                data: b
                            }).await;

                            if let Err(e) = r  {
                                log::warn!("send message to worker failed: {e}");
                            }
                        }
                    }
                },
                r = async {self.udp4.as_ref().unwrap().recv(&mut buf.borrow_mut()).await}, if &self.udp4.is_some() => {
                    match r {
                        Err(e) => log::warn!("failed to receive data from tun device: {e}"),
                        Ok(n) => {
                            let mut b = BytesMut::new();
                            b.extend_from_slice(&buf_ref[..n]);
                            let r = self.worker_channel.sender.send(Message{
                                t: MessageType::FromUdp,
                                data: b
                            }).await;

                            if let Err(e) = r  {
                                log::warn!("send message to worker failed: {e}");
                            }
                        }
                    }
                }
            }
        }
    }
}
