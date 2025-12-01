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


### Connection

Connection module currently based on wireguard(boringtun) to providing connectivity.

#### Code design

Connection module always provides a `Device` to other modules, which used to send and receive packets.

The `Device` object should listen a tun socket and two udp sockets(ipv4 and ipv6) to receive packets from other nodes. In the code path of receiving and sending packets, the hooks for plugins should
exist.
