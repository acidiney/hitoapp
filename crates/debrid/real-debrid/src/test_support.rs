use crate::{Error, RealDebridClient};
use std::net::SocketAddr;

pub fn owned_protocol_client(
    provider_access_token: String,
    ca_der: &[u8],
    api_address: SocketAddr,
    media_address: SocketAddr,
) -> Result<RealDebridClient, Error> {
    RealDebridClient::with_test_tls(
        provider_access_token,
        ca_der,
        api_address,
        ("media.hito-fixture.invalid", media_address),
    )
}
