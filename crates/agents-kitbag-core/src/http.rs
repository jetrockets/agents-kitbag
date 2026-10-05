//! Requests to the services' own APIs: one question, one answer.

use std::time::Duration;

/// How long a service gets to answer before a check gives up.
pub const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET",
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    pub fn post(url: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: "POST",
            url: url.into(),
            headers: Vec::new(),
            body: Some(body.into()),
        }
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    pub fn bearer(self, token: &str) -> Self {
        self.header("Authorization", format!("Bearer {token}"))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

pub trait Http: Send + Sync {
    /// The response, whatever its status. An error is a request that got no
    /// answer: no network, a timeout, a name that does not resolve.
    fn send(&self, request: &Request) -> Result<Response, String>;
}

#[cfg(feature = "net")]
pub use real::UreqHttp;

#[cfg(feature = "net")]
mod real {
    use std::sync::Arc;

    use super::{Http, Request, Response, TIMEOUT};

    /// The real client.
    pub struct UreqHttp {
        agent: ureq::Agent,
    }

    impl UreqHttp {
        pub fn new() -> Result<Self, String> {
            let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
            let agent = ureq::AgentBuilder::new()
                .timeout(TIMEOUT)
                .user_agent(concat!("agents-kitbag/", env!("CARGO_PKG_VERSION")))
                .tls_connector(Arc::new(tls))
                .build();
            Ok(Self { agent })
        }
    }

    impl Http for UreqHttp {
        fn send(&self, request: &Request) -> Result<Response, String> {
            let mut call = self.agent.request(request.method, &request.url);
            for (name, value) in &request.headers {
                call = call.set(name, value);
            }
            let result = match &request.body {
                Some(body) => call.send_string(body),
                None => call.call(),
            };
            // The address and how it answered, for `--verbose`. A token is
            // only ever in a header or the body, which are not logged.
            match result {
                Ok(response) | Err(ureq::Error::Status(_, response)) => {
                    log::debug!("{} {}: {}", request.method, request.url, response.status());
                    Ok(Response {
                        status: response.status(),
                        body: response.into_string().unwrap_or_default(),
                    })
                }
                Err(ureq::Error::Transport(transport)) => {
                    let reason = describe(&transport);
                    log::debug!("{} {}: {reason}", request.method, request.url);
                    Err(reason)
                }
            }
        }
    }

    fn describe(error: &ureq::Transport) -> String {
        match error.kind() {
            ureq::ErrorKind::Dns => "DNS lookup failed. Check your internet connection.".to_owned(),
            ureq::ErrorKind::ConnectionFailed => {
                "Connection refused. The server may be down.".to_owned()
            }
            ureq::ErrorKind::Io => format!(
                "No answer within {} seconds, or the connection dropped.",
                TIMEOUT.as_secs()
            ),
            kind => format!("Network error: {kind}"),
        }
    }
}
