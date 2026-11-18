//! Personas and their network routes.
//!
//! A persona is the unit of separation. Each persona has its accounts, one browser profile and one route.
//! In step 1 there is one persona, and the user interface does not show it. The store can hold more personas,
//! so step 2 does not change the stored data.

use crate::ids::{PersonaAddress, PersonaId};

/// A persona.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Persona {
    pub id: PersonaId,
    /// The name that the person gives to the persona. Only the person sees it.
    pub name: String,
    /// The address of the persona key. Other people know the persona by this address.
    pub address: PersonaAddress,
    /// The network route for all the accounts and the browser profile of the persona.
    pub route: Route,
}

impl Persona {
    /// The name of the browser profile folder of the persona.
    ///
    /// The name comes from the identifier only. It does not contain the name of the persona or its address.
    pub fn profile_folder(&self) -> String {
        self.id.to_string()
    }
}

/// The data that is necessary to add a persona.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPersona {
    pub name: String,
    pub address: PersonaAddress,
    pub route: Route,
}

/// How the traffic of a persona goes to the network.
#[derive(Clone, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Route {
    /// The traffic goes directly to the network.
    #[default]
    Direct,
    /// The traffic goes through a proxy. For Tor, use its SOCKS5 port.
    Proxy(ProxyEndpoint),
}

/// A proxy server.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProxyEndpoint {
    pub kind: ProxyKind,
    pub host: String,
    pub port: u16,
}

/// The protocol of a proxy server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProxyKind {
    /// HTTP CONNECT.
    Http,
    /// SOCKS5. The proxy gets the host name, so the DNS lookup does not leak.
    Socks5,
}

impl ProxyEndpoint {
    /// The URL form of the proxy, for example `socks5://127.0.0.1:9050`.
    pub fn url(&self) -> String {
        let scheme = match self.kind {
            ProxyKind::Http => "http",
            ProxyKind::Socks5 => "socks5",
        };
        format!("{scheme}://{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_folder_does_not_show_the_name() {
        let persona = Persona {
            id: PersonaId(7),
            name: "Work".into(),
            address: PersonaAddress([1; 20]),
            route: Route::Direct,
        };
        assert_eq!(persona.profile_folder(), "persona-7");
    }

    #[test]
    fn proxy_url() {
        let proxy = ProxyEndpoint {
            kind: ProxyKind::Socks5,
            host: "127.0.0.1".into(),
            port: 9050,
        };
        assert_eq!(proxy.url(), "socks5://127.0.0.1:9050");
    }
}
