use core::slice;
use std::{net::SocketAddr, str::FromStr, usize};

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
        let n: usize;
        match &addr.ip() {
            std::net::IpAddr::V4(_) => n = self.udp4.send_to(buf, addr).await?,
            std::net::IpAddr::V6(_) => n = self.udp6.send_to(buf, addr).await?,
        }

        Ok(n)
    }

    pub async fn recv_from(&self, buf: &mut [u8]) -> ConetResult<(usize, SocketAddr)> {
        let len = buf.len();
        let raw_buf = buf.as_mut_ptr();

        // SAFETY: only one branch uses buf every loop
        tokio::select! {
            r = async {
                unsafe{
                    let temp_buf = slice::from_raw_parts_mut(raw_buf, len);
                    self.udp6.recv_from(temp_buf).await
                }
            }=> {
                let n = r?;
                Ok(n)
            },
            r = async {
                unsafe{
                    let temp_buf = slice::from_raw_parts_mut(raw_buf, len);
                    self.udp4.recv_from(temp_buf).await
                }
            } => {
                let n = r?;
                Ok(n)
            }
        }
    }
}
