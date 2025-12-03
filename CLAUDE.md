# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Always use context7 when I need code generation, setup or configuration steps, or library/API documentation. This means you should automatically use the Context7 MCP tools to resolve library id and get library docs without me having to explicitly ask.
Always use exa when I need web search.

## Project Overview

conet is an overlay network written in Rust that provides secure connectivity, routing, and extensibility through plugins.

## Common Commands

### Building
```bash
cargo build              # Development build
cargo build --release    # Production build
```

### Testing
```bash
cargo test               # Run all tests
cargo test <module_name> # Run tests for specific module
```

### Running
```bash
# Requires root privileges for TUN interface
sudo ./target/release/conet                    # Default config (config.toml)
sudo ./target/release/conet -c <config_file>   # Custom config file
```

### Key Generation (WireGuard)
```bash
wg genkey > privatekey        # Generate private key
wg pubkey < privatekey > publickey  # Derive public key
```

## Architecture

The project is organized into three main modules:

### Connection Module (`src/connection`)
- **Device**: Main struct that manages TUN interface, UDP sockets, and plugin hooks
- Creates TUN interface using `tun-rs`(https://crates.io/crates/tun-rs)
- Runs async packet loop handling TUN and UDP I/O via `tokio::select!`
- Supports plugin hooks for packet processing (`PluginHook` trait)
- Uses boringtun for WireGuard cryptography (x25519 key pair)
- Use `async_channel`(https://docs.rs/async-channel/latest/async_channel) for workers to pass packets
- Reference @design.md for other requirements

## Development Notes

### Platform Support
- Linux only (uses TUN/TAP and libc directly)
- Requires root privileges for TUN interface creation
- IPv4 and IPv6 support via separate UDP sockets

### Dependencies
- `boringtun`: WireGuard implementation for cryptographic operations
- `tokio`: Async runtime for networking and packet processing
- `clap`: CLI argument parsing
- `serde/toml`: Configuration parsing
- `tun-rs`: TUN/TAP implementation in rust
