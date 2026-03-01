extern crate alloc;

use alloc::vec;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};
use smoltcp::time::Instant;

use super::virtio_net::VirtioNet;

pub struct NetworkStack {
    pub iface:   Interface,
    pub sockets: SocketSet<'static>,
    pub device:  VirtioNet,
}

impl NetworkStack {
    pub fn init(mut device: VirtioNet) -> Self {
        let mac = device.mac;
        let config = Config::new(EthernetAddress(mac).into());
        let mut iface = Interface::new(config, &mut device, Instant::ZERO);
        iface.update_ip_addrs(|addrs| {
            addrs.push(IpCidr::new(IpAddress::v4(10, 0, 2, 15), 24)).ok();
        });
        iface
            .routes_mut()
            .add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2))
            .ok();
        Self {
            iface,
            sockets: SocketSet::new(vec![]),
            device,
        }
    }

    pub fn poll(&mut self, timestamp: Instant) {
        self.iface.poll(timestamp, &mut self.device, &mut self.sockets);
    }
}
