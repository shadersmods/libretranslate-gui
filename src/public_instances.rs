use anyhow::{Result, ensure};
use futures_util::StreamExt;
use serde::Deserialize;
use std::{
    collections::HashSet,
    io::Read,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};

pub const SOURCE: &str = "https://raw.githubusercontent.com/shadersmods/libretranslate-remote-database/main/data/open.json";
const MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Candidate {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub country: Option<Country>,
    #[serde(default)]
    pub languages: Option<Vec<Language>>,
    #[serde(default)]
    pub supported_files: Option<Vec<String>>,
    #[serde(default)]
    pub files_translation: Option<bool>,
    #[serde(default)]
    pub character_limit: Option<i64>,
    #[serde(default)]
    pub checked_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Country {
    pub code: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Language {
    pub code: String,
}

impl Candidate {
    pub fn language_codes(&self) -> Option<Vec<String>> {
        let values = self.languages.as_ref()?;
        if values.len() > 500 {
            return None;
        }
        let mut codes = values
            .iter()
            .filter(|item| {
                !item.code.is_empty()
                    && item.code.len() <= 16
                    && item
                        .code
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
            .map(|item| item.code.clone())
            .collect::<Vec<_>>();
        codes.sort();
        codes.dedup();
        Some(codes)
    }

    pub fn file_formats(&self) -> Option<Vec<String>> {
        if self.files_translation == Some(false) {
            return Some(Vec::new());
        }
        let values = self.supported_files.as_ref()?;
        if values.len() > 100 {
            return None;
        }
        let mut formats = values
            .iter()
            .filter(|value| {
                let suffix = value.trim_start_matches('.');
                !suffix.is_empty()
                    && suffix.len() <= 12
                    && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
            .map(|value| format!(".{}", value.trim_start_matches('.').to_ascii_lowercase()))
            .collect::<Vec<_>>();
        formats.sort();
        formats.dedup();
        Some(formats)
    }

    pub fn apply(
        &self,
        draft: &mut crate::model::Instance,
        default_name: &str,
        previous_name: Option<&str>,
    ) {
        if draft.kind != crate::model::InstanceKind::Remote {
            return;
        }
        if draft.name.trim().is_empty()
            || draft.name == default_name
            || draft.name == "Remote translator"
            || previous_name.is_some_and(|name| name.chars().take(64).eq(draft.name.chars()))
        {
            draft.name = self.name.chars().take(64).collect();
        }
        if draft.remote_url != self.url {
            draft.server.api_key.clear();
        }
        draft.remote_url.clone_from(&self.url);
    }
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || a == 0
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 198 && (18..=19).contains(&b))
                || (a == 192 && b == 0))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            segments[0] & 0xe000 == 0x2000
                && segments[..2] != [0x2001, 0x0db8]
                && segments[..2] != [0x2001, 0]
                && segments[0] != 0x2002
        }
    }
}

struct PublicResolver;
impl reqwest::dns::Resolve for PublicResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let mut addresses = tokio::net::lookup_host((name.as_str(), 0))
                .await?
                .collect::<Vec<_>>();
            if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Non-public server address",
                )
                .into());
            }
            addresses.sort_by_key(|address| !address.is_ipv4());
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProbeOutcome {
    Online(u64),
    Offline,
    Timeout,
    Invalid,
    Http(u16),
}

pub fn probe_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .dns_resolver(Arc::new(PublicResolver))
        .no_proxy()
        .user_agent("LibreTranslate-GUI")
        .build()?)
}

pub async fn probe_all(
    candidates: Vec<Candidate>,
    mut on_result: impl FnMut(String, ProbeOutcome),
) {
    let Ok(client) = probe_client() else {
        for candidate in candidates {
            on_result(candidate.url, ProbeOutcome::Offline);
        }
        return;
    };
    probe_many(client, candidates, on_result).await;
}

async fn probe_many(
    client: reqwest::Client,
    candidates: Vec<Candidate>,
    mut on_result: impl FnMut(String, ProbeOutcome),
) {
    let mut requests = futures_util::stream::iter(candidates.into_iter().map(|candidate| {
        let client = client.clone();
        async move {
            let outcome = probe(&client, &candidate).await;
            (candidate.url, outcome)
        }
    }))
    .buffer_unordered(4);
    while let Some((url, outcome)) = requests.next().await {
        on_result(url, outcome);
    }
}

