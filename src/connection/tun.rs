use std::usize;

use ipnet::IpNet;

use crate::errors::ConetResult;

pub struct TunDevice {
    device: tun_rs::AsyncDevice,
    mtu: u16,
}

impl TunDevice {
    pub fn new(name: &str, mtu: Option<u16>) -> ConetResult<Self> {
        let mtu = mtu.unwrap_or(1420);
        let device = tun_rs::DeviceBuilder::new()
            .name(name)
            .mtu(mtu)
            .build_async()?;
        Ok(Self { device, mtu })
    }

    pub fn add_address(&self, addr: &IpNet) -> ConetResult<()> {
        match addr {
            IpNet::V4(ipv4_net) => {
                self.device
                    .add_address_v4(ipv4_net.addr(), ipv4_net.netmask())?;
            }
            IpNet::V6(ipv6_net) => {
                self.device
                    .add_address_v6(ipv6_net.addr(), ipv6_net.netmask())?;
            }
        }

        Ok(())
    }

    pub fn set_mtu(&self, value: u16) -> ConetResult<()> {
        self.device.set_mtu(value)?;
        Ok(())
    }

    pub fn mtu(&self) -> u16 {
        self.mtu
    }

    pub async fn recv(&self, buf: &mut [u8]) -> ConetResult<usize> {
        let n = self.device.recv(buf).await?;
        Ok(n)
    }

    pub async fn send(&self, buf: &mut [u8]) -> ConetResult<usize> {
        let n = self.device.send(buf).await?;
        Ok(n)
    }
}
