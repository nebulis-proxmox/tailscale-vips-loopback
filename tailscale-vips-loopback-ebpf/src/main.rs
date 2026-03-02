#![no_std]
#![no_main]

use core::net::{Ipv4Addr, SocketAddrV4};

use aya_ebpf::{
    macros::{cgroup_sock_addr, map},
    maps::HashMap,
    programs::SockAddrContext,
};
use aya_log_ebpf::info;
use tailscale_vips_loopback_common::CustomSocketAddrV4;

#[map]
static REDIRECT_LIST: HashMap<u64, u64> = HashMap::with_max_entries(256, 0);

#[cgroup_sock_addr(connect4)]
pub fn tailscale_vips_loopback(ctx: SockAddrContext) -> i32 {
    match try_connect4(ctx) {
        Ok(ret) => ret,
        Err(ret) => ret,
    }
}

fn redirect_socket(socket: &CustomSocketAddrV4) -> Option<CustomSocketAddrV4> {
    unsafe { REDIRECT_LIST.get(&socket.into()) }.map(|d| CustomSocketAddrV4::from(d))
}

fn try_connect4(ctx: SockAddrContext) -> Result<i32, i32> {
    let protocol = unsafe { (*ctx.sock_addr).protocol };

    if protocol != IPPROTO_IP {
        return Ok(1);
    }

    let dst_ip = unsafe { (*ctx.sock_addr).user_ip4 }.to_be();
    let dst_port: [u8; 4] = unsafe { (*ctx.sock_addr).user_port }.to_be_bytes();
    let dst_port = u16::from_be_bytes([dst_port[3], dst_port[2]]);
    let destination_addr = Ipv4Addr::from(dst_ip);

    let socket = CustomSocketAddrV4(SocketAddrV4::new(destination_addr, dst_port));

    if let Some(redirect_addr) = redirect_socket(&socket) {
        let redirect_addr: &SocketAddrV4 = (&redirect_addr).into();
        unsafe {
            (*ctx.sock_addr).user_ip4 = u32::from(*redirect_addr.ip()).to_be();
            (*ctx.sock_addr).user_port = redirect_addr.port().to_be() as u32;
        }
        info!(
            ctx,
            "redirecting connect4 from {}:{} to {}:{}",
            *socket.0.ip(),
            socket.0.port(),
            *redirect_addr.ip(),
            redirect_addr.port()
        );
    }
    Ok(1)
}
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[unsafe(link_section = "license")]
#[unsafe(no_mangle)]
static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";

const IPPROTO_IP: u32 = 6;
