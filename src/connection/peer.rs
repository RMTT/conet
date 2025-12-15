use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
};

use boringtun::{noise::Tunn, x25519::PublicKey};
use ipnet::IpNet;

pub struct Peer {
    pub tunn: Tunn,
    pub id: u32,
    pub nodeid: String,
    pub netid: String,
    pub endpoint: Option<SocketAddr>,
    pub allowed_ips: Vec<IpNet>,
    pub persistent_keepalive: Option<u16>,
}

impl Peer {
    pub fn contains(&self, target: &IpAddr) -> bool {
        for net in &self.allowed_ips {
            if net.contains(target) {
                return true;
            }
        }
        false
    }
}

/// combine peer connection info
pub struct PeerMap {
    peers_by_ip: HashMap<IpNet, Arc<Mutex<Peer>>>,
    peers_by_idx: HashMap<u32, Arc<Mutex<Peer>>>,
    peers: HashMap<PublicKey, Arc<Mutex<Peer>>>,
}

impl PeerMap {
    /// Create a new PeerMap
    pub fn new() -> Self {
        Self {
            peers_by_ip: HashMap::new(),
            peers_by_idx: HashMap::new(),
            peers: HashMap::new(),
        }
    }

    /// Add a peer to all maps
    pub fn add_peer(&mut self, pubkey: PublicKey, peer: Peer) {
        let id = peer.id;
        let allowed_ips = peer.allowed_ips.clone();
        let peer_arc = Arc::new(Mutex::new(peer));

        // Add to peers map
        self.peers.insert(pubkey, peer_arc.clone());

        // Add to peers_by_idx map
        self.peers_by_idx.insert(id, peer_arc.clone());

        // Add to peers_by_ip map
        for ip in allowed_ips {
            self.peers_by_ip.insert(ip.clone(), peer_arc.clone());
        }
    }

    pub fn add_peer_by_pubkey(&mut self, pubkey: PublicKey, peer: Arc<Mutex<Peer>>) {
        self.peers.insert(pubkey, peer.clone());
    }

    /// Add a peer and return by IP
    pub fn add_peer_by_ip(&mut self, ip: IpNet, peer: Arc<Mutex<Peer>>) {
        self.peers_by_ip.insert(ip, peer);
    }

    /// Add a peer and return by ID
    pub fn add_peer_by_id(&mut self, id: u32, peer: Arc<Mutex<Peer>>) {
        self.peers_by_idx.insert(id, peer);
    }

    /// Delete a peer from all maps
    pub fn remove_peer(&mut self, pubkey: PublicKey, peer: &Peer) -> Option<Arc<Mutex<Peer>>> {
        let id = peer.id;

        self.peers_by_idx.remove(&id);

        for ip in &peer.allowed_ips {
            self.peers_by_ip.remove(&ip);
        }

        self.peers.remove(&pubkey)
    }

    /// Remove a peer by public key
    pub fn remove_peer_by_pubkey(&mut self, pubkey: &PublicKey) -> Option<Arc<Mutex<Peer>>> {
        self.peers.remove(pubkey)
    }

    /// Remove a peer by ID
    pub fn remove_peer_by_id(&mut self, id: &u32) -> Option<Arc<Mutex<Peer>>> {
        self.peers_by_idx.remove(id)
    }

    /// Remove a peer by IP
    pub fn remove_peer_by_ip(&mut self, ip: &IpNet) -> Option<Arc<Mutex<Peer>>> {
        self.peers_by_ip.remove(ip)
    }

    /// Get a peer by public key
    pub fn get_peer(&self, pubkey: &PublicKey) -> Option<&Arc<Mutex<Peer>>> {
        self.peers.get(pubkey)
    }

    /// Get a peer by ID
    pub fn get_peer_by_id(&self, id: &u32) -> Option<&Arc<Mutex<Peer>>> {
        self.peers_by_idx.get(id)
    }

    /// Get a peer by IP
    pub fn get_peer_by_ip(&self, ip: &IpNet) -> Option<&Arc<Mutex<Peer>>> {
        self.peers_by_ip.get(ip)
    }

    /// Get all peers
    pub fn get_all_peers(&self) -> &HashMap<PublicKey, Arc<Mutex<Peer>>> {
        &self.peers
    }

    /// Clear all peers
    pub fn clear(&mut self) {
        self.peers.clear();
        self.peers_by_idx.clear();
        self.peers_by_ip.clear();
    }

    /// Get the number of peers
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Check if there are any peers
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }
}
