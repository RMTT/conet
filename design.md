conet is an overlay network providing connectivity, routing and extensibility.

conet contains several components to work:
1. connection module: providing connectivity between nodes
2. routing module: routing packets between nodes
3. plugins: making conet extensible via luajit
4. cli: command line tool for configuring nodes


## Modules

### cli

cli takes two configuration files: one for configuring nodes, one for connecting other nodes

#### configuration format

configuring nodes:
```toml
[connection]

[routing]

[plugin]

```

configuration of registry:
```toml
[[peers]]
```

### Connection

Connection module currently based on wireguard(boringtun) to providing connectivity.

#### Configuration

format of connection configuration:
```toml
[connection]
netid = "test-net"
nodeid = "m1"
interface = "conet0"
listenPort = 51820
address = ["10.10.10.1/32"]
private_key = ""
```

format of registry:
```toml
[[peers]]
netid = "test-net"

[[peers.nodes]]
nodeid = "m2"
public_key = "peer_public_key_hex_here"
endpoint = "peer.example.com:51820"
allowed_ips = ["10.0.0.0/24"]

[[peers.nodes]]
nodeid = "m3",
public_key = "peer_public_key_hex_here"
endpoint = "peer2.example.com:51820"
allowed_ips = ["10.0.0.0/24"]
```

#### Code design

Connection module always provides a `Device` to other modules, which used to send and receive packets:
- The `Device` object should listen a tun socket and two udp sockets(ipv4 and ipv6) to receive packets from other peers. In the code path of receiving and sending packets, the hooks for plugins should exist. By the way, ipv4 and ipv6 should be two different fields.
- For `tun` socket mainched by `Device`, data received from `tun` should be encrypted. The dataflow is: packet from apps -> tun -> encrypted packet -> peer
- For `udp` socket(ipv4 and ipv6) mainched by `Device`, data received from `udp` should be decrypted. The dataflow is: encrypted packets from peers -> udp socket -> original packets -> tun -> apps
- `Device` will create several workers to handle packets received from `tun` and `udp` sockets
- The `Device` should also keep the state of peers(endpoint, public_key).
