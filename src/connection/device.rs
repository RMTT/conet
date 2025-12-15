use crate::errors::ConetResult;
use crate::utils;
use crate::{connection::config::ConnectionConfig, errors::Error};
use async_channel::{Receiver, Sender};
use boringtun::noise::errors::WireGuardError;
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
use std::{net::SocketAddr, time::Duration};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use super::peer::Peer;
use super::peer::PeerMap;
use super::tun::TunDevice;
use super::udp::UdpSocket;

const MAX_UDP_SIZE: usize = 0xFFFF;
const HANDSHAKE_RATE_LIMIT: u64 = 100; // The number of handshakes per second we can tolerate before using cookies
const TIMER_INTERVAL: Duration = Duration::from_millis(250); // Timer update interval
const RATE_LIMITER_TIMER_INTERVAL: Duration = Duration::from_secs(1);

pub struct Device {
    config: ConnectionConfig,
    tun: TunDevice,
    udp: UdpSocket,
    key_pair: (StaticSecret, PublicKey),
    pub message_channel: MessageChannel,
    peer_map: RwLock<PeerMap>,
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

        let peer_map = RwLock::new(PeerMap::new());

        Ok(Self {
            config,
            key_pair: (private_key, public_key),
            message_channel,
            tun,
            udp,
            peer_map,
            cancel_token,
            index_generator: Mutex::new(Default::default()),
            rate_limiter: RateLimiter::new(&public_key, HANDSHAKE_RATE_LIMIT),
        })
    }

    /// Add a single peer to the device
    pub async fn add_peer(
        &self,
        node: &crate::connection::config::PeerInfo,
        netid: &str,
    ) -> ConetResult<()> {
        let mut peer_map = self.peer_map.write().await;
        let mut idx_generator = self.index_generator.lock().unwrap();

        if &node.nodeid == &self.config.nodeid && netid == &self.config.netid {
            return Ok(());
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
                        "endpoint of node {} cannot be resolved: {e}, skipped",
                        node.nodeid,
                    );
                    return Err(Error::Err(format!("Cannot resolve endpoint: {}", e)));
                }
            };
            log::debug!(
                "endpoint of node {} be resolved to: {endpoint:?}",
                node.nodeid,
            );
        }

        let peer = Peer {
            tunn: tu,
            id: idx,
            nodeid: node.nodeid.clone(),
            netid: netid.to_string(),
            endpoint,
            allowed_ips: node.allowed_ips.clone(),
            persistent_keepalive: node.persistent_keepalive,
        };

        peer_map.add_peer(pubkey, peer);

        Ok(())
    }

    /// Remove all peers belonging to a specific netid
    pub async fn remove_peers_by_netid(&self, netid: &str) -> ConetResult<usize> {
        let mut peer_map = self.peer_map.write().await;
        let mut removed_count = 0;

        // Collect peers to remove
        let mut pubkeys_to_remove = Vec::new();
        for (pubkey, peer) in peer_map.get_all_peers() {
            if let Ok(peer_lock) = peer.lock() {
                if peer_lock.netid == netid {
                    pubkeys_to_remove.push(pubkey.clone());
                    removed_count += 1;
                }
            }
        }

        // Remove peers
        for pubkey in pubkeys_to_remove {
            peer_map.remove_peer_by_pubkey(&pubkey);
        }

        log::info!("Removed {} peers from netid {}", removed_count, netid);
        Ok(removed_count)
    }

    /// Update timers for all peers
    async fn update_all_timers(&self) -> ConetResult<()> {
        let peer_map = self.peer_map.read().await;
        let mut dst = BytesMut::zeroed(2048);

        for (_, peer) in peer_map.get_all_peers() {
            if let Ok(mut peer_lock) = peer.lock() {
                let endpoint = peer_lock.endpoint;

                match peer_lock.tunn.update_timers(&mut dst) {
                    TunnResult::Done => {
                        // No action needed
                    }
                    TunnResult::Err(e) => {
                        log::warn!("Timer update error for peer {}: {:?}", peer_lock.id, e);
                        // If connection expired, clear endpoint
                        if matches!(e, WireGuardError::ConnectionExpired) {
                            peer_lock.endpoint = None;
                        }
                    }
                    TunnResult::WriteToNetwork(packet) => {
                        if let Some(endpoint) = endpoint {
                            match self.udp.send_to(&packet, endpoint).await {
                                Ok(_) => {
                                    log::debug!("Sent timer update packet to {}", endpoint);
                                }
                                Err(e) => {
                                    log::warn!(
                                        "Failed to send timer update packet to {}: {}",
                                        endpoint,
                                        e
                                    );
                                }
                            }
                        }
                    }
                    _ => {
                        log::warn!(
                            "Unexpected result from update_timers for peer {}",
                            peer_lock.id
                        );
                    }
                }
            }
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

        let mut dst = BytesMut::zeroed(std::cmp::max(data.len() + 32, 148));
        let (packet_result, endpoint) = {
            let peer_map = self.peer_map.read().await;
            let mut peer = match peer_map.get_peer_by_ip(&IpNet::from(dst_addr)) {
                Some(p) => p.lock().unwrap(),
                None => return Err(Error::Err(format!("no endpoint to {}", &dst_addr))),
            };

            // Check if destination is in allowed IPs
            if !peer.contains(&dst_addr) {
                return Err(Error::Err(format!("{} is not in allowed_ips", &dst_addr)));
            }

            let packet_result = peer.tunn.encapsulate(&data, &mut dst);
            let endpoint = peer.endpoint;

            (packet_result, endpoint)
        }; // release lock here

        match packet_result {
            boringtun::noise::TunnResult::Done => return Ok(()),
            boringtun::noise::TunnResult::Err(wire_guard_error) => {
                return Err(Error::Err(format!("{:?}", wire_guard_error)));
            }
            boringtun::noise::TunnResult::WriteToNetwork(packet) => {
                if let Some(endpoint) = endpoint {
                    match self.udp.send_to(&packet, endpoint).await {
                        Ok(_) => {
                            log::debug!("send packet to {endpoint}");
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

        // Find the peer
        let peer_ref = {
            let peer_map = self.peer_map.read().await;
            let private_key = &self.key_pair.0;
            let public_key = &self.key_pair.1;

            let peer = match &packet {
                boringtun::noise::Packet::HandshakeInit(p) => {
                    parse_handshake_anon(private_key, public_key, &p)
                        .ok()
                        .and_then(|hh| peer_map.get_peer(&PublicKey::from(hh.peer_static_public)))
                }
                boringtun::noise::Packet::HandshakeResponse(p) => {
                    peer_map.get_peer_by_id(&(p.receiver_idx >> 8))
                }
                boringtun::noise::Packet::PacketCookieReply(p) => {
                    peer_map.get_peer_by_id(&(p.receiver_idx >> 8))
                }
                boringtun::noise::Packet::PacketData(p) => {
                    peer_map.get_peer_by_id(&(p.receiver_idx >> 8))
                }
            };

            // Clone the Arc to keep the peer alive
            peer.cloned()
        }; // peer_map is dropped here, releasing the lock

        let peer_ref = match peer_ref {
            Some(p) => p,
            None => {
                log::debug!(
                    "received packet from other node with source address {}, but there is no peer",
                    src_addr
                );
                return Ok(());
            }
        };

        // First handle the packet and collect actions
        let mut tunnel_packets = Vec::new();
        let mut udp_packets = Vec::new();
        let mut flush_needed = false;

        {
            let mut peer_lock = peer_ref.lock().unwrap();

            // set endpoint
            peer_lock.endpoint = Some(src_addr);

            match peer_lock.tunn.handle_verified_packet(packet, &mut dst) {
                TunnResult::Done => return Ok(()),
                TunnResult::Err(e) => return Err(Error::Err(format!("{:?}", e))),
                TunnResult::WriteToNetwork(p) => {
                    udp_packets.push(p.to_vec());
                    flush_needed = true;
                }
                TunnResult::WriteToTunnelV4(p, ipv4_addr) => {
                    if peer_lock.contains(&IpAddr::from(ipv4_addr)) {
                        tunnel_packets.push(p.to_vec());
                    }
                }
                TunnResult::WriteToTunnelV6(p, ipv6_addr) => {
                    if peer_lock.contains(&IpAddr::from(ipv6_addr)) {
                        tunnel_packets.push(p.to_vec());
                    }
                }
            }

            if flush_needed {
                while let TunnResult::WriteToNetwork(packet) =
                    peer_lock.tunn.decapsulate(None, &[], &mut dst)
                {
                    udp_packets.push(packet.to_vec());
                }
            }
        } // Lock is released here

        // Send UDP packets outside the lock
        for packet in udp_packets {
            if let Err(e) = self.udp.send_to(&packet, src_addr).await {
                return Err(Error::Err(format!("{:?}", e)));
            }
        }

        // Send tunnel packets outside the lock
        for mut packet in tunnel_packets {
            if let Err(e) = self.tun.send(&mut packet).await {
                return Err(Error::Err(format!("{:?}", e)));
            }
        }

        Ok(())
    }

    pub async fn timer_loop(&self) -> ConetResult<()> {
        let mut timer_interval = tokio::time::interval(TIMER_INTERVAL);
        let mut rate_limiter_reset_interval = tokio::time::interval(RATE_LIMITER_TIMER_INTERVAL);

        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                        return Ok(());
                },
                _ = timer_interval.tick() => {
                    // Update timers for all peers
                    if let Err(e) = self.update_all_timers().await {
                        log::warn!("Failed to update timers: {}", e);
                    }
                },
                _ = rate_limiter_reset_interval.tick() => {
                    self.rate_limiter.reset_count();
                }
            }
        }
    }

    pub async fn event_loop(&self) -> ConetResult<()> {
        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    return Ok(());
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
