//! Parses and validates provider HTTP endpoints.
//!
//! Provider URLs are intentionally limited to `http://` localhost or loopback
//! addresses because Elgar currently talks to a local LM Studio server.

use std::net::{IpAddr, SocketAddr};

use crate::provider::types::ProviderError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::provider) struct HttpEndpoint {
    pub(in crate::provider) host: String,
    pub(in crate::provider) port: u16,
    pub(in crate::provider) path: String,
}

impl HttpEndpoint {
    /// Parses a provider URL and rejects non-localhost/non-loopback targets.
    pub(in crate::provider) fn parse(url: &str) -> Result<Self, ProviderError> {
        let rest = url.strip_prefix("http://").ok_or_else(|| {
            ProviderError::configuration("only http:// provider URLs are supported")
        })?;
        let (authority, path) = rest.split_once('/').ok_or_else(|| {
            ProviderError::configuration("provider URL must include a request path")
        })?;
        let (host, port) = parse_authority(authority)?;
        Ok(Self {
            host,
            port,
            path: format!("/{path}"),
        })
    }

    pub(in crate::provider) fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub(in crate::provider) fn socket_addr(&self) -> Result<SocketAddr, ProviderError> {
        if self.host.eq_ignore_ascii_case("localhost") {
            return format!("127.0.0.1:{}", self.port)
                .parse::<SocketAddr>()
                .map_err(|_| ProviderError::configuration("provider URL port is invalid"));
        }

        let host = self
            .host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(&self.host);
        let ip = host.parse::<IpAddr>().map_err(|_| {
            ProviderError::configuration("provider URL host must be localhost or a loopback IP")
        })?;
        if !ip.is_loopback() {
            return Err(ProviderError::configuration(
                "provider URL host must be localhost or a loopback IP",
            ));
        }

        Ok(SocketAddr::new(ip, self.port))
    }
}

fn parse_authority(authority: &str) -> Result<(String, u16), ProviderError> {
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => {
            let parsed_port = port
                .parse::<u16>()
                .map_err(|_| ProviderError::configuration("provider URL port is invalid"))?;
            (host, parsed_port)
        }
        None => (authority, 80),
    };

    if host.trim().is_empty() {
        return Err(ProviderError::configuration(
            "provider URL host must not be empty",
        ));
    }

    Ok((host.to_string(), port))
}
