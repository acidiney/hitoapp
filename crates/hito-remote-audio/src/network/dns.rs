use super::{DnsPurpose, NetworkError, dns_runtime::DnsScope};
use hickory_resolver::{
    Resolver,
    config::{LookupIpStrategy, ProtocolConfig, ResolveHosts, ResolverConfig},
};
use std::{net::SocketAddr, time::Duration};

const MAX_NAMESERVERS: usize = 3;

pub struct Dns {
    config: ResolverConfig,
}

impl Dns {
    pub fn system() -> Result<Self, NetworkError> {
        Self::configured(system_config()?)
    }

    fn configured(config: ResolverConfig) -> Result<Self, NetworkError> {
        let (_, _, servers) = config.into_parts();
        if servers.is_empty()
            || servers.len() > MAX_NAMESERVERS
            || servers.iter().any(|server| {
                server.ip.is_unspecified()
                    || server.ip.is_multicast()
                    || server.connections.is_empty()
                    || server.connections.len() > 2
                    || server.connections.iter().any(|connection| {
                        connection.port == 0
                            || connection.bind_addr.is_some()
                            || !matches!(
                                connection.protocol,
                                ProtocolConfig::Udp | ProtocolConfig::Tcp
                            )
                    })
            })
        {
            return Err(NetworkError::Configuration);
        }
        Ok(Self {
            config: ResolverConfig::from_name_servers(servers),
        })
    }

    #[cfg(feature = "test-support")]
    pub fn with_test_tcp(address: SocketAddr) -> Result<Self, NetworkError> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(NetworkError::Configuration);
        }
        let mut connection = hickory_resolver::config::ConnectionConfig::tcp();
        connection.port = address.port();
        Self::configured(ResolverConfig::from_name_servers(vec![
            hickory_resolver::config::NameServerConfig::new(address.ip(), true, vec![connection]),
        ]))
    }

    pub async fn addresses(
        &self,
        host: &str,
        purpose: DnsPurpose,
    ) -> Result<Vec<SocketAddr>, NetworkError> {
        let mut scope = DnsScope::new(purpose);
        let mut builder = Resolver::builder_with_config(self.config.clone(), scope.provider());
        let options = builder.options_mut();
        options.timeout = Duration::from_secs(10);
        options.attempts = 0;
        options.ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
        options.cache_size = 0;
        options.use_hosts_file = ResolveHosts::Never;
        options.num_concurrent_reqs = 1;
        options.max_active_requests = 2;
        options.preserve_intermediates = false;
        let resolver = builder.build().map_err(|_| NetworkError::Configuration)?;
        let response = scope.run(resolver.lookup_ip(format!("{host}."))).await?;
        let mut addresses = Vec::new();
        for address in response.iter() {
            if addresses.len() == 32 {
                return Err(NetworkError::Destination);
            }
            addresses.push(SocketAddr::new(address, 443));
        }
        Ok(addresses)
    }
}

#[cfg(all(unix, not(any(target_os = "android", target_vendor = "apple"))))]
fn system_config() -> Result<ResolverConfig, NetworkError> {
    use std::io::Read;
    let file = std::fs::File::open("/etc/resolv.conf").map_err(|_| NetworkError::Configuration)?;
    let mut bytes = Vec::new();
    file.take(16_385)
        .read_to_end(&mut bytes)
        .map_err(|_| NetworkError::Configuration)?;
    if bytes.len() > 16_384 {
        return Err(NetworkError::Configuration);
    }
    hickory_resolver::system_conf::parse_resolv_conf(bytes)
        .map(|(config, _)| config)
        .map_err(|_| NetworkError::Configuration)
}

#[cfg(any(target_os = "windows", target_os = "android", target_vendor = "apple"))]
fn system_config() -> Result<ResolverConfig, NetworkError> {
    hickory_resolver::system_conf::read_system_conf()
        .map(|(config, _)| config)
        .map_err(|_| NetworkError::Configuration)
}

#[cfg(not(any(unix, target_os = "windows")))]
fn system_config() -> Result<ResolverConfig, NetworkError> {
    Err(NetworkError::Configuration)
}
