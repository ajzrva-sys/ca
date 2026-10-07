use futures::{stream, StreamExt};
use serde::Deserialize;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(100);
const REQUEST_TIMEOUT: Duration = Duration::from_millis(400);
const MAX_CONCURRENT_SCANS: usize = 32;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_MODEL_COUNT: usize = 256;
const MAX_MODEL_ID_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalProviderCandidate {
    pub provider_id: String,
    pub port: u16,
    pub base_url: String,
    pub models: Vec<String>,
}

#[derive(Deserialize)]
struct ModelListResponse {
    data: Vec<ModelObject>,
}

#[derive(Deserialize)]
struct ModelObject {
    id: String,
}

pub fn discovery_ports() -> Vec<u16> {
    let mut ports = vec![8000, 11434, 1234];
    ports.extend(8080..=8179);
    ports.sort_unstable();
    ports.dedup();
    ports
}

pub async fn discover_local_providers() -> Result<Vec<LocalProviderCandidate>, String> {
    discover_ports(&discovery_ports()).await
}

async fn discover_ports(ports: &[u16]) -> Result<Vec<LocalProviderCandidate>, String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| "could not prepare local provider discovery".to_string())?;

    let requests = stream::iter(ports.iter().copied()).map(|port| {
        let client = client.clone();
        async move { scan_port(&client, port).await }
    });
    let mut candidates: Vec<_> = requests
        .buffer_unordered(MAX_CONCURRENT_SCANS)
        .filter_map(|candidate| async move { candidate })
        .collect()
        .await;
    candidates.sort_by_key(|candidate| candidate.port);
    Ok(candidates)
}

async fn scan_port(client: &reqwest::Client, port: u16) -> Option<LocalProviderCandidate> {
    let base_url = format!("http://127.0.0.1:{port}/v1");
    let response = client.get(format!("{base_url}/models")).send().await.ok()?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        || !safe_json_content_type(response.headers())
    {
        return None;
    }

    let mut body = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.ok()?;
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    let decoded: ModelListResponse = serde_json::from_slice(&body).ok()?;
    let mut models: Vec<_> = decoded
        .data
        .into_iter()
        .map(|model| model.id)
        .filter(|id| valid_model_id(id))
        .take(MAX_MODEL_COUNT)
        .collect();
    models.sort();
    models.dedup();
    if models.is_empty() {
        return None;
    }

    Some(LocalProviderCandidate {
        provider_id: format!("local-{port}"),
        port,
        base_url,
        models,
    })
}

