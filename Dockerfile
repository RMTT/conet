FROM rust:1.87 AS builder

WORKDIR /app

COPY . .

RUN cargo install --path .

FROM debian:trixie-slim

RUN apt-get update && apt-get install -y tcpdump iproute2 iputils-ping

COPY --from=builder /usr/local/cargo/bin/conet /usr/local/bin/conet
