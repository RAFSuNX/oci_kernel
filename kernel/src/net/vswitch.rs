extern crate alloc;
use alloc::{vec, vec::Vec};
use alloc::collections::BTreeSet;

use crate::container::runtime::ContainerId;

/// A minimal IPv4 address newtype (4 octets, big-endian storage).
///
/// We avoid smoltcp's IpAddress here so the vswitch remains testable on the
/// host target without importing the full smoltcp device layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ipv4Addr([u8; 4]);

impl Ipv4Addr {
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self([a, b, c, d])
    }

    pub fn octets(self) -> [u8; 4] {
        self.0
    }
}

impl From<[u8; 4]> for Ipv4Addr {
    fn from(b: [u8; 4]) -> Self { Self(b) }
}

/// An allocation pool for IPv4 container addresses within a subnet.
///
/// Reserves `.1` (gateway) and the network/broadcast addresses.
/// `release` rewinds the `next` cursor so freed addresses are reused first.
pub struct IpPool {
    base: u32, // network address in host byte order
    mask: u8,  // prefix length
    used: BTreeSet<u32>,
    next: u32, // next candidate (host byte order)
}

impl IpPool {
    /// Create a pool for the subnet `base/mask`.
    /// Allocation starts at `base + 2` (`.1` is the gateway).
    pub fn new(base: [u8; 4], mask: u8) -> Self {
        let b = u32::from_be_bytes(base);
        Self { base: b, mask, used: BTreeSet::new(), next: b + 2 }
    }

    /// Allocate the next available address. Returns `None` if the pool is full.
    pub fn allocate(&mut self) -> Option<Ipv4Addr> {
        let size = 1u32.checked_shl(32u32.saturating_sub(self.mask as u32))
            .unwrap_or(0);
        let max = self.base.saturating_add(size).saturating_sub(1); // broadcast
        while self.next < max {
            let candidate = self.next;
            self.next += 1;
            if self.used.insert(candidate) {
                return Some(Ipv4Addr::from(candidate.to_be_bytes()));
            }
        }
        None
    }

    /// Return an address to the pool. If it precedes `next`, rewind the cursor
    /// so the address will be offered again on the next `allocate()` call.
    pub fn release(&mut self, ip: Ipv4Addr) {
        let n = u32::from_be_bytes(ip.octets());
        if self.used.remove(&n) && n < self.next {
            self.next = n;
        }
    }

    pub fn available(&self) -> bool {
        let size = 1u32.checked_shl(32u32.saturating_sub(self.mask as u32))
            .unwrap_or(0);
        self.used.len() < size.saturating_sub(3) as usize // exclude net + gw + broadcast
    }
}

/// Firewall rule applied by the virtual switch.
#[derive(Debug, Clone, Copy)]
pub enum FirewallRule {
    /// Containers may not initiate connections to the host OS.
    BlockContainerToHost,
    /// Containers may reach the internet via NAT through the host NIC.
    AllowNatEgress,
    /// Containers in different namespaces cannot talk directly by default.
    BlockContainerToContainer,
}

/// A layer-2/3 virtual switch connecting all container vNICs.
///
/// Policy summary (default):
/// - Container → host: **blocked**
/// - Container → internet (NAT): **allowed**
/// - Container ↔ container (same host): **blocked**
pub struct VSwitch {
    rules: Vec<FirewallRule>,
}

impl VSwitch {
    pub fn new() -> Self {
        Self {
            rules: vec![
                FirewallRule::BlockContainerToHost,
                FirewallRule::AllowNatEgress,
                FirewallRule::BlockContainerToContainer,
            ],
        }
    }

    /// Returns `false` — containers are never allowed to reach the host OS directly.
    pub fn allow_to_host(&self, _src: ContainerId) -> bool {
        false
    }

    /// Returns `true` — outbound internet traffic is allowed (will be NAT'd).
    pub fn allow_egress(&self, _src: ContainerId, _dst: Ipv4Addr) -> bool {
        true
    }

    /// Returns `false` — cross-container traffic is blocked by default.
    pub fn allow_container_to_container(&self, _src: ContainerId, _dst: ContainerId) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_pool_assigns_unique_addresses() {
        let mut pool = IpPool::new([10, 0, 0, 0], 16);
        let a = pool.allocate().unwrap();
        let b = pool.allocate().unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn ip_pool_releases_and_reuses() {
        let mut pool = IpPool::new([10, 0, 0, 0], 16);
        let a = pool.allocate().unwrap();
        pool.release(a);
        let b = pool.allocate().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn ip_pool_first_address_is_dot_two() {
        let mut pool = IpPool::new([10, 1, 0, 0], 24);
        let ip = pool.allocate().unwrap();
        // .1 is reserved for gateway, first allocation is .2
        assert_eq!(ip, Ipv4Addr::new(10, 1, 0, 2));
    }

    #[test]
    fn vswitch_blocks_container_to_host() {
        let vswitch = VSwitch::new();
        let src = ContainerId::new();
        assert!(!vswitch.allow_to_host(src));
    }

    #[test]
    fn vswitch_allows_nat_to_internet() {
        let vswitch = VSwitch::new();
        let src = ContainerId::new();
        let dst = Ipv4Addr::new(8, 8, 8, 8);
        assert!(vswitch.allow_egress(src, dst));
    }

    #[test]
    fn vswitch_blocks_container_to_container() {
        let vswitch = VSwitch::new();
        let a = ContainerId::new();
        let b = ContainerId::new();
        assert!(!vswitch.allow_container_to_container(a, b));
    }
}
