//! The HTTP interface of the connector, and the real client.
//!
//! The functions of this crate use `Http`. The app gives them a `Client`. The tests give them a fake client.

use std::fmt;
use std::time::Duration;

use panim_core::{ProxyKind, Route};

/// The time limit to connect to the server or to the proxy.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// The time limit for one full request.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(30);
/// The client refuses a body that is larger than this number of bytes.
const MAX_BODY: u64 = 4 * 1024 * 1024;

/// The method of a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
}

impl Method {
    pub fn name(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Patch => "PATCH",
        }
    }
}

/// A request.
///
/// The `Debug` form shows only the method and the URL. A header or a body can contain a secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl Request {
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Adds a header.
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_string(), value.into()));
        self
    }

    /// The value of a header. The name can be in uppercase or lowercase.
    pub fn header_value(&self, name: &str) -> Option<&str> {
        find_header(&self.headers, name)
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Request({} {})", self.method.name(), self.url)
    }
}

/// A response.
///
/// The `Debug` form shows only the status. The body of a login response contains the token.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    /// The value of a header. The name can be in uppercase or lowercase.
    pub fn header(&self, name: &str) -> Option<&str> {
        find_header(&self.headers, name)
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Response({})", self.status)
    }
}

fn find_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// The request did not get a response: no connection, a TLS error or a time limit.
///
/// The text comes from the client. It does not contain a header or a body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpError(pub String);

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HttpError {}

/// Sends requests. A response with an error status is a correct result, not an `HttpError`.
pub trait Http: Send + Sync {
    fn send(&self, request: Request) -> Result<Response, HttpError>;
}

/// The real client. It sends each request on the route of the persona.
pub struct Client {
    agent: ureq::Agent,
}

impl Client {
    /// Makes a client for a route.
    ///
    /// With a SOCKS5 proxy, the proxy gets the host name, so the DNS lookup does not leak. With an HTTP proxy,
    /// the client uses CONNECT. The client accepts only `https` URLs and does not follow a redirect.
    pub fn new(route: &Route) -> Result<Self, HttpError> {
        let proxy = match route {
            Route::Direct => None,
            Route::Proxy(endpoint) => {
                let protocol = match endpoint.kind {
                    ProxyKind::Http => ureq::ProxyProtocol::Http,
                    ProxyKind::Socks5 => ureq::ProxyProtocol::Socks5h,
                };
                let proxy = ureq::Proxy::builder(protocol)
                    .host(&endpoint.host)
                    .port(endpoint.port)
                    .resolve_target(false)
                    .build()
                    .map_err(|e| HttpError(format!("the proxy is not correct: {e}")))?;
                Some(proxy)
            }
        };
        let config = ureq::Agent::config_builder()
            .proxy(proxy)
            .https_only(true)
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(TOTAL_TIMEOUT))
            .build();
        Ok(Self {
            agent: ureq::Agent::new_with_config(config),
        })
    }
}

impl Http for Client {
    fn send(&self, request: Request) -> Result<Response, HttpError> {
        let mut builder = ureq::http::Request::builder()
            .method(request.method.name())
            .uri(&request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let body = request.body.unwrap_or_default();
        let request = builder.body(body).map_err(|e| HttpError(e.to_string()))?;
        let mut response = self.agent.run(request).map_err(|e| HttpError(e.to_string()))?;
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_string())))
            .collect();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_vec()
            .map_err(|e| HttpError(e.to_string()))?;
        Ok(Response {
            status: response.status().as_u16(),
            headers,
            body,
        })
    }
}

/// A fake client for tests. It gives recorded responses in sequence and keeps the requests.
#[cfg(any(test, feature = "fake"))]
pub mod fake {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::{Http, HttpError, Request, Response};

    #[derive(Default)]
    pub struct FakeHttp {
        answers: Mutex<VecDeque<Result<Response, HttpError>>>,
        requests: Mutex<Vec<Request>>,
    }

    impl FakeHttp {
        pub fn new() -> Self {
            Self::default()
        }

        /// Adds a response with a JSON body.
        pub fn json(&self, status: u16, body: &str) -> &Self {
            self.response(status, &[], body)
        }

