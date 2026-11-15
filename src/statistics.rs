use crate::{components::Button, i18n::Locale, manager::Status, model::Instance, state::AppState};
use dioxus::prelude::*;
use std::{collections::BTreeMap, io::Read, time::Duration};

#[derive(Clone, Debug, Default)]
pub struct Endpoint {
    pub requests: u64,
    pub errors: u64,
    pub seconds: f64,
}
impl Endpoint {
    pub fn average_ms(&self) -> Option<f64> {
        (self.requests > 0).then(|| self.seconds / self.requests as f64 * 1000.0)
    }
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub total: Endpoint,
    pub in_flight: Option<f64>,
    pub endpoints: BTreeMap<String, Endpoint>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricsError {
    Connection,
    Authorization,
    Invalid,
}
impl MetricsError {
    fn key(self) -> &'static str {
        match self {
            Self::Authorization => "statistics_auth_error",
            Self::Connection => "statistics_unavailable",
            Self::Invalid => "statistics_invalid",
        }
    }
}

// Parse the quoted Prometheus label values rather than splitting on commas inside them.
fn labels(mut input: &str) -> Option<BTreeMap<String, String>> {
    let mut labels = BTreeMap::new();
    while !input.trim().is_empty() {
        input = input.trim_start();
        let (name, rest) = input.split_once('=')?;
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return None;
        }
        let rest = rest.strip_prefix('"')?;
        let mut value = String::new();
        let mut chars = rest.char_indices();
        let end = loop {
            let (position, character) = chars.next()?;
            match character {
                '"' => break position + 1,
                '\\' => value.push(match chars.next()?.1 {
                    'n' => '\n',
                    '\\' => '\\',
                    '"' => '"',
                    _ => return None,
                }),
                character => value.push(character),
            }
        };
        if labels.insert(name.to_string(), value).is_some() {
            return None;
        }
        input = &rest[end..];
        if !input.trim().is_empty() {
            input = input.trim_start().strip_prefix(',')?;
        }
    }
    Some(labels)
}

pub fn parse(text: &str) -> Result<Snapshot, MetricsError> {
    let mut snapshot = Snapshot::default();
    let mut found_count = false;
    let mut pairs = BTreeMap::<BTreeMap<String, String>, (bool, bool)>::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let name = line.split(['{', ' ', '\t']).next().unwrap_or_default();
        let kind = match name {
            "libretranslate_http_request_duration_seconds_count" => 0,
            "libretranslate_http_request_duration_seconds_sum" => 1,
            "libretranslate_http_requests_in_flight" => 2,
            _ => continue,
        };
        let rest = line[name.len()..].trim_start();
        let (labels, value) = if let Some(rest) = rest.strip_prefix('{') {
            let end = rest.rfind('}').ok_or(MetricsError::Invalid)?;
            (
                labels(&rest[..end]).ok_or(MetricsError::Invalid)?,
                rest[end + 1..].trim(),
            )
        } else {
            (BTreeMap::new(), rest)
        };
        let value: f64 = value
            .split_whitespace()
            .next()
            .ok_or(MetricsError::Invalid)?
            .parse()
            .map_err(|_| MetricsError::Invalid)?;
        if !value.is_finite() || value < 0.0 {
            return Err(MetricsError::Invalid);
        }
        if kind == 2 {
            snapshot.in_flight = Some(snapshot.in_flight.unwrap_or_default() + value);
            continue;
        }
        let pair = pairs.entry(labels.clone()).or_default();
        let seen = if kind == 0 { &mut pair.0 } else { &mut pair.1 };
        if *seen {
            return Err(MetricsError::Invalid);
        }
        *seen = true;
        let endpoint = labels
            .get("endpoint")
            .filter(|name| {
                name.starts_with('/') && name.len() <= 512 && !name.chars().any(char::is_control)
            })
            .ok_or(MetricsError::Invalid)?;
        let row = snapshot.endpoints.entry(endpoint.clone()).or_default();
        if kind == 1 {
            row.seconds += value;
            snapshot.total.seconds += value;
        } else {
            if value.fract() != 0.0 || value >= u64::MAX as f64 {
                return Err(MetricsError::Invalid);
            }
            let count = value as u64;
            let failed = labels
                .get("status")
                .and_then(|status| status.parse::<u16>().ok())
                .ok_or(MetricsError::Invalid)?
                >= 400;
            row.requests = row
                .requests
                .checked_add(count)
                .ok_or(MetricsError::Invalid)?;
            snapshot.total.requests = snapshot
                .total
                .requests
                .checked_add(count)
                .ok_or(MetricsError::Invalid)?;
            if failed {
                row.errors = row.errors.checked_add(count).ok_or(MetricsError::Invalid)?;
                snapshot.total.errors = snapshot
                    .total
                    .errors
                    .checked_add(count)
                    .ok_or(MetricsError::Invalid)?;
            }
            found_count = true;
        }
    }
    if !found_count
        || !snapshot.total.seconds.is_finite()
        || pairs.values().any(|pair| *pair != (true, true))
    {
        return Err(MetricsError::Invalid);
    }
    Ok(snapshot)
}

