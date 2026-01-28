use std::{net::SocketAddr, str::FromStr};

use socket2::{Domain, Protocol, SockAddr, Type};

use crate::errors::ConetResult;

pub struct UdpSocket {
    udp4: tokio::net::UdpSocket,
    udp6: tokio::net::UdpSocket,
}

impl UdpSocket {
    pub async fn new(port: u16) -> ConetResult<Self> {
        #[cfg(not(target_os = "linux"))]
        let udp6 = tokio::net::UdpSocket::bind(format!("[::]:{}", port)).await?;

        #[cfg(target_os = "linux")]
        let udp6 = {
            let std_addr = std::net::SocketAddr::from_str(&format!("[::]:{port}"))?;
            let s = socket2::Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
            s.set_only_v6(true)?;
            s.set_nonblocking(true)?;
            let address: SockAddr = SockAddr::from(std_addr);
            s.bind(&address)?;

            tokio::net::UdpSocket::from_std(s.into())?
        };

        let udp4 = tokio::net::UdpSocket::bind(format!("0.0.0.0:{}", port)).await?;

        Ok(Self { udp4, udp6 })
    }

    pub async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> ConetResult<usize> {
        let n = match &addr.ip() {
            std::net::IpAddr::V4(_) => self.udp4.send_to(buf, addr).await?,
            std::net::IpAddr::V6(_) => self.udp6.send_to(buf, addr).await?,
        };

        Ok(n)
    }

    pub async fn recv_from_v4(&self, buf: &mut [u8]) -> ConetResult<(usize, SocketAddr)> {
        let n = self.udp4.recv_from(buf).await?;
        Ok(n)
    }

    pub async fn recv_from_v6(&self, buf: &mut [u8]) -> ConetResult<(usize, SocketAddr)> {
        let n = self.udp6.recv_from(buf).await?;
        Ok(n)
    }
}
