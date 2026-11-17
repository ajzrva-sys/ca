//! SuperGrok image and video calls. The base URL is injected so tests hit a loopback server.
use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SAVED: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub enum MediaError {
    Denied(String),
    Failed(String),
}

impl MediaError {
    pub fn text(self) -> String {
        match self {
            Self::Denied(body) => body,
            Self::Failed(message) => message,
        }
    }

    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Denied(_))
    }
}

#[derive(Debug)]
pub struct MediaRequest {
    pub base_url: String,
    pub bearer: String,
    pub directory: PathBuf,
}

impl MediaRequest {
    pub fn from_env(directory: PathBuf) -> Self {
        let base_url =
            std::env::var("CA_XAI_MEDIA_BASE").unwrap_or_else(|_| "https://api.x.ai/v1".into());
        let bearer = std::env::var("CA_XAI_MEDIA_BEARER").unwrap_or_default();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            bearer,
            directory,
        }
    }
}

pub fn media_directory(cwd: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os("CA_XAI_MEDIA_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    cwd.join(".ca-media")
}

pub async fn bearer_for_request(explicit: &str) -> Result<String, String> {
    if !explicit.is_empty() {
        return Ok(explicit.to_string());
    }
    let store = crate::xai_oauth::XaiOauthStore::default();
    crate::xai_oauth::resolve_access_token(&store)
        .await
        .map_err(|error| error.to_string())
}

fn reject_video_options(
    duration: Option<u64>,
    resolution: Option<&str>,
    aspect: Option<&str>,
) -> Result<(), String> {
    if let Some(duration) = duration {
        if duration != 6 && duration != 10 {
            return Err("duration must be 6 or 10".into());
        }
    }
    if let Some(resolution) = resolution {
        if resolution != "480p" && resolution != "720p" {
            return Err("resolution must be 480p or 720p".into());
        }
    }
    if let Some(aspect) = aspect {
        if !matches!(
            aspect,
            "16:9" | "9:16" | "1:1" | "4:3" | "3:4" | "3:2" | "2:3"
        ) {
            return Err(format!("unsupported aspect ratio {aspect}"));
        }
    }
    Ok(())
}

pub async fn image_gen(request: &MediaRequest, arguments: &Value) -> Result<String, MediaError> {
    let prompt = required_text(arguments, "prompt")?;
    let aspect_ratio = arguments
        .get("aspect_ratio")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("auto");
    let body = json!({"prompt": prompt, "aspect_ratio": aspect_ratio});
    let saved = post_and_save(request, "/images/generations", &body, "png").await?;
    Ok(saved.display().to_string())
}

pub async fn image_edit(request: &MediaRequest, arguments: &Value) -> Result<String, MediaError> {
    let prompt = required_text(arguments, "prompt")?;
    let images = string_list(arguments, "images")?;
    if images.is_empty() {
        return Err(MediaError::Failed("images is required".into()));
    }
    let mut encoded = Vec::new();
    for path in images {
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|error| MediaError::Failed(format!("Error reading {path}: {error}")))?;
        encoded.push(base64::engine::general_purpose::STANDARD.encode(bytes));
    }
    let body = json!({"prompt": prompt, "images": encoded});
    let saved = post_and_save(request, "/images/edits", &body, "png").await?;
    Ok(saved.display().to_string())
}

pub async fn image_to_video(
    request: &MediaRequest,
    arguments: &Value,
) -> Result<String, MediaError> {
    let image = required_text(arguments, "image")?;
    let duration = arguments
        .get("duration")
        .and_then(Value::as_u64)
        .unwrap_or(6);
    let resolution = arguments
        .get("resolution_name")
        .and_then(Value::as_str)
        .unwrap_or("480p");
    reject_video_options(Some(duration), Some(resolution), None).map_err(MediaError::Failed)?;
    let prompt = arguments
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or("");
    let body = json!({
        "image": image,
        "prompt": prompt,
        "duration": duration,
        "resolution_name": resolution,
    });
    let saved = post_and_save(request, "/videos", &body, "mp4").await?;
    Ok(saved.display().to_string())
}

pub async fn reference_to_video(
    request: &MediaRequest,
    arguments: &Value,
) -> Result<String, MediaError> {
    let prompt = required_text(arguments, "prompt")?;
    let images = string_list(arguments, "images").unwrap_or_default();
    let voices = string_list(arguments, "voices").unwrap_or_default();
    let keyframes = arguments
        .get("keyframes")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if images.len() > 14 {
        return Err(MediaError::Failed("at most 14 reference images".into()));
    }
    if voices.len() > 3 {
        return Err(MediaError::Failed("at most 3 voices".into()));
    }
    if keyframes > 4 {
        return Err(MediaError::Failed("at most 4 keyframes".into()));
    }
    let duration = arguments.get("duration").and_then(Value::as_u64);
    let aspect = arguments.get("aspect_ratio").and_then(Value::as_str);
    reject_video_options(duration, None, aspect).map_err(MediaError::Failed)?;
    let body = json!({
        "prompt": prompt,
        "images": images,
        "voices": voices,
        "first_frame": arguments.get("first_frame").cloned().unwrap_or(Value::Null),
        "last_frame": arguments.get("last_frame").cloned().unwrap_or(Value::Null),
        "keyframes": arguments.get("keyframes").cloned().unwrap_or(json!([])),
        "duration": duration.unwrap_or(6),
        "aspect_ratio": aspect.unwrap_or("16:9"),
    });
    let saved = post_and_save(request, "/videos", &body, "mp4").await?;
    Ok(saved.display().to_string())
}