        /// Adds a response with headers and a body.
        pub fn response(&self, status: u16, headers: &[(&str, &str)], body: &str) -> &Self {
            let headers = headers
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect();
            self.answers.lock().unwrap().push_back(Ok(Response {
                status,
                headers,
                body: body.as_bytes().to_vec(),
            }));
            self
        }

        /// Adds a network failure.
        pub fn failure(&self, reason: &str) -> &Self {
            self.answers
                .lock()
                .unwrap()
                .push_back(Err(HttpError(reason.to_string())));
            self
        }

        /// The requests that the client got, in sequence. The call removes them from the client.
        pub fn take_requests(&self) -> Vec<Request> {
            std::mem::take(&mut self.requests.lock().unwrap())
        }

        /// The number of responses that no request used.
        pub fn answers_left(&self) -> usize {
            self.answers.lock().unwrap().len()
        }
    }

    impl Http for FakeHttp {
        fn send(&self, request: Request) -> Result<Response, HttpError> {
            self.requests.lock().unwrap().push(request);
            self.answers
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(HttpError("the fake client has no more responses".into())))
        }
    }
}

#[cfg(test)]
mod tests {
    //! These tests examine the real client with a fake proxy on 127.0.0.1. They use no other network.

    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    use panim_core::{ProxyEndpoint, ProxyKind, Route};

    use super::{Client, Http, Method, Request};

    /// A fake proxy on a free port of 127.0.0.1.
    fn proxy(kind: ProxyKind) -> (TcpListener, Route) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let route = Route::Proxy(ProxyEndpoint {
            kind,
            host: "127.0.0.1".into(),
            port,
        });
        (listener, route)
    }

    /// Sends a request with a token on a different thread. The fake proxy does not complete the request.
    fn send(route: Route, url: &str) -> thread::JoinHandle<bool> {
        let request = Request::new(Method::Get, url).header("Authorization", "Bearer gho_secret");
        thread::spawn(move || Client::new(&route).unwrap().send(request).is_err())
    }

    fn accept(listener: &TcpListener) -> TcpStream {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        stream
    }

    #[test]
    fn http_proxy_gets_connect_and_no_token() {
        let (listener, route) = proxy(ProxyKind::Http);
        let sent = send(route, "https://api.github.com/user");
        let mut stream = accept(&listener);
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && head.len() < 4096 {
            stream.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).unwrap();
        assert!(head.starts_with("CONNECT api.github.com:443 "), "{head}");
        assert!(!head.contains("gho_secret"), "the token goes only inside TLS");
        drop(stream);
        assert!(sent.join().unwrap());
    }

    #[test]
    fn socks5_proxy_gets_the_host_name() {
        let (listener, route) = proxy(ProxyKind::Socks5);
        let sent = send(route, "https://api.github.com/user");
        let mut stream = accept(&listener);
        let mut greeting = [0u8; 2];
        stream.read_exact(&mut greeting).unwrap();
        assert_eq!(greeting[0], 5, "SOCKS version 5");
        let mut methods = vec![0u8; greeting[1] as usize];
        stream.read_exact(&mut methods).unwrap();
        assert!(methods.contains(&0), "no authentication");
        stream.write_all(&[5, 0]).unwrap();

        let mut head = [0u8; 5];
        stream.read_exact(&mut head).unwrap();
        // CONNECT with a domain name (3), not an IP address. Thus, the DNS lookup does not leak.
        assert_eq!(&head[..4], &[5, 1, 0, 3]);
        let mut rest = vec![0u8; head[4] as usize + 2];
        stream.read_exact(&mut rest).unwrap();
        let (host, port) = rest.split_at(head[4] as usize);
        assert_eq!(host, b"api.github.com");
        assert_eq!(port, &443u16.to_be_bytes());
        drop(stream);
        assert!(sent.join().unwrap());
    }

    #[test]
    fn plain_http_is_refused_before_a_connection() {
        let (listener, route) = proxy(ProxyKind::Http);
        assert!(send(route, "http://api.github.com/user").join().unwrap());
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().map(|_| ()).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "the client made no connection"
        );
    }
}
