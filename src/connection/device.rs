use crate::errors::ConetResult;
use crate::utils;
use crate::{connection::config::ConnectionConfig, errors::Error};
use async_channel::{Receiver, Sender};
use boringtun::noise::handshake::parse_handshake_anon;
use boringtun::noise::rate_limiter::RateLimiter;
use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use bytes::BytesMut;
use ipnet::IpNet;
use rand::RngCore;
use rand::rngs::OsRng;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::Mutex;
use std::usize;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use super::config::{PeerConfig, RegistryConfig};
use super::tun::TunDevice;
use super::udp::UdpSocket;

const MAX_UDP_SIZE: usize = 0xFFFF;
const HANDSHAKE_RATE_LIMIT: u64 = 100; // The number of handshakes per second we can tolerate before using cookies

pub struct Peer {
    pub tunn: Tunn,
    pub id: u32,
    pub endpoint: Option<SocketAddr>,
    pub allowed_ips: Vec<IpNet>,
}

/// combine registry configuration and actual peer info
pub struct PeerState {
    peers_by_ip: HashMap<IpNet, Arc<Peer>>,
    peers_by_idx: HashMap<u32, Arc<Peer>>,
    peers: HashMap<PublicKey, Arc<Peer>>,
    registry: HashMap<String, PeerConfig>,
}

pub struct Device {
    config: ConnectionConfig,
    tun: TunDevice,
    udp: UdpSocket,
    key_pair: (StaticSecret, PublicKey),
    pub message_channel: MessageChannel,
    peer_state: RwLock<PeerState>,
    /// currently index_generator only be used in update_registry with peer_state, so it's ok to
    /// use sync::Mutex
    index_generator: Mutex<IndexLfsr>,
    cancel_token: CancellationToken,
    rate_limiter: RateLimiter,
}

pub struct MessageChannel {
    pub sender: Sender<Message>,
    pub receiver: Receiver<Message>,
}

pub enum MessageType {
    Stop,
    FromTun,
    FromUdp,
}
// Message for workers
pub struct Message {
    pub t: MessageType,
    pub data: BytesMut,
    pub src_addr: Option<SocketAddr>,
}

impl Device {
    pub async fn new(
        config: ConnectionConfig,
        cancel_token: CancellationToken,
    ) -> ConetResult<Self> {
        // parse private_key, the validate of configuration shoule guarantee private_key can be
        // decoded
        let private_key = utils::base64_to_private_key(config.private_key.clone())?;
        let public_key = PublicKey::from(&private_key);

        // create tun device
        let tun = TunDevice::new(config.interface.as_str(), None)?;
        tun.set_mtu(config.mtu)?;
        for addr in &config.address {
            tun.add_address(addr)?;
        }

        // create udp sockets
        let udp = UdpSocket::new(config.listen_port).await?;

        let (sender, receiver) = async_channel::bounded(2048);
        let message_channel = MessageChannel { sender, receiver };

        let peers_by_ip = HashMap::new();
        let peers_by_idx = HashMap::new();
        let peers = HashMap::new();
        let registry = HashMap::new();
        let peer_state = RwLock::new(PeerState {
            peers_by_ip,
            peers_by_idx,
            peers,
            registry,
        });

        Ok(Self {
            config,
            key_pair: (private_key, public_key),
            message_channel,
            tun,
            udp,
            peer_state,
            cancel_token,
            index_generator: Mutex::new(Default::default()),
            rate_limiter: RateLimiter::new(&public_key, HANDSHAKE_RATE_LIMIT),
        })
    }

    pub async fn update_registry(&self, config: RegistryConfig) -> ConetResult<()> {
        let mut peer_state = self.peer_state.write().await;
        let mut idx_generator = self.index_generator.lock().unwrap();

        for peer in config.peers {
            for node in &peer.nodes {
                if &node.nodeid == &self.config.nodeid && &peer.netid == &self.config.netid {
                    continue;
                }

                let idx = idx_generator.next();
                let pubkey = utils::base64_to_public_key(node.public_key.clone())?;
                let tu = Tunn::new(
                    self.key_pair.0.clone(),
                    pubkey.clone(),
                    None,
                    None,
                    idx,
                    None,
                );

                let mut endpoint: Option<SocketAddr> = None;
                if let Some(end) = &node.endpoint {
                    endpoint = match end.to_socket_addrs() {
                        Ok(a) => a.last(),
                        Err(e) => {
                            log::warn!(
                                "endpoint of node {} in net {} cannot be resolved: {e}, skipped",
                                node.nodeid,
                                peer.netid,
                            );
                            continue;
                        }
                    };
                    log::debug!(
                        "endpoint of node {} in net {} be resolved to: {endpoint:?}",
                        node.nodeid,
                        peer.netid,
                    );
                }

                let peer = Arc::new(Peer {
                    tunn: tu,
                    id: idx,
                    endpoint,
                    allowed_ips: node.allowed_ips.clone(),
                });
                peer_state.peers_by_idx.insert(idx, peer.clone());
                peer_state.peers.insert(pubkey, peer.clone());
                for ip in &node.allowed_ips {
                    peer_state.peers_by_ip.insert(ip.clone(), peer.clone());
                }
            }

            peer_state.registry.insert(peer.netid.clone(), peer);
        }

        Ok(())
    }

