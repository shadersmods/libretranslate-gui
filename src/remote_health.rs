use futures_util::StreamExt;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum ProbeOutcome {
    Online(u64),
    Offline,
    Timeout,
    Invalid,
    Http(u16),
}

// Shared by public candidate checks and saved remote-instance polling.
pub async fn check(client: &reqwest::Client, url: &str, timeout: Duration) -> ProbeOutcome {
    let url = url.trim_end_matches('/');
    let started = Instant::now();
    let outcome = tokio::time::timeout(timeout, async {
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
