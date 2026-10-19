mod dns;
mod dns_runtime;

pub use dns::Dns;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::net::{IpAddr, SocketAddr};

#[derive(Clone, Copy)]
pub enum NetworkError {
    Configuration,
    Transport,
    Destination,
}

impl NetworkError {
    fn outcome(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Transport => "transport",
            Self::Destination => "destination",
        }
    }
}

#[derive(Clone, Copy)]
pub enum DnsPurpose {
    Api,
    Media,
}

impl DnsPurpose {
    fn event(self) -> &'static str {
        match self {
            Self::Api => "remote_audio_api_dns_finished",
            Self::Media => "remote_audio_media_dns_finished",
        }
    }
}

pub struct PinnedResolver {
    host: String,
    addresses: Vec<SocketAddr>,
}

impl PinnedResolver {
    pub fn new(host: String, addresses: Vec<SocketAddr>) -> Self {
        Self { host, addresses }
    }
}

impl Resolve for PinnedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let addresses = (name.as_str() == self.host).then(|| self.addresses.clone());
        Box::pin(async move {
            match addresses {
                Some(addresses) => Ok(Box::new(addresses.into_iter()) as Addrs),
                None => Err(Box::new(std::io::Error::other("Destination not admitted"))
                    as Box<dyn std::error::Error + Send + Sync>),
            }
        })
    }
}

pub fn public_addresses(resolved: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, NetworkError> {
    let mut addresses = Vec::new();
    for address in resolved {
        if addresses.len() == 32 || !public_address(address.ip()) {
            return Err(NetworkError::Destination);
        }
        addresses.push(address);
    }
    if addresses.is_empty() {
        return Err(NetworkError::Destination);
    }
    addresses.sort();
    addresses.dedup();
    Ok(addresses)
}

fn public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let [a, b, c, _] = address.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 192 && b == 0 && (c == 0 || c == 2))
                || (a == 192 && b == 88 && c == 99)
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(address) => {
            let segments = address.segments();
            let octets = address.octets();
            segments[0] & 0xe000 == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8))
                && segments[0] != 0x2002
                && segments[0] != 0x3ffe
                && !(segments[0] == 0x3fff && segments[1] & 0xf000 == 0)
                && !matches!(&octets[8..12], [0, 0, 0x5e, 0xfe] | [2, 0, 0x5e, 0xfe])
        }
    }
}
