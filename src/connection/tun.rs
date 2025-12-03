use ipnet::IpNet;

use crate::errors::ConetResult;

pub struct TunDevice {
    device: tun_rs::AsyncDevice,
}

impl TunDevice {
    pub fn new(name: &str) -> ConetResult<Self> {
        let device = tun_rs::DeviceBuilder::new().name(name).build_async()?;
        Ok(Self { device })
    }

    pub fn add_address(&self, addr: &IpNet) -> ConetResult<()> {
        match addr {
            IpNet::V4(ipv4_net) => {
                self.device
                    .add_address_v4(ipv4_net.network(), ipv4_net.netmask())?;
            }
            IpNet::V6(ipv6_net) => {
                self.device
                    .add_address_v6(ipv6_net.network(), ipv6_net.netmask())?;
            }
        }

        Ok(())
    }

    pub fn set_mtu(&self, value: u16) -> ConetResult<()> {
        self.device.set_mtu(value)?;
        Ok(())
    }

    pub async fn recv(&self, buf: &mut [u8]) -> ConetResult<usize> {
        let n = self.device.recv(buf).await?;
        Ok(n)
    }
}
