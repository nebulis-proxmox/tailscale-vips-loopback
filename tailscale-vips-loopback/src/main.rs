use anyhow::Context as _;
use aya::programs::{CgroupAttachMode, CgroupSockAddr};
use clap::Parser;
use log::info;
#[rustfmt::skip]
use log::{debug, warn};
use aya::maps::HashMap;
use std::{
    fs::File,
    net::{IpAddr, SocketAddrV4},
    ops::Not,
    time::Duration,
};
use tailscale_vips_loopback_common::CustomSocketAddrV4;
use tokio::{process::Command, signal};

#[derive(Debug, Parser)]
struct Opt {
    #[clap(short, long, default_value = "/sys/fs/cgroup")]
    cgroup_path: String,
}

async fn fetch_serve_config() -> anyhow::Result<Vec<(SocketAddrV4, SocketAddrV4)>> {
    let result = Command::new("tailscale")
        .args(["serve", "status", "--json"])
        .output()
        .await
        .context("failed to fetch serve config")?;

    let serve_config = str::from_utf8(&result.stdout).context("Not valid UTF-8")?;

    let serve_config = serde_json::from_str::<serde_json::Value>(serve_config)
        .context("failed to parse serve config as JSON")?;

    serve_config
        .get("Services")
        .and_then(|svcs| svcs.as_object())
        .and_then(|svcs| {
            let mut svcs_redirections: Vec<(SocketAddrV4, SocketAddrV4)> =
                Vec::with_capacity(svcs.len());

            for (svc_name, svc_config) in svcs {
                let dns_svc_name = svc_name.trim_start_matches("svc:");

                let svc_ip = dns_lookup::lookup_host(dns_svc_name)
                    .ok()?
                    .find(|addr| addr.is_ipv4())
                    .and_then(|addr| match addr {
                        IpAddr::V4(ipv4_addr) => Some(ipv4_addr),
                        _ => None,
                    })?;

                let redirections = svc_config
                    .get("TCP")
                    .and_then(|tcp| tcp.as_object())
                    .and_then(|tcp_config| {
                        let mut redirections = Vec::with_capacity(tcp_config.len());

                        for (port, port_config) in tcp_config {
                            let port = port.parse::<u16>().ok()?;
                            let tcp_forward = port_config
                                .get("TCPForward")?
                                .as_str()?
                                .parse::<SocketAddrV4>()
                                .ok()?;

                            redirections.push((port, tcp_forward));
                        }
                        Some(redirections)
                    })?
                    .into_iter()
                    .map(|(port, tcp_forward)| (SocketAddrV4::new(svc_ip, port), tcp_forward));

                svcs_redirections.extend(redirections);
            }

            Some(svcs_redirections)
        })
        .or_else(|| Some(Vec::with_capacity(0)))
        .context("Cannot get tailscale config")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();

    env_logger::init();

    // Bump the memlock rlimit. This is needed for older kernels that don't use the
    // new memcg based accounting, see https://lwn.net/Articles/837122/
    let rlim = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    let ret = unsafe { libc::setrlimit(libc::RLIMIT_MEMLOCK, &rlim) };
    if ret != 0 {
        debug!("remove limit on locked memory failed, ret is: {ret}");
    }

    // This will include your eBPF object file as raw bytes at compile-time and load it at
    // runtime. This approach is recommended for most real-world use cases. If you would
    // like to specify the eBPF program at runtime rather than at compile-time, you can
    // reach for `Bpf::load_file` instead.
    let mut ebpf = aya::Ebpf::load(aya::include_bytes_aligned!(concat!(
        env!("OUT_DIR"),
        "/tailscale-vips-loopback"
    )))?;

    match aya_log::EbpfLogger::init(&mut ebpf) {
        Err(e) => {
            // This can happen if you remove all log statements from your eBPF program.
            warn!("failed to initialize eBPF logger: {e}");
        }
        Ok(logger) => {
            let mut logger =
                tokio::io::unix::AsyncFd::with_interest(logger, tokio::io::Interest::READABLE)?;
            tokio::task::spawn(async move {
                loop {
                    let mut guard = logger.readable_mut().await.unwrap();
                    guard.get_inner_mut().flush();
                    guard.clear_ready();
                }
            });
        }
    }
    let Opt { cgroup_path } = opt;

    let program: &mut CgroupSockAddr = ebpf
        .program_mut("tailscale_vips_loopback")
        .unwrap()
        .try_into()?;
    program.load()?;
    let file = File::open(&cgroup_path)?;
    program.attach(&file, CgroupAttachMode::Single)
        .context("failed to attach the CgroupSockAddr program with default flags - try changing CgroupAttachMode::Single to CgroupAttachMode::AllowMultiple")?;

    let mut redirect_list: HashMap<_, u64, u64> =
        HashMap::try_from(ebpf.map_mut("REDIRECT_LIST").unwrap())
            .context("failed to get REDIRECT_LIST map from eBPF program")
            .unwrap();

    info!("Successfully loaded and attached eBPF program. Starting to monitor serve config...");

    let mut interval = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                match fetch_serve_config().await {
                Ok(config) => {
                    let new_srcs = config.iter().map(|(src, _)| src).collect::<Vec<_>>();
                    let old_srcs = redirect_list.keys().filter_map(Result::ok).map(|k| CustomSocketAddrV4::from(&k).0).collect::<Vec<_>>();

                    let to_remove = redirect_list
                        .keys()
                        .filter_map(Result::ok)
                        .filter(|k| new_srcs.contains(&&CustomSocketAddrV4::from(k).0).not())
                        .collect::<Vec<_>>();

                    for src in to_remove {
                        info!(
                            "Removing redirection from {}",
                            CustomSocketAddrV4::from(&src).0
                        );
                        redirect_list.remove(&src).unwrap_or_else(|e| {
                            warn!(
                                "Failed to remove redirection from {}: {e}",
                                CustomSocketAddrV4::from(&src).0
                            )
                        });
                    }

                    let to_add = config
                        .into_iter()
                        .filter(|(src, _)| old_srcs.contains(src).not())
                        .collect::<Vec<_>>();

                    for (src, dst) in to_add {
                        info!("Inserting redirection from {} to {}", src, dst);
                        redirect_list
                            .insert(
                                <&CustomSocketAddrV4 as Into<u64>>::into(&CustomSocketAddrV4(src)),
                                <&CustomSocketAddrV4 as Into<u64>>::into(&CustomSocketAddrV4(dst)),
                                0,
                            )
                            .unwrap_or_else(|e| {
                                warn!("Failed to insert redirection from {} to {}: {e}", src, dst)
                            });
                    }
                }
                Err(e) => warn!("Failed to fetch serve config: {e}"),
            }
            },
            _ = signal::ctrl_c() => {
                println!("Exiting...");
                break
            },
        }
    }

    Ok(())
}