pub fn fetch(instance: &Instance) -> Result<Snapshot, MetricsError> {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(4))
        .build()
        .map_err(|_| MetricsError::Connection)?;
    let mut request = client.get(format!("{}/metrics", instance.url().trim_end_matches('/')));
    if !instance.server.metrics_token.is_empty() {
        request = request.bearer_auth(&instance.server.metrics_token);
    }
    let response = request.send().map_err(|_| MetricsError::Connection)?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err(MetricsError::Authorization);
    }
    if !response.status().is_success() {
        return Err(MetricsError::Connection);
    }
    let mut body = Vec::new();
    response
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut body)
        .map_err(|_| MetricsError::Connection)?;
    if body.len() > 2 * 1024 * 1024 {
        return Err(MetricsError::Invalid);
    }
    parse(std::str::from_utf8(&body).map_err(|_| MetricsError::Invalid)?)
}

fn duration(locale: Locale, value: Option<f64>) -> String {
    value
        .map(|value| {
            locale
                .text("statistics_ms")
                .replace("{value}", &format!("{value:.1}"))
        })
        .unwrap_or_else(|| "—".into())
}
#[component]
pub fn StatisticsPanel(instance: ReadSignal<Instance>, status: ReadSignal<Status>) -> Element {
    let locale = use_context::<Signal<AppState>>().read().locale;
    let mut refresh = use_signal(|| 0u32);
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            refresh.with_mut(|value| *value = value.wrapping_add(1));
        }
    });
    let snapshot = use_resource(move || {
        let instance = instance();
        let running = status() == Status::Running;
        let _ = refresh();
        async move {
            if !running {
                return Err(MetricsError::Connection);
            }
            tokio::task::spawn_blocking(move || fetch(&instance))
                .await
                .unwrap_or(Err(MetricsError::Connection))
        }
    });
    rsx! {section {class:"statistics-panel",
        div {class:"statistics-heading",h2 {{locale.text("statistics")}} Button{class:"secondary",disabled:snapshot.pending()||status()!=Status::Running,onclick:move |_|refresh.with_mut(|value|*value=value.wrapping_add(1)),{locale.text("refresh_statistics")}}}
        if status()!=Status::Running {p {class:"field-hint",{locale.text("statistics_start_server")}}}
        else {match &*snapshot.read() {
            None=>rsx!{p{class:"field-hint",{locale.text("statistics_loading")}}},
            Some(Err(error))=>rsx!{p{class:"field-error",role:"status",{locale.text(error.key())}}},
            Some(Ok(data))=>rsx!{
                p{class:"field-hint",{locale.text("statistics_scope")}}
                dl {class:"statistics-summary",
                    div {dt {{locale.text("statistics_requests")}}dd {"{data.total.requests}"}}
                    div {dt {{locale.text("statistics_errors")}}dd {"{data.total.errors}"}}
                    div {dt {{locale.text("statistics_average")}}dd {{duration(locale,data.total.average_ms())}}}
                    div {dt {{locale.text("statistics_in_flight")}}dd {{data.in_flight.map(|value|format!("{value:.0}")).unwrap_or_else(||"—".into())}}}
                }
                div {class:"statistics-table-scroll",table {class:"statistics-table",
                    thead {tr {th{scope:"col",{locale.text("statistics_endpoint")}}th{scope:"col",{locale.text("statistics_requests")}}th{scope:"col",{locale.text("statistics_errors")}}th{scope:"col",{locale.text("statistics_average")}}}}
                    tbody {for (endpoint,row) in &data.endpoints {tr {key:"{endpoint}",td {code {"{endpoint}"}}td {bdi {dir:"ltr","{row.requests}"}}td {bdi {dir:"ltr","{row.errors}"}}td {bdi {dir:"ltr",{duration(locale,row.average_ms())}}}}}}
                }}
            },
        }}
    }}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregates_clients_statuses_and_endpoints_without_exposing_identity_labels() {
        let data=parse(r#"
# TYPE libretranslate_http_request_duration_seconds summary
libretranslate_http_request_duration_seconds_count{endpoint="/prefix/translate",status="200",request_ip="127.0.0.1",api_key="a,\"b"} 8
libretranslate_http_request_duration_seconds_sum{endpoint="/prefix/translate",status="200",request_ip="127.0.0.1",api_key="a,\"b"} 2
libretranslate_http_request_duration_seconds_count{endpoint="/prefix/translate",status="500",request_ip="127.0.0.2",api_key=""} 2
libretranslate_http_request_duration_seconds_sum{endpoint="/prefix/translate",status="500",request_ip="127.0.0.2",api_key=""} 1
libretranslate_http_request_duration_seconds_count{endpoint="/detect",status="200"} 1
libretranslate_http_request_duration_seconds_sum{endpoint="/detect",status="200"} 0.3
libretranslate_http_requests_in_flight{endpoint="/prefix/translate"} 2
libretranslate_http_requests_in_flight{endpoint="/detect"} 1
ignored_metric 999
"#).unwrap();
        assert_eq!(data.total.requests, 11);
        assert_eq!(data.total.errors, 2);
        assert_eq!(data.in_flight, Some(3.0));
        assert!((data.endpoints["/prefix/translate"].average_ms().unwrap() - 300.0).abs() < 0.001);
        assert_eq!(data.endpoints.len(), 2);
    }
    #[test]
    fn empty_and_corrupt_metrics_are_unavailable_instead_of_zero() {
        assert_eq!(parse("# no samples").unwrap_err(), MetricsError::Invalid);
        for value in ["NaN", "+Inf", "-2", "1.2"] {
            assert_eq!(parse(&format!("libretranslate_http_request_duration_seconds_count{{endpoint=\"/translate\",status=\"200\"}} {value}")).unwrap_err(),MetricsError::Invalid);
        }
        let zero=parse("libretranslate_http_request_duration_seconds_count{endpoint=\"/translate\",status=\"200\"} 0\nlibretranslate_http_request_duration_seconds_sum{endpoint=\"/translate\",status=\"200\"} 0").unwrap();
        assert_eq!(zero.total.requests, 0);
        assert!(zero.total.average_ms().is_none());
        assert!(zero.in_flight.is_none());
    }
    #[test]
    fn incomplete_or_mismatched_series_are_rejected() {
        let count = "libretranslate_http_request_duration_seconds_count{endpoint=\"/translate\",status=\"200\",api_key=\"a\"} 4";
        for sum in [
            "",
            "libretranslate_http_request_duration_seconds_sum{endpoint=\"/other\",status=\"200\",api_key=\"a\"} 8",
            "libretranslate_http_request_duration_seconds_sum{endpoint=\"/translate\",status=\"200\",api_key=\"b\"} 8",
        ] {
            assert_eq!(
                parse(&format!("{count}\n{sum}")).unwrap_err(),
                MetricsError::Invalid
            );
        }
    }
}