    pub async fn handle_tun_packets(&self, data: BytesMut) -> ConetResult<()> {
        let dst_addr = match Tunn::dst_address(&data) {
            Some(addr) => addr,
            None => {
                return Err(Error::Err(
                    "cannot get dst_addr from tun packet".to_string(),
                ));
            }
        };
        log::debug!("receive packet from tun device with dst: {dst_addr}");

        let peer_state = self.peer_state.write().await;
        let peer = match peer_state.peers_by_ip.get(&IpNet::from(dst_addr)) {
            Some(p) => p,
            None => return Err(Error::Err(format!("no endpoint to {}", &dst_addr))),
        };

        if !peer.contains(&dst_addr) {
            return Err(Error::Err(format!("{} is not in allowed_ips", &dst_addr)));
        }

        let mut dst = BytesMut::zeroed(std::cmp::max(data.len() + 32, 148));
        // SAFETY: RwLock of PeerState has provided safety
        unsafe {
            let peer_ptr = Arc::into_raw(peer.clone()) as *mut Peer;
            let tunn = &mut (*peer_ptr).tunn;
            let id = (*peer_ptr).id;
            match tunn.encapsulate(&data, &mut dst) {
                boringtun::noise::TunnResult::Done => return Ok(()),
                boringtun::noise::TunnResult::Err(wire_guard_error) => {
                    return Err(Error::Err(format!("{:?}", wire_guard_error)));
                }
                boringtun::noise::TunnResult::WriteToNetwork(packet) => {
                    if let Some(endpoint) = peer.endpoint {
                        match self.udp.send_to(&packet, endpoint).await {
                            Ok(_) => {
                                log::debug!("send packet to {endpoint} with peer id {}", id);
                                Ok(())
                            }
                            Err(e) => {
                                return Err(Error::Err(format!(
                                    "cannot send packet to {}: {}",
                                    endpoint, e
                                )));
                            }
                        }
                    } else {
                        return Err(Error::Err(format!("no endpoint for {}", &dst_addr)));
                    }
                }
                _ => return Err(Error::Err("Unexpected result from encapsulate".to_string())),
            }
        }
    }

    pub async fn handle_udp_packets(
        &self,
        src_addr: SocketAddr,
        data: BytesMut,
    ) -> ConetResult<()> {
        let mut dst = BytesMut::zeroed(MAX_UDP_SIZE);
        let packet = match self
            .rate_limiter
            .verify_packet(Some(src_addr.ip()), &data, &mut dst)
        {
            Ok(p) => p,
            Err(e) => return Err(Error::Err(format!("packet failed to verify: {:?}", e))),
        };

        log::debug!("receive packet from udp sockets with source: {src_addr}");

        let peer_state = self.peer_state.write().await;
        let private_key = &self.key_pair.0;
        let public_key = &self.key_pair.1;
        let peer = match &packet {
            boringtun::noise::Packet::HandshakeInit(p) => {
                parse_handshake_anon(private_key, public_key, &p)
                    .ok()
                    .and_then(|hh| {
                        peer_state
                            .peers
                            .get(&PublicKey::from(hh.peer_static_public))
                    })
            }
            boringtun::noise::Packet::HandshakeResponse(p) => {
                peer_state.peers_by_idx.get(&(p.receiver_idx >> 8))
            }
            boringtun::noise::Packet::PacketCookieReply(p) => {
                peer_state.peers_by_idx.get(&(p.receiver_idx >> 8))
            }
            boringtun::noise::Packet::PacketData(p) => {
                peer_state.peers_by_idx.get(&(p.receiver_idx >> 8))
            }
        };

        let peer = match peer {
            Some(p) => p,
            None => {
                log::debug!(
                    "received packet from other node with source address {}, but there is no peer",
                    src_addr
                );
                return Ok(());
            }
        };

        // Are there packets to send from the queue?
        let mut flush = false;
        // SAFETY: RwLock of PeerState has provided safety
        unsafe {
            let peer_ptr = Arc::into_raw(peer.clone()) as *mut Peer;
            let tunn = &mut (*peer_ptr).tunn;
            // set endpoint here because raw pointer is not send, so can't use it after resume from
            // await
            (*peer_ptr).endpoint = Some(src_addr);
            match tunn.handle_verified_packet(packet, &mut dst) {
                TunnResult::Done => return Ok(()),
                TunnResult::Err(e) => return Err(Error::Err(format!("{:?}", e))),
                TunnResult::WriteToNetwork(p) => {
                    flush = true;
                    if let Err(e) = self.udp.send_to(&p, src_addr).await {
                        return Err(Error::Err(format!("{:?}", e)));
                    }
                }
                TunnResult::WriteToTunnelV4(p, ipv4_addr) => {
                    if peer.contains(&IpAddr::from(ipv4_addr)) {
                        if let Err(e) = self.tun.send(p).await {
                            return Err(Error::Err(format!("{:?}", e)));
                        }
                    }
                }
                TunnResult::WriteToTunnelV6(p, ipv6_addr) => {
                    if peer.contains(&IpAddr::from(ipv6_addr)) {
                        if let Err(e) = self.tun.send(p).await {
                            return Err(Error::Err(format!("{:?}", e)));
                        }
                    }
                }
            }

            if flush {
                while let TunnResult::WriteToNetwork(packet) = tunn.decapsulate(None, &[], &mut dst)
                {
                    let _ = self.udp.send_to(packet, src_addr).await;
                }
            }
        }

        Ok(())
    }