async fn post_and_save(
    request: &MediaRequest,
    path: &str,
    body: &Value,
    extension: &str,
) -> Result<PathBuf, MediaError> {
    let bearer = bearer_for_request(&request.bearer)
        .await
        .map_err(MediaError::Failed)?;
    let url = format!("{}{path}", request.base_url);
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(bearer)
        .json(body)
        .send()
        .await
        .map_err(|error| MediaError::Failed(format!("Error: {error}")))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| MediaError::Failed(format!("Error: {error}")))?;
    if status.as_u16() == 403 {
        return Err(MediaError::Denied(
            String::from_utf8_lossy(&bytes).trim().to_string(),
        ));
    }
    if !status.is_success() {
        return Err(MediaError::Failed(format!(
            "media request failed: {status} {}",
            String::from_utf8_lossy(&bytes)
        )));
    }
    let payload: Value =
        serde_json::from_slice(&bytes).map_err(|error| MediaError::Failed(error.to_string()))?;
    let encoded = payload["data"][0]["b64_json"]
        .as_str()
        .or_else(|| payload["b64_json"].as_str())
        .ok_or_else(|| MediaError::Failed("media response has no image or video bytes".into()))?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| MediaError::Failed(error.to_string()))?;
    tokio::fs::create_dir_all(&request.directory)
        .await
        .map_err(|error| MediaError::Failed(error.to_string()))?;
    let name = format!(
        "media-{}.{}",
        SAVED.fetch_add(1, Ordering::Relaxed),
        extension
    );
    let path = request.directory.join(name);
    tokio::fs::write(&path, decoded)
        .await
        .map_err(|error| MediaError::Failed(error.to_string()))?;
    Ok(path)
}

fn required_text<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, MediaError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| MediaError::Failed(format!("{key} is required")))
}

fn string_list(arguments: &Value, key: &str) -> Result<Vec<String>, MediaError> {
    let Some(value) = arguments.get(key) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| MediaError::Failed(format!("{key} must be a list")))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| MediaError::Failed(format!("{key} entries must be strings")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    async fn serve(body_for: Arc<AtomicUsize>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let count = body_for.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut buf = vec![0_u8; 65536];
                    let _ = socket.readable().await;
                    let _ = socket.try_read(&mut buf);
                    let payload = if count == 0 {
                        let encoded =
                            base64::engine::general_purpose::STANDARD.encode(b"png-bytes");
                        format!("{{\"data\":[{{\"b64_json\":\"{encoded}\"}}]}}")
                    } else {
                        "Video generation is a SuperGrok feature on this tier and zero-data-retention blocks video".into()
                    };
                    let status = if count == 0 {
                        "200 OK"
                    } else {
                        "403 Forbidden"
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                        payload.len()
                    );
                    let _ = socket.try_write(response.as_bytes());
                });
            }
        });
        format!("http://{address}/v1")
    }

    #[tokio::test]
    async fn media_tools_save_a_200_and_return_a_tier_403_without_retry() {
        let hits = Arc::new(AtomicUsize::new(0));
        let base = serve(hits.clone()).await;
        let directory = tempfile::tempdir().unwrap();
        std::env::set_var("CA_XAI_MEDIA_BASE", &base);
        std::env::set_var("CA_XAI_MEDIA_BEARER", "test-bearer");
        std::env::set_var("CA_XAI_MEDIA_DIR", directory.path());
        let cwd = directory.path();
        let context = crate::agent::registry::ToolContext {
            recovery: None,
            cwd: cwd.to_path_buf(),
            timeout_secs: 2,
            profile: crate::agent::registry::ToolProfile::Main,
            workspace_roots: vec![cwd.to_path_buf()],
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            network_allowed: true,
            job_owner: crate::jobs::JobOwner::process(),
            job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
        };
        let saved = crate::agent::tools::execute_tool_with_context(
            "image_gen",
            &json!({"prompt": "a cat"}),
            &context,
        )
        .await;
        assert!(saved.is_success(), "{}", saved.model_text());
        let path = PathBuf::from(saved.model_text().trim());
        assert!(path.starts_with(directory.path()), "{}", path.display());
        assert_eq!(std::fs::read(&path).unwrap(), b"png-bytes");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let denied = crate::agent::tools::execute_tool_with_context(
            "image_to_video",
            &json!({"image": "frame.png", "duration": 6, "resolution_name": "480p"}),
            &context,
        )
        .await;
        let text = denied.model_text();
        assert!(text.contains("zero-data-retention"), "{text}");
        assert!(text.contains("tier"), "{text}");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        let rejected = crate::agent::tools::execute_tool_with_context(
            "reference_to_video",
            &json!({"prompt": "walk", "duration": 5, "aspect_ratio": "nope"}),
            &context,
        )
        .await;
        assert!(
            rejected.model_text().contains("duration") || rejected.model_text().contains("aspect"),
            "{}",
            rejected.model_text()
        );
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }
}