pub async fn probe(client: &reqwest::Client, candidate: &Candidate) -> ProbeOutcome {
    let Some(url) = public_url(&candidate.url) else {
        return ProbeOutcome::Offline;
    };
    let started = Instant::now();
    let outcome = tokio::time::timeout(Duration::from_secs(8), async {
        let health = probe_endpoint(client, &format!("{url}/health"), false).await;
        if matches!(health, ProbeOutcome::Http(404 | 405)) {
            probe_endpoint(client, &format!("{url}/languages"), true).await
        } else {
            health
        }
    })
    .await
    .unwrap_or(ProbeOutcome::Timeout);
    if matches!(outcome, ProbeOutcome::Online(_)) {
        ProbeOutcome::Online(started.elapsed().as_millis().min(u64::MAX as u128) as u64)
    } else {
        outcome
    }
}

async fn probe_endpoint(client: &reqwest::Client, address: &str, languages: bool) -> ProbeOutcome {
    let started = Instant::now();
    let reply = match client
        .get(address)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
    {
        Ok(reply) => reply,
        Err(error) => {
            return if error.is_timeout() {
                ProbeOutcome::Timeout
            } else {
                ProbeOutcome::Offline
            };
        }
    };
    if !reply.status().is_success() {
        return ProbeOutcome::Http(reply.status().as_u16());
    }
    let mut bytes = Vec::new();
    let mut stream = reply.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                return if error.is_timeout() {
                    ProbeOutcome::Timeout
                } else {
                    ProbeOutcome::Offline
                };
            }
        };
        if bytes.len() + chunk.len() > if languages { 1024 * 1024 } else { 32 * 1024 } {
            return ProbeOutcome::Invalid;
        }
        bytes.extend_from_slice(&chunk);
    }
    if serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .is_some_and(|body| {
            if languages {
                body.as_array().is_some_and(|items| {
                    !items.is_empty()
                        && items.len() <= 500
                        && items.iter().all(|item| {
                            item["code"].as_str().is_some_and(|code| {
                                !code.is_empty()
                                    && code.len() <= 16
                                    && code
                                        .bytes()
                                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                            })
                        })
                })
            } else {
                body["status"].as_str() == Some("ok")
            }
        })
    {
        ProbeOutcome::Online(started.elapsed().as_millis().min(u64::MAX as u128) as u64)
    } else {
        ProbeOutcome::Invalid
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Catalog {
    pub instances: Vec<Candidate>,
}

fn public_url(value: &str) -> Option<String> {
    if value
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return None;
    }
    let url = reqwest::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let host = url
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']');
    match host.parse::<IpAddr>() {
        Err(_) => {
            if host == "localhost"
                || host.ends_with(".localhost")
                || host.ends_with(".local")
                || !host.contains('.')
            {
                return None;
            }
        }
        Ok(ip) if !public_ip(ip) => return None,
        Ok(_) => {}
    }
    Some(url.as_str().trim_end_matches('/').to_owned())
}

fn parse(bytes: &[u8]) -> Result<Catalog> {
    #[derive(Deserialize)]
    struct Feed {
        schema_version: u32,
        instances: Vec<serde_json::Value>,
    }
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Public instance list is too large"
    );
    let feed: Feed = serde_json::from_slice(bytes)?;
    ensure!(
        feed.schema_version == 1,
        "Unsupported public instance list version"
    );
    ensure!(feed.instances.len() <= 1000, "Too many public instances");
    let mut seen = HashSet::new();
    let mut instances = feed
        .instances
        .into_iter()
        .filter_map(|value| {
            let mut candidate: Candidate = serde_json::from_value(value).ok()?;
            candidate.name = candidate.name.trim().to_owned();
            if candidate.name.is_empty()
                || candidate.name.chars().count() > 100
                || candidate.name.chars().any(char::is_control)
            {
                return None;
            }
            candidate.url = public_url(&candidate.url)?;
            seen.insert(candidate.url.clone()).then_some(candidate)
        })
        .collect::<Vec<_>>();
    instances.sort_by_cached_key(|item| item.name.to_lowercase());
    Ok(Catalog { instances })
}

