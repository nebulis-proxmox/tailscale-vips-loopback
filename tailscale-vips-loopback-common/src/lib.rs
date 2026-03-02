#![no_std]

use core::net::SocketAddrV4;

pub struct CustomSocketAddrV4(pub SocketAddrV4);

impl From<&u64> for CustomSocketAddrV4 {
    fn from(value: &u64) -> Self {
        let port = (value & 0xFFFF) as u16;
        let ip = ((value >> 16) & 0xFFFFFFFF) as u32;
        Self(SocketAddrV4::new(ip.into(), port))
    }
}

impl<'a> Into<&'a SocketAddrV4> for &'a CustomSocketAddrV4 {
    fn into(self) -> &'a SocketAddrV4 {
        &self.0
    }
}

impl Into<u64> for &CustomSocketAddrV4 {
    fn into(self) -> u64 {
        let ip = u32::from(*self.0.ip());
        let port = self.0.port() as u64;
        (ip as u64) << 16 | port
    }
}