fn safe_json_content_type(headers: &reqwest::header::HeaderMap) -> bool {
    let Some(value) = headers.get(reqwest::header::CONTENT_TYPE) else {
        return true;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let media_type = value.split(';').next().unwrap_or_default().trim();
    media_type.eq_ignore_ascii_case("application/json") || media_type.ends_with("+json")
}

fn valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_MODEL_ID_BYTES
        && id.trim() == id
        && !id.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn server(
        response: Vec<u8>,
    ) -> (u16, Arc<Mutex<Option<String>>>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let request = Arc::new(Mutex::new(None));
        let captured = request.clone();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 8192];
            let count = socket.read(&mut bytes).await.unwrap();
            bytes.truncate(count);
            *captured.lock().unwrap() = Some(String::from_utf8_lossy(&bytes).into_owned());
            socket.write_all(&response).await.unwrap();
        });
        (port, request, handle)
    }

    fn json_response(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn production_ports_are_exact_and_unique() {
        let ports = discovery_ports();
        let expected: Vec<_> = std::iter::once(1234)
            .chain(std::iter::once(8000))
            .chain(8080..=8179)
            .chain(std::iter::once(11434))
            .collect();
        assert_eq!(ports, expected);
        assert_eq!(ports.len(), 103);
    }

    #[tokio::test]
    async fn valid_catalog_is_sorted_deduplicated_and_credentialless() {
        let body = r#"{"data":[{"id":"zeta"},{"id":"alpha"},{"id":"alpha"}]}"#;
        let (port, request, handle) = server(json_response(body)).await;
        let candidates = discover_ports(&[port]).await.unwrap();
        handle.await.unwrap();

        assert_eq!(
            candidates,
            vec![LocalProviderCandidate {
                provider_id: format!("local-{port}"),
                port,
                base_url: format!("http://127.0.0.1:{port}/v1"),
                models: vec!["alpha".into(), "zeta".into()],
            }]
        );
        let request = request.lock().unwrap().clone().unwrap();
        assert!(request.starts_with("GET /v1/models HTTP/1.1\r\n"));
        let request = request.to_ascii_lowercase();
        assert!(!request.contains("authorization:"));
        assert!(!request.contains("proxy-authorization:"));
        assert!(!request.contains("cookie:"));
    }

    #[tokio::test]
    async fn redirect_is_not_followed() {
        let target_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_port = target_listener.local_addr().unwrap().port();
        let target = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_millis(700), target_listener.accept())
                .await
                .is_ok()
        });
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{target_port}/v1/models\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes();
        let (port, _, handle) = server(response).await;
        assert!(discover_ports(&[port]).await.unwrap().is_empty());
        handle.await.unwrap();
        assert!(!target.await.unwrap());
    }

    #[tokio::test]
    async fn malformed_empty_unsafe_and_oversized_catalogs_are_ignored() {
        let cases = [
            json_response("not-json"),
            json_response(r#"{"data":[]}"#),
            json_response(r#"{"data":[{"id":" bad"},{"id":"line\nfeed"}]}"#),
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_RESPONSE_BYTES + 1
            )
            .into_bytes(),
        ];
        for response in cases {
            let (port, _, handle) = server(response).await;
            assert!(discover_ports(&[port]).await.unwrap().is_empty());
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn chunked_body_is_capped_without_a_content_length() {
        let body = vec![b'x'; MAX_RESPONSE_BYTES + 1];
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(&body);
        response.extend_from_slice(b"\r\n0\r\n\r\n");
        let (port, _, handle) = server(response).await;
        assert!(discover_ports(&[port]).await.unwrap().is_empty());
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn supplied_port_scans_obey_the_concurrency_bound() {
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let mut ports = Vec::new();
        let mut handles = Vec::new();
        for index in 0..40 {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            ports.push(listener.local_addr().unwrap().port());
            let active = active.clone();
            let maximum = maximum.clone();
            handles.push(tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 1024];
                let _ = socket.read(&mut request).await.unwrap();
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(40)).await;
                let body = format!(r#"{{"data":[{{"id":"model-{index}"}}]}}"#);
                socket.write_all(&json_response(&body)).await.unwrap();
                active.fetch_sub(1, Ordering::SeqCst);
            }));
        }

        let candidates = discover_ports(&ports).await.unwrap();
        for handle in handles {
            handle.await.unwrap();
        }
        assert_eq!(candidates.len(), 40);
        assert!(maximum.load(Ordering::SeqCst) <= MAX_CONCURRENT_SCANS);
        assert!(maximum.load(Ordering::SeqCst) > 1);
    }

    #[tokio::test]
    async fn non_json_content_type_and_http_failure_are_ignored_without_body_use() {
        let marker = "SECRET_RESPONSE_MARKER";
        let responses = [
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{marker}",
                marker.len()
            )
            .into_bytes(),
            format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{marker}",
                marker.len()
            )
            .into_bytes(),
        ];
        for response in responses {
            let (port, _, handle) = server(response).await;
            let result = discover_ports(&[port]).await.unwrap();
            handle.await.unwrap();
            assert!(result.is_empty());
            assert!(!format!("{result:?}").contains(marker));
        }
    }

    #[tokio::test]
    async fn silent_endpoint_is_bounded_by_request_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_millis(700)).await;
        });
        let started = std::time::Instant::now();
        assert!(discover_ports(&[port]).await.unwrap().is_empty());
        assert!(started.elapsed() < Duration::from_millis(650));
        handle.abort();
    }
}