    pub async fn event_loop(&self) -> ConetResult<()> {
        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    let b = BytesMut::new();
                    let r = self.message_channel.sender.send(Message{
                        t: MessageType::Stop,
                        data: b,
                        src_addr: None
                    }).await;

                    if let Err(e) = r  {
                        log::warn!("send stop message to worker failed: {e}");
                    }else{
                        return Ok(());
                    }
                },
                (r,mut b) = async {
                    let mut buf = BytesMut::zeroed(MAX_UDP_SIZE);
                    let r = self.tun.recv(&mut buf).await;
                    (r, buf)
                } => {
                    match r {
                        Err(e) => log::warn!("failed to receive data from tun device: {e}"),
                        Ok(n) => {
                            b.truncate(n);
                            let r = self.message_channel.sender.send(Message{
                                t: MessageType::FromTun,
                                data: b,
                                src_addr: None
                            }).await;

                            if let Err(e) = r  {
                                log::warn!("send tun message to worker failed: {e}");
                            }
                        }
                    }
                },
                (r,mut b) = async {
                    let mut buf = BytesMut::zeroed(MAX_UDP_SIZE);
                    let r = self.udp.recv_from(&mut buf).await;
                    (r,buf)
                } => {
                    match r {
                        Err(e) => log::warn!("failed to receive data from tun device: {e}"),
                        Ok((n, addr)) => {
                            b.truncate(n);
                            let r = self.message_channel.sender.send(Message{
                                t: MessageType::FromUdp,
                                data: b,
                                src_addr: Some(addr)
                            }).await;

                            if let Err(e) = r  {
                                log::warn!("send udp message to worker failed: {e}");
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Peer {
    fn contains(&self, target: &IpAddr) -> bool {
        for net in &self.allowed_ips {
            if net.contains(target) {
                return true;
            }
        }
        false
    }
}

/// A basic linear-feedback shift register implemented as xorshift, used to
/// distribute peer indexes across the 24-bit address space reserved for peer
/// identification.
/// The purpose is to obscure the total number of peers using the system and to
/// ensure it requires a non-trivial amount of processing power and/or samples
/// to guess other peers' indices. Anything more ambitious than this is wasted
/// with only 24 bits of space.
struct IndexLfsr {
    initial: u32,
    lfsr: u32,
    mask: u32,
}

impl IndexLfsr {
    /// Generate a random 24-bit nonzero integer
    fn random_index() -> u32 {
        const LFSR_MAX: u32 = 0xffffff; // 24-bit seed
        loop {
            let i = OsRng.next_u32() & LFSR_MAX;
            if i > 0 {
                // LFSR seed must be non-zero
                return i;
            }
        }
    }

    /// Generate the next value in the pseudorandom sequence
    fn next(&mut self) -> u32 {
        // 24-bit polynomial for randomness. This is arbitrarily chosen to
        // inject bitflips into the value.
        const LFSR_POLY: u32 = 0xd80000; // 24-bit polynomial
        let value = self.lfsr - 1; // lfsr will never have value of 0
        self.lfsr = (self.lfsr >> 1) ^ ((0u32.wrapping_sub(self.lfsr & 1u32)) & LFSR_POLY);
        assert!(self.lfsr != self.initial, "Too many peers created");
        value ^ self.mask
    }
}

impl Default for IndexLfsr {
    fn default() -> Self {
        let seed = Self::random_index();
        IndexLfsr {
            initial: seed,
            lfsr: seed,
            mask: Self::random_index(),
        }
    }
}
