# Conet

Conet is an overlay network written in Rust that provides secure connectivity, routing, and extensibility through plugins.

## Architecture

The project is organized into three main modules:

### Connection Module
- **Device**: Main struct that manages TUN interface, UDP sockets, and plugin hooks.
- Creates TUN interface using `tun-rs`.
- Runs async packet loop handling TUN and UDP I/O via `tokio`.
- Supports plugin hooks for packet processing.
- Uses `boringtun` for WireGuard cryptography.

## Building and Running

### Prerequisites
- Linux (for TUN/TAP support)
- Rust toolchain

### Building

```bash
cargo build              # Development build
cargo build --release    # Production build
```

### Testing

```bash
cargo test               # Run all tests
```

### Running

Running the application requires root privileges to create the TUN interface.

```bash
sudo ./target/release/conet -c config.toml -r registry.toml
```

## Configuration

### Node Configuration (`config.toml`)

```toml
[connection]
netid = "test-net"
nodeid = "m1"
interface = "conet0"
listen_port = 51820
address = ["10.10.10.1/32"]
private_key = "BASE64_PRIVATE_KEY"
```

### Registry Configuration (`registry.toml`)

```toml
[[peers]]
netid = "test-net"

[[peers.nodes]]
nodeid = "m2"
public_key = "PEER_PUBLIC_KEY"
endpoint = "peer.example.com:51820"
allowed_ips = ["10.0.0.0/24"]
```