pub fn fetch() -> Result<Catalog> {
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("LibreTranslate-GUI")
        .build()?
        .get(SOURCE)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()?;
    ensure!(
        response.status().is_success(),
        "Could not download public instance list"
    );
    let mut bytes = Vec::new();
    response.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    parse(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Instance;

    #[test]
    fn reads_the_published_shape_and_preserves_prefixes_ports_and_http() {
        let catalog = parse(br#"{"schema_version":1,"generated_at":"2026-11-16T12:00:00Z","instances":[
            {"id":"first","name":"Zulu","url":"https://example.org/lt/","languages":[{"code":"en"}],"country":null},
            {"name":"Alpha","url":"http://186.248.219.194:5001/","character_limit":-1}
        ]}"#).unwrap();
        assert_eq!(catalog.instances[0].url, "http://186.248.219.194:5001");
        assert_eq!(catalog.instances[1].url, "https://example.org/lt");
    }

    #[test]
    fn retains_capabilities_and_distinguishes_disabled_unknown_and_unlimited() {
        let catalog = parse(br#"{"schema_version":1,"instances":[{"name":"Example","url":"https://example.org",
            "country":{"code":"DE","name":"Germany"},"languages":[{"code":"es"},{"code":"en"},{"code":"en"},{"code":"bad code"}],
            "supported_files":[".TXT","pdf",".txt","<script>"],"files_translation":true,"character_limit":-1,
            "checked_at":"2026-11-16T12:00:00.000Z"}]}"#).unwrap();
        let item = &catalog.instances[0];
        assert_eq!(item.country.as_ref().unwrap().code, "DE");
        assert_eq!(item.language_codes().unwrap(), ["en", "es"]);
        assert_eq!(item.file_formats().unwrap(), [".pdf", ".txt"]);
        assert_eq!(item.character_limit, Some(-1));
        assert!(item.checked_at.is_some());
        let disabled = Candidate {
            files_translation: Some(false),
            ..item.clone()
        };
        assert_eq!(disabled.file_formats(), Some(Vec::new()));
        let unknown = Candidate::default();
        assert_eq!(unknown.language_codes(), None);
        assert_eq!(unknown.file_formats(), None);
        assert_eq!(unknown.character_limit, None);
    }

    fn run_async<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn health_fixture(
        status: u16,
        body: Vec<u8>,
        stall: bool,
    ) -> (Candidate, reqwest::Client, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("Fixture connection failed: {error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 2048];
            while !request.windows(4).any(|value| value == b"\r\n\r\n") {
                let size = socket.read(&mut chunk).unwrap();
                if size == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..size]);
            }
            let headers = format!(
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\nLocation: http://127.0.0.1:1/private\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(headers.as_bytes());
            if stall {
                std::thread::sleep(Duration::from_millis(200));
            } else {
                let _ = socket.write_all(&body);
            }
            String::from_utf8(request).unwrap()
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .resolve("public.example", address)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(if stall {
                Duration::from_millis(100)
            } else {
                Duration::from_secs(2)
            })
            .build()
            .unwrap();
        (
            Candidate {
                name: "Fixture".into(),
                url: format!("http://public.example:{}/prefix", address.port()),
                ..Candidate::default()
            },
            client,
            worker,
        )
    }

    #[test]
    fn probes_health_with_prefix_and_without_sending_keys_or_translation_text() {
        let (candidate, client, worker) =
            health_fixture(200, br#"{"status":"ok"}"#.to_vec(), false);
        assert!(matches!(
            run_async(probe(&client, &candidate)),
            ProbeOutcome::Online(_)
        ));
        let request = worker.join().unwrap();
        assert!(request.starts_with("GET /prefix/health HTTP/1.1\r\n"));
        assert!(!request.to_ascii_lowercase().contains("authorization"));
        assert!(!request.contains("api_key"));
    }

    #[test]
    fn servers_without_health_can_be_verified_through_the_languages_api() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let worker = std::thread::spawn(move || {
            let mut paths = Vec::new();
            for (status, body) in [(404, "{}"), (200, r#"[{"code":"en"},{"code":"es"}]"#)] {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(2))
                        }
                        Err(error) => panic!("Fixture connection failed: {error}"),
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut request = Vec::new();
                let mut bytes = [0; 1024];
                while !request.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
                    let size = socket.read(&mut bytes).unwrap();
                    if size == 0 {
                        break;
                    }
                    request.extend_from_slice(&bytes[..size]);
                }
                paths.push(
                    String::from_utf8(request)
                        .unwrap()
                        .lines()
                        .next()
                        .unwrap()
                        .to_string(),
                );
                let reply = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(reply.as_bytes()).unwrap();
            }
            paths
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .resolve("public.example", address)
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let candidate = Candidate {
            url: format!("http://public.example:{}/prefix", address.port()),
            ..Candidate::default()
        };
        assert!(matches!(
            run_async(probe(&client, &candidate)),
            ProbeOutcome::Online(_)
        ));
        assert_eq!(
            worker.join().unwrap(),
            [
                "GET /prefix/health HTTP/1.1",
                "GET /prefix/languages HTTP/1.1"
            ]
        );
    }

    #[test]
    fn probes_reject_login_pages_oversized_bodies_http_errors_and_redirects() {
        for (status, body, expected) in [
            (200, b"<html>Login</html>".to_vec(), ProbeOutcome::Invalid),
            (200, vec![b' '; 32 * 1024 + 1], ProbeOutcome::Invalid),
            (403, b"{}".to_vec(), ProbeOutcome::Http(403)),
            (429, b"{}".to_vec(), ProbeOutcome::Http(429)),
            (503, b"{}".to_vec(), ProbeOutcome::Http(503)),
            (302, b"{}".to_vec(), ProbeOutcome::Http(302)),
        ] {
            let (candidate, client, worker) = health_fixture(status, body, false);
            assert_eq!(run_async(probe(&client, &candidate)), expected);
            worker.join().unwrap();
        }
    }

    #[test]
    fn probes_time_out_on_stalled_bodies_and_reject_private_destinations() {
        let (candidate, client, worker) = health_fixture(200, b"{}".to_vec(), true);
        assert_eq!(run_async(probe(&client, &candidate)), ProbeOutcome::Timeout);
        worker.join().unwrap();
        let invalid = Candidate {
            url: "http://127.0.0.1/private".into(),
            ..Candidate::default()
        };
        assert_eq!(run_async(probe(&client, &invalid)), ProbeOutcome::Offline);
        use reqwest::dns::Resolve;
        assert!(run_async(PublicResolver.resolve("localhost".parse().unwrap())).is_err());
    }

    #[test]
    fn invalid_rows_and_duplicate_normalized_urls_do_not_break_the_list() {
        let catalog = parse(
            br#"{"schema_version":1,"instances":[
            {"name":"Valid","url":"https://example.org/"},
            {"name":"Duplicate","url":"https://EXAMPLE.org"},
            {"name":"","url":"https://empty.example.org"},
            {"name":123,"url":"https://bad.example.org"},
            {"name":"Bad\nname","url":"https://bad.example.org"}
        ]}"#,
        )
        .unwrap();
        assert_eq!(catalog.instances.len(), 1);
    }

    #[test]
    fn feed_candidates_cannot_supply_credentials_private_addresses_or_other_schemes() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://user:secret@example.org",
            "https://example.org?api_key=secret",
            "https://example.org/#x",
            "https://localhost",
            "http://127.0.0.1",
            "http://2130706433",
            "http://192.168.1.1",
            "http://100.64.1.1",
            "https://[::1]",
            "https://[::ffff:127.0.0.1]",
            "https://[2001:db8::1]",
            "https://demo.local",
            " https://example.org",
            "https://example.org\\private",
        ] {
            assert!(public_url(url).is_none(), "{url}");
        }
    }

    #[test]
    fn rejects_invalid_envelopes_and_accepts_an_empty_list() {
        for bytes in [
            b"<html>Error</html>".as_slice(),
            br#"{"schema_version":2,"instances":[]}"#,
            br#"{"schema_version":1,"instances":{}}"#,
        ] {
            assert!(parse(bytes).is_err());
        }
        assert!(
            parse(br#"{"schema_version":1,"instances":[]}"#)
                .unwrap()
                .instances
                .is_empty()
        );
        assert!(parse(&vec![b' '; MAX_BYTES as usize + 1]).is_err());
        let too_many =
            serde_json::json!({"schema_version":1,"instances":vec![serde_json::Value::Null;1001]});
        assert!(parse(&serde_json::to_vec(&too_many).unwrap()).is_err());
    }

    #[test]
    fn choosing_a_candidate_preserves_identity_custom_names_and_health_settings() {
        let mut draft = Instance::remote();
        draft.name = "My translator".into();
        draft.server.api_key = "old-server-key".into();
        let before = draft.clone();
        let candidate = Candidate {
            name: "Public server".into(),
            url: "https://example.org/lt".into(),
            ..Candidate::default()
        };
        candidate.apply(&mut draft, "Remote server", None);
        assert_eq!(draft.id, before.id);
        assert_eq!(draft.health, before.health);
        assert_eq!(draft.name, "My translator");
        assert_eq!(draft.remote_url, candidate.url);
        assert!(draft.server.api_key.is_empty());
        assert!(draft.validate(&[]).is_ok());
    }

    #[test]
    fn default_and_previous_candidate_names_follow_the_selection() {
        let mut draft = Instance::remote();
        let candidate = Candidate {
            name: "New public server".into(),
            url: "https://example.org".into(),
            ..Candidate::default()
        };
        candidate.apply(&mut draft, "Remote server", None);
        assert_eq!(draft.name, candidate.name);
        let next = Candidate {
            name: "Second public server".into(),
            url: "https://second.example.org".into(),
            ..Candidate::default()
        };
        next.apply(&mut draft, "Remote server", Some(&candidate.name));
        assert_eq!(draft.name, next.name);
        let mut local = Instance::default();
        candidate.apply(&mut local, "Remote server", None);
        assert!(local.remote_url.is_empty());
    }
}
