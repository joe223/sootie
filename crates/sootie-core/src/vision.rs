use std::env;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::{json, Value};

use crate::config::VisionSettings;
use crate::types::{Bounds, SootieError, SootieResult};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:9876";
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_GROUND_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_CONFIDENCE_THRESHOLD: f64 = 0.5;

#[derive(Debug, Clone)]
pub(crate) struct VisionConfig {
    enabled: bool,
    base_url: String,
    connect_timeout: Duration,
    ground_timeout: Duration,
    confidence_threshold: f64,
}

impl VisionConfig {
    pub(crate) fn from_env_and_settings(settings: &VisionSettings) -> Self {
        let configured_url = env::var("SOOTIE_VISION_URL").ok();
        let configured_port = env::var("SOOTIE_VISION_PORT").ok();
        let disabled = env_flag("SOOTIE_VISION_DISABLED")
            || env_flag("SOOTIE_VISION_OFF")
            || env_flag("SOOTIE_DISABLE_VISION")
            || settings.disabled == Some(true);
        #[cfg(test)]
        let test_default_disabled =
            configured_url.is_none() && configured_port.is_none() && settings.url.is_none();
        #[cfg(not(test))]
        let test_default_disabled = false;
        let base_url = configured_url
            .or_else(|| settings.url.clone())
            .unwrap_or_else(|| {
                configured_port
                    .or_else(|| settings.port.map(|port| port.to_string()))
                    .map(|port| format!("http://127.0.0.1:{port}"))
                    .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
            });

        Self {
            enabled: settings.enabled.unwrap_or(true) && !disabled && !test_default_disabled,
            base_url,
            connect_timeout: duration_env_ms("SOOTIE_VISION_CONNECT_TIMEOUT_MS")
                .or(settings.connect_timeout)
                .unwrap_or(DEFAULT_CONNECT_TIMEOUT),
            ground_timeout: duration_env_ms("SOOTIE_VISION_TIMEOUT_MS")
                .or(settings.ground_timeout)
                .unwrap_or(DEFAULT_GROUND_TIMEOUT),
            confidence_threshold: f64_env(
                "SOOTIE_VISION_CONFIDENCE_THRESHOLD",
                settings
                    .confidence_threshold
                    .unwrap_or(DEFAULT_CONFIDENCE_THRESHOLD),
            )
            .clamp(0.0, 1.0),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_tests(base_url: String) -> Self {
        Self {
            enabled: true,
            base_url,
            connect_timeout: Duration::from_secs(2),
            ground_timeout: Duration::from_secs(2),
            confidence_threshold: DEFAULT_CONFIDENCE_THRESHOLD,
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn confidence_threshold(&self) -> f64 {
        self.confidence_threshold
    }
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self::from_env_and_settings(&VisionSettings::default())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GroundRequest<'a> {
    pub(crate) image_base64: &'a str,
    pub(crate) description: &'a str,
    pub(crate) screen_width: f64,
    pub(crate) screen_height: f64,
    pub(crate) crop_box: Option<[f64; 4]>,
}

#[derive(Debug, Clone)]
pub(crate) struct GroundResult {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) confidence: f64,
    pub(crate) method: String,
    pub(crate) inference_ms: Option<u64>,
    pub(crate) raw_text: Option<String>,
    pub(crate) bounds: Option<Bounds>,
    pub(crate) response: Value,
}

pub(crate) fn ground(
    config: &VisionConfig,
    request: &GroundRequest<'_>,
) -> SootieResult<Option<GroundResult>> {
    if !config.is_enabled() {
        return Ok(None);
    }
    let endpoint = Endpoint::parse(&config.base_url)?;
    let payload = ground_payload(request);
    let response = http_json(
        &endpoint,
        "POST",
        "/ground",
        Some(&payload),
        config.connect_timeout,
        config.ground_timeout,
    )?;
    parse_ground_response(&response, request.screen_width, request.screen_height).map(Some)
}

#[allow(dead_code)]
pub(crate) fn health(config: &VisionConfig) -> SootieResult<Option<Value>> {
    if !config.is_enabled() {
        return Ok(None);
    }
    let endpoint = Endpoint::parse(&config.base_url)?;
    http_json(
        &endpoint,
        "GET",
        "/health",
        None,
        config.connect_timeout,
        config.connect_timeout,
    )
    .map(Some)
}

fn ground_payload(request: &GroundRequest<'_>) -> Value {
    let mut payload = json!({
        "image": request.image_base64,
        "description": request.description,
        "screen_w": request.screen_width,
        "screen_h": request.screen_height,
    });
    if let Some(crop_box) = request.crop_box {
        payload["crop_box"] = json!(crop_box);
    }
    payload
}

fn parse_ground_response(
    response: &Value,
    screen_width: f64,
    screen_height: f64,
) -> SootieResult<GroundResult> {
    if let (Some(x), Some(y), Some(confidence)) = (
        response.get("x").and_then(Value::as_f64),
        response.get("y").and_then(Value::as_f64),
        response.get("confidence").and_then(Value::as_f64),
    ) {
        return Ok(GroundResult {
            x,
            y,
            confidence: confidence.clamp(0.0, 1.0),
            method: response
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("vision-ground")
                .to_string(),
            inference_ms: response.get("inference_ms").and_then(Value::as_u64),
            raw_text: response
                .get("raw")
                .and_then(Value::as_str)
                .map(str::to_string),
            bounds: None,
            response: response.clone(),
        });
    }

    let Some(first_match) = response
        .get("matches")
        .and_then(Value::as_array)
        .and_then(|matches| matches.first())
    else {
        return Err(SootieError::Platform(
            "vision /ground response did not contain coordinates".to_string(),
        ));
    };
    let point = parse_point(first_match.get("point")).ok_or_else(|| {
        SootieError::Platform("vision /ground match did not contain a point".to_string())
    })?;
    let x = scale_coordinate(point.0, screen_width);
    let y = scale_coordinate(point.1, screen_height);
    let bounds = first_match
        .get("bbox")
        .and_then(|bbox| parse_bounds(bbox, screen_width, screen_height));
    Ok(GroundResult {
        x,
        y,
        confidence: first_match
            .get("confidence")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .clamp(0.0, 1.0),
        method: "vision-match".to_string(),
        inference_ms: response
            .get("timings")
            .and_then(|timings| timings.get("total_ms"))
            .and_then(Value::as_f64)
            .map(|ms| ms.max(0.0).round() as u64),
        raw_text: first_match
            .get("label")
            .and_then(Value::as_str)
            .map(str::to_string),
        bounds,
        response: response.clone(),
    })
}

fn parse_point(value: Option<&Value>) -> Option<(f64, f64)> {
    match value? {
        Value::Array(values) => Some((values.first()?.as_f64()?, values.get(1)?.as_f64()?)),
        Value::Object(map) => Some((map.get("x")?.as_f64()?, map.get("y")?.as_f64()?)),
        _ => None,
    }
}

fn parse_bounds(value: &Value, screen_width: f64, screen_height: f64) -> Option<Bounds> {
    let object = value.as_object()?;
    let x = scale_coordinate(object.get("x")?.as_f64()?, screen_width);
    let y = scale_coordinate(object.get("y")?.as_f64()?, screen_height);
    let width = scale_size(object.get("width")?.as_f64()?, screen_width);
    let height = scale_size(object.get("height")?.as_f64()?, screen_height);
    Some(Bounds {
        x,
        y,
        width,
        height,
    })
}

fn scale_coordinate(value: f64, size: f64) -> f64 {
    if value.abs() <= 1.0 && size > 0.0 {
        value * size
    } else {
        value
    }
}

fn scale_size(value: f64, size: f64) -> f64 {
    if (0.0..=1.0).contains(&value) && size > 0.0 {
        value * size
    } else {
        value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Endpoint {
    host: String,
    port: u16,
    path_prefix: String,
}

impl Endpoint {
    fn parse(base_url: &str) -> SootieResult<Self> {
        let Some(rest) = base_url.strip_prefix("http://") else {
            return Err(SootieError::Unsupported(
                "SOOTIE_VISION_URL must use http://".to_string(),
            ));
        };
        let (authority, path_prefix) = rest.split_once('/').unwrap_or((rest, ""));
        let authority = authority.trim();
        let (host, port) = parse_authority(authority)?;
        Ok(Self {
            host,
            port,
            path_prefix: normalize_path_prefix(path_prefix),
        })
    }

    fn path(&self, suffix: &str) -> String {
        if self.path_prefix.is_empty() {
            suffix.to_string()
        } else {
            format!("{}{}", self.path_prefix, suffix)
        }
    }
}

fn parse_authority(authority: &str) -> SootieResult<(String, u16)> {
    if authority.is_empty() {
        return Err(SootieError::InvalidArguments(
            "SOOTIE_VISION_URL host is empty".to_string(),
        ));
    }
    if let Some(host_port) = authority.strip_prefix('[') {
        let Some((host, rest)) = host_port.split_once(']') else {
            return Err(SootieError::InvalidArguments(
                "SOOTIE_VISION_URL IPv6 host is missing ']'".to_string(),
            ));
        };
        let port = rest
            .strip_prefix(':')
            .map(parse_port)
            .transpose()?
            .unwrap_or(80);
        return Ok((host.to_string(), port));
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        Ok((host.to_string(), parse_port(port)?))
    } else {
        Ok((authority.to_string(), 80))
    }
}

fn parse_port(port: &str) -> SootieResult<u16> {
    port.parse::<u16>()
        .map_err(|_| SootieError::InvalidArguments("SOOTIE_VISION_URL port is invalid".to_string()))
}

fn normalize_path_prefix(path: &str) -> String {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("/{trimmed}")
    }
}

fn http_json(
    endpoint: &Endpoint,
    method: &str,
    path: &str,
    body: Option<&Value>,
    connect_timeout: Duration,
    read_timeout: Duration,
) -> SootieResult<Value> {
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| SootieError::Platform("vision endpoint did not resolve".to_string()))?;
    let mut stream = TcpStream::connect_timeout(&address, connect_timeout)?;
    let _ = stream.set_read_timeout(Some(read_timeout));
    let _ = stream.set_write_timeout(Some(connect_timeout));
    let body_bytes = body
        .map(serde_json::to_vec)
        .transpose()?
        .unwrap_or_default();
    let request_path = endpoint.path(path);
    let headers = if body.is_some() {
        format!(
            "{method} {request_path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            endpoint.host,
            body_bytes.len()
        )
    } else {
        format!(
            "{method} {request_path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            endpoint.host
        )
    };
    stream.write_all(headers.as_bytes())?;
    if !body_bytes.is_empty() {
        stream.write_all(&body_bytes)?;
    }
    let response = read_http_response(&mut stream)?;
    let (headers, body) = response.split_once("\r\n\r\n").unwrap_or(("", &response));
    let status = parse_status_code(headers).unwrap_or(0);
    if !(200..300).contains(&status) {
        return Err(SootieError::Platform(format!(
            "vision endpoint returned HTTP {status}"
        )));
    }
    let body = if header_has_token(headers, "transfer-encoding", "chunked") {
        decode_chunked_body(body.as_bytes())?
    } else {
        body.to_string()
    };
    serde_json::from_str(body.trim()).map_err(SootieError::Json)
}

fn read_http_response(stream: &mut TcpStream) -> SootieResult<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                if http_response_body_complete(&bytes) {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if bytes.is_empty() {
                    return Err(SootieError::Io(error));
                }
                break;
            }
            Err(error) => return Err(SootieError::Io(error)),
        }
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn parse_status_code(headers: &str) -> Option<u16> {
    headers
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn http_response_body_complete(bytes: &[u8]) -> bool {
    let Some(header_end) = find_header_end(bytes) else {
        return false;
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let Some(content_length) = http_content_length(&headers) else {
        return false;
    };
    bytes.len() >= header_end + 4 + content_length
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn http_content_length(headers: &str) -> Option<usize> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())
            .flatten()
    })
}

fn header_has_token(headers: &str, name: &str, token: &str) -> bool {
    headers.lines().any(|line| {
        let Some((header_name, value)) = line.split_once(':') else {
            return false;
        };
        header_name.eq_ignore_ascii_case(name)
            && value
                .split(',')
                .any(|candidate| candidate.trim().eq_ignore_ascii_case(token))
    })
}

fn decode_chunked_body(body: &[u8]) -> SootieResult<String> {
    let mut cursor = 0;
    let mut decoded = Vec::new();
    loop {
        let Some(line_end) = find_crlf(body, cursor) else {
            return Err(SootieError::Platform(
                "vision endpoint returned incomplete chunked response".to_string(),
            ));
        };
        let size_line = String::from_utf8_lossy(&body[cursor..line_end]);
        let size_text = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| {
            SootieError::Platform("vision endpoint returned invalid chunk size".to_string())
        })?;
        cursor = line_end + 2;
        if size == 0 {
            break;
        }
        let chunk_end = cursor + size;
        if body.len() < chunk_end + 2 {
            return Err(SootieError::Platform(
                "vision endpoint returned truncated chunked response".to_string(),
            ));
        }
        decoded.extend_from_slice(&body[cursor..chunk_end]);
        cursor = chunk_end + 2;
    }
    Ok(String::from_utf8_lossy(&decoded).to_string())
}

fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|index| start + index)
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn duration_env_ms(name: &str) -> Option<Duration> {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
}

fn f64_env(name: &str, default: f64) -> f64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn parses_direct_ground_response() {
        let response = json!({
            "x": 12.0,
            "y": 34.0,
            "confidence": 0.82,
            "method": "full-screen",
            "raw": "[0.1, 0.2]",
            "inference_ms": 45
        });

        let result = parse_ground_response(&response, 100.0, 100.0).unwrap();

        assert_eq!(result.x, 12.0);
        assert_eq!(result.y, 34.0);
        assert_eq!(result.confidence, 0.82);
        assert_eq!(result.method, "full-screen");
        assert_eq!(result.inference_ms, Some(45));
        assert_eq!(result.raw_text.as_deref(), Some("[0.1, 0.2]"));
    }

    #[test]
    fn parses_match_based_ground_response() {
        let response = json!({
            "matches": [{
                "label": "button",
                "confidence": 0.7,
                "point": {"x": 0.25, "y": 0.5},
                "bbox": {"x": 0.2, "y": 0.4, "width": 0.1, "height": 0.2}
            }],
            "timings": {"total_ms": 10.4}
        });

        let result = parse_ground_response(&response, 400.0, 300.0).unwrap();

        assert_eq!(result.x, 100.0);
        assert_eq!(result.y, 150.0);
        assert_eq!(result.confidence, 0.7);
        assert_eq!(result.method, "vision-match");
        assert_eq!(
            result.bounds,
            Some(Bounds {
                x: 80.0,
                y: 120.0,
                width: 40.0,
                height: 60.0,
            })
        );
    }

    #[test]
    fn decodes_chunked_http_response_body() {
        let decoded = decode_chunked_body(b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n").unwrap();

        assert_eq!(decoded, "hello world");
    }

    #[test]
    fn posts_ground_request_to_configured_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_test_http_request(&mut stream);
            assert!(request.starts_with("POST /ground HTTP/1.1"));
            assert!(request.contains("\"description\":\"target\""));
            assert!(request.contains("\"screen_w\":400.0"));
            let body = r#"{"x":20.0,"y":30.0,"confidence":0.9,"method":"full-screen"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        let config = VisionConfig::for_tests(format!("http://127.0.0.1:{port}"));

        let result = ground(
            &config,
            &GroundRequest {
                image_base64: "abc",
                description: "target",
                screen_width: 400.0,
                screen_height: 300.0,
                crop_box: None,
            },
        )
        .unwrap()
        .unwrap();

        assert_eq!(result.x, 20.0);
        assert_eq!(result.y, 30.0);
        assert_eq!(result.confidence, 0.9);
        handle.join().unwrap();
    }

    fn read_test_http_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            if test_http_body_complete(&bytes) {
                break;
            }
        }
        String::from_utf8_lossy(&bytes).to_string()
    }

    fn test_http_body_complete(bytes: &[u8]) -> bool {
        let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            return false;
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        });
        match content_length {
            Some(length) => bytes.len() >= header_end + 4 + length,
            None => true,
        }
    }
}
