//! Host-mediated destinations. A child network flag does not mint credentials
//! or an unrestricted route. The proxy connects to an address it already vetted.
#![allow(dead_code)]
use std::net::{IpAddr, SocketAddr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressClass {
    Public,
    Loopback,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub reference: String,
    pub protocol: String,
    pub hostname: String,
    pub port: u16,
    pub classes: Vec<AddressClass>,
    pub opaque_tunnel: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedRoute {
    pub reference: String,
    pub address: SocketAddr,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BrokerError {
    UnknownReference,
    Protocol,
    Port,
    AddressClass,
    Revoked,
    SiblingReference,
}

pub fn classify(address: IpAddr) -> AddressClass {
    match address {
        IpAddr::V4(ip) if ip.is_loopback() => AddressClass::Loopback,
        IpAddr::V6(ip) if ip.is_loopback() => AddressClass::Loopback,
        IpAddr::V4(ip) if ip.is_private() || ip.is_link_local() => AddressClass::Private,
        IpAddr::V6(ip) if ip.is_unique_local() || ip.is_unicast_link_local() => {
            AddressClass::Private
        }
        _ => AddressClass::Public,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn admit(
    destination: &Destination,
    reference: &str,
    owner: &str,
    presented_owner: &str,
    protocol: &str,
    port: u16,
    address: SocketAddr,
    generation: u64,
    live_generation: u64,
) -> Result<ApprovedRoute, BrokerError> {
    if generation != live_generation {
        return Err(BrokerError::Revoked);
    }
    if owner != presented_owner {
        return Err(BrokerError::SiblingReference);
    }
    if destination.reference != reference {
        return Err(BrokerError::UnknownReference);
    }
    if destination.protocol != protocol {
        return Err(BrokerError::Protocol);
    }
    if destination.port != port || address.port() != port {
        return Err(BrokerError::Port);
    }
    let class = classify(address.ip());
    if !destination.classes.contains(&class) {
        return Err(BrokerError::AddressClass);
    }
    Ok(ApprovedRoute {
        reference: reference.to_string(),
        address,
    })
}

/// Connect only to the admitted address. The hostname is not resolved again.
pub fn connect_admitted(route: &ApprovedRoute) -> std::io::Result<std::net::TcpStream> {
    std::net::TcpStream::connect_timeout(&route.address, std::time::Duration::from_secs(2))
}

pub fn direct_udp_allowed() -> bool {
    false
}

/// Last-match Seatbelt rules: every network family is denied, then TCP to the
/// proxy port alone is allowed. UDP, raw sockets, and other destinations stay denied.
pub fn proxy_only_rules(port: u16) -> String {
    format!("(deny network*)\n(allow network-outbound (remote tcp \"localhost:{port}\"))\n")
}

struct LiveRoute {
    route: ApprovedRoute,
    owner: String,
    generation: u64,
}

/// Per-worker HTTP proxy. It connects to an address admitted earlier and does
/// not resolve that hostname again.
pub struct ChildProxy {
    port: u16,
    generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    routes: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, LiveRoute>>>,
}

impl ChildProxy {
    pub fn start(generation: u64) -> std::io::Result<Self> {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let generation = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(generation));
        let closed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let routes = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let generation_thread = std::sync::Arc::clone(&generation);
        let closed_thread = std::sync::Arc::clone(&closed);
        let routes_thread = std::sync::Arc::clone(&routes);
        std::thread::spawn(move || {
            proxy_loop(listener, generation_thread, closed_thread, routes_thread)
        });
        Ok(Self {
            port,
            generation,
            closed,
            routes,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn insert(&self, owner: &str, route: ApprovedRoute, generation: u64) {
        self.routes
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(
                route.reference.clone(),
                LiveRoute {
                    route,
                    owner: owner.to_string(),
                    generation,
                },
            );
    }

    /// Bump the generation and close the listener. A cancellation request is
    /// not itself confirmation that a stream has finished.
    pub fn revoke(&self) -> u64 {
        let next = self
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            .saturating_add(1);
        self.closed
            .store(true, std::sync::atomic::Ordering::Release);
        let _ = std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, self.port));
        next
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::Acquire)
    }
}

fn proxy_loop(
    listener: std::net::TcpListener,
    generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    routes: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, LiveRoute>>>,
) {
    while !closed.load(std::sync::atomic::Ordering::Acquire) {
        let Ok((mut client, _)) = listener.accept() else {
            break;
        };
        if closed.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        let _ = serve_client(&mut client, &generation, &routes);
    }
}

fn serve_client(
    client: &mut std::net::TcpStream,
    generation: &std::sync::atomic::AtomicU64,
    routes: &std::sync::Mutex<std::collections::HashMap<String, LiveRoute>>,
) -> std::io::Result<()> {
    use std::io::{Read, Write};
    client.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let mut buffer = [0_u8; 8192];
    let n = client.read(&mut buffer)?;
    let text = String::from_utf8_lossy(&buffer[..n]);
    let reference = header(&text, "x-ca-destination");
    let owner = header(&text, "x-ca-owner");
    let live = generation.load(std::sync::atomic::Ordering::Acquire);
    let routes = routes.lock().unwrap_or_else(|poison| poison.into_inner());
    let Some(reference) = reference else {
        return write_status(client, 403);
    };
    let Some(live_route) = routes.get(&reference) else {
        return write_status(client, 403);
    };
    if owner.as_deref() != Some(live_route.owner.as_str()) || live_route.generation != live {
        return write_status(client, 403);
    }
    // The stored address was vetted at admission. Do not look up the hostname.
    let mut upstream = connect_admitted(&live_route.route)?;
    let forwarded = strip_private_headers(&text);
    upstream.write_all(forwarded.as_bytes())?;
    let mut response = Vec::new();
    upstream.read_to_end(&mut response)?;
    client.write_all(&response)?;
    Ok(())
}

fn header(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

fn strip_private_headers(text: &str) -> String {
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        let name = line.split(':').next().unwrap_or("");
        if name.trim().eq_ignore_ascii_case("x-ca-destination")
            || name.trim().eq_ignore_ascii_case("x-ca-owner")
        {
            continue;
        }
        out.push_str(line);
    }
    out
}

fn write_status(client: &mut std::net::TcpStream, status: u16) -> std::io::Result<()> {
    use std::io::Write;
    client.write_all(format!("HTTP/1.1 {status} denied\r\nContent-Length: 0\r\n\r\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn local_destination(port: u16) -> Destination {
        Destination {
            reference: "local-http".into(),
            protocol: "http".into(),
            hostname: "localhost".into(),
            port,
            classes: vec![AddressClass::Loopback],
            opaque_tunnel: false,
        }
    }

    #[test]
    fn admission_rejects_bypass_redirect_and_sibling_use() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let destination = local_destination(port);
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let route = admit(
            &destination,
            "local-http",
            "worker-a",
            "worker-a",
            "http",
            port,
            address,
            1,
            1,
        )
        .unwrap();
        let _stream = connect_admitted(&route).unwrap();
        assert_eq!(
            admit(
                &destination,
                "local-http",
                "worker-a",
                "worker-b",
                "http",
                port,
                address,
                1,
                1
            ),
            Err(BrokerError::SiblingReference)
        );
        assert_eq!(
            admit(
                &destination,
                "local-http",
                "worker-a",
                "worker-a",
                "http",
                port,
                SocketAddr::from((Ipv4Addr::LOCALHOST, port.wrapping_add(1))),
                1,
                1
            ),
            Err(BrokerError::Port)
        );
        assert_eq!(
            admit(
                &destination,
                "local-http",
                "worker-a",
                "worker-a",
                "https",
                port,
                address,
                1,
                1
            ),
            Err(BrokerError::Protocol)
        );
        assert_eq!(
            admit(
                &destination,
                "local-http",
                "worker-a",
                "worker-a",
                "http",
                port,
                address,
                1,
                2
            ),
            Err(BrokerError::Revoked)
        );
        assert!(!direct_udp_allowed());
        let public = Destination {
            classes: vec![AddressClass::Public],
            ..destination
        };
        assert_eq!(
            admit(
                &public,
                "local-http",
                "worker-a",
                "worker-a",
                "http",
                port,
                address,
                1,
                1
            ),
            Err(BrokerError::AddressClass)
        );
    }

    #[test]
    fn proxy_forwards_one_admitted_loopback_and_revoke_closes_it() {
        use std::io::{Read, Write};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let hits_thread = std::sync::Arc::clone(&hits);
        let upstream = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut socket, _)) = upstream.accept() {
                let mut buffer = [0_u8; 1024];
                let _ = socket.read(&mut buffer);
                hits_thread.fetch_add(1, Ordering::SeqCst);
                let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
            }
        });
        let destination = local_destination(upstream_port);
        let address = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, upstream_port));
        let route = admit(
            &destination,
            "local-http",
            "worker-a",
            "worker-a",
            "http",
            upstream_port,
            address,
            1,
            1,
        )
        .unwrap();
        let proxy = ChildProxy::start(1).unwrap();
        proxy.insert("worker-a", route, 1);
        let rules = proxy_only_rules(proxy.port());
        assert!(rules.contains(&format!("localhost:{}", proxy.port())));
        assert!(rules.contains("(deny network*)"));
        assert!(!direct_udp_allowed());
        let mut client = std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
        client
            .write_all(
                b"GET /once HTTP/1.1\r\nHost: localhost\r\nx-ca-destination: local-http\r\nx-ca-owner: worker-a\r\n\r\n",
            )
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains("200"));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let old_env = std::env::var("http_proxy").ok();
        std::env::set_var("http_proxy", "http://127.0.0.1:9");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "http_proxy must not open a second path"
        );
        if let Some(value) = old_env {
            std::env::set_var("http_proxy", value);
        } else {
            std::env::remove_var("http_proxy");
        }
        proxy.revoke();
        assert!(proxy.is_closed());
        let mut again = std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port()));
        if let Ok(socket) = again.as_mut() {
            let _ = socket.write_all(
                b"GET /again HTTP/1.1\r\nHost: localhost\r\nx-ca-destination: local-http\r\nx-ca-owner: worker-a\r\n\r\n",
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }
}
