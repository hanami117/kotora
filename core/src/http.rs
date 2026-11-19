//! One HTTP exchange, shared by the service clients (MyAnimeList: forms; AniList: GraphQL JSON).
//! Tests put a recorded [`Transport`] in its place, so they never call a service.

use std::time::Duration;

pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub bearer: Option<String>,
    /// A form-encoded body (POST, PATCH).
    pub form: Option<String>,
    /// A JSON body (POST; AniList's GraphQL). Wins over `form`.
    pub json: Option<String>,
}

pub struct Response {
    pub status: u16,
    /// `Retry-After`, in seconds.
    pub retry_after: Option<u64>,
    pub body: String,
}

/// One HTTP exchange. `Err` means no answer at all (DNS, connect, timeout).
pub trait Transport {
    fn send(&self, req: &Request) -> Result<Response, String>;
}

/// The real transport: blocking `ureq` on the caller's (worker) thread.
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .user_agent(concat!("Kotora/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        UreqTransport { agent }
    }
}

impl Transport for UreqTransport {
    fn send(&self, req: &Request) -> Result<Response, String> {
        let auth = req.bearer.as_ref().map(|b| format!("Bearer {b}"));
        let mut resp = match req.method {
            "POST" | "PATCH" => {
                let r = if req.method == "POST" { self.agent.post(&req.url) } else { self.agent.patch(&req.url) };
                let (r, body) = match &req.json {
                    Some(j) => (r.header("Content-Type", "application/json").header("Accept", "application/json"), j.clone()),
                    None => (r.header("Content-Type", "application/x-www-form-urlencoded"), req.form.clone().unwrap_or_default()),
                };
                let mut r = r;
                if let Some(a) = &auth {
                    r = r.header("Authorization", a);
                }
                r.send(body)
            }
            "DELETE" => {
                let mut r = self.agent.delete(&req.url);
                if let Some(a) = &auth {
                    r = r.header("Authorization", a);
                }
                r.call()
            }
            _ => {
                let mut r = self.agent.get(&req.url);
                if let Some(a) = &auth {
                    r = r.header("Authorization", a);
                }
                r.call()
            }
        }
        .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok());
        let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        Ok(Response { status, retry_after, body })
    }
}

/// Decodes `%XX` and `+` (a query string or a URL fragment); bad UTF-8 becomes U+FFFD.
pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if b.len() >= i + 3 => {
                if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
