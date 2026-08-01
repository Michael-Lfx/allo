//! Explicit, developer-run admission probe for Parallel `web_fetch`.
//!
//! This example is deliberately outside the production tool registry. It does
//! not run during normal `cargo test` and prints contract/schema/result shapes
//! only, never page text, unless the caller explicitly passes `--raw` or sets
//! `ALLO_WEB_FETCH_PROBE_RAW=1`.

use std::error::Error;
use std::time::{Duration, Instant};

use reqwest::header::{
    ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, RETRY_AFTER,
};
use serde_json::{Value, json};

const PARALLEL_ENDPOINT: &str = "https://search.parallel.ai/mcp";
const FETCH_TOOL: &str = "web_fetch";
const LEGACY_VERSION: &str = "2025-11-25";
const MODERN_VERSION: &str = "2026-07-28";
const MAX_PROBE_BODY: usize = 2 * 1024 * 1024;

#[derive(Debug)]
struct WireResponse {
    status: reqwest::StatusCode,
    session_id: Option<String>,
    retry_after: bool,
    json: Option<Value>,
    byte_len: usize,
    elapsed: Duration,
}

struct Discovery {
    protocol_version: String,
    session_mode: &'static str,
    initialize_ms: u128,
    tools_list_ms: u128,
    tool_count: usize,
    tool: Option<Value>,
    session_id: Option<String>,
    modern: bool,
}

#[derive(PartialEq)]
struct ProbeCase {
    name: &'static str,
    urls: &'static [&'static str],
}

const CASES: &[ProbeCase] = &[
    ProbeCase {
        name: "static_html",
        urls: &["https://example.com/"],
    },
    ProbeCase {
        name: "js_shell",
        urls: &["https://react.dev/"],
    },
    ProbeCase {
        name: "pdf",
        urls: &["https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf"],
    },
    ProbeCase {
        name: "chinese_article",
        urls: &["https://zh.wikipedia.org/wiki/%E4%B8%AD%E5%8D%8E%E4%BA%BA%E6%B0%91%E5%85%B1%E5%92%8C%E5%9B%BD"],
    },
    ProbeCase {
        name: "short_page",
        urls: &["https://example.org/"],
    },
    ProbeCase {
        name: "long_page",
        urls: &["https://www.ietf.org/rfc/rfc9110.txt"],
    },
    ProbeCase {
        name: "http_404",
        urls: &["https://example.com/allo-managed-web-fetch-probe-404"],
    },
    ProbeCase {
        name: "http_403",
        urls: &["https://httpbin.org/status/403"],
    },
    ProbeCase {
        name: "two_success",
        urls: &["https://example.com/", "https://example.org/"],
    },
    ProbeCase {
        name: "one_success_one_failure",
        urls: &["https://example.com/", "https://example.com/allo-managed-web-fetch-probe-404"],
    },
    ProbeCase {
        name: "two_success_one_failure",
        urls: &[
            "https://example.com/",
            "https://example.org/",
            "https://example.com/allo-managed-web-fetch-probe-404",
        ],
    },
    ProbeCase {
        name: "duplicate_urls",
        urls: &["https://example.com/", "https://example.com/"],
    },
    ProbeCase {
        name: "redirect",
        urls: &["https://google.com/"],
    },
];

struct Args {
    raw: bool,
    cases: Option<Vec<String>>,
}

struct SummarizedResult {
    structured_content_present: bool,
    text_content_present: bool,
    item_count: usize,
    partial_shape: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()?;

    let discovery = discover(&client).await?;
    print_discovery(&discovery);

    let Some(tool) = discovery.tool.as_ref() else {
        eprintln!(
            "probe outcome=not_admitted reason=tool_missing tool={FETCH_TOOL}"
        );
        return Ok(());
    };
    let _ = tool;

    let selected = select_cases(args.cases.as_deref())?;
    for (index, case) in selected.iter().enumerate() {
        let id = 100 + index as u64;
        let response = call_tool(
            &client,
            discovery.modern,
            id,
            discovery.session_id.as_deref(),
            case.urls,
        )
        .await;
        match response {
            Ok(response) => print_call(case.name, &response, args.raw),
            Err(error) => {
                println!("case={} outcome=failed error={error}", case.name);
            }
        }
    }

    Ok(())
}

fn parse_args() -> Result<Args, Box<dyn Error>> {
    let mut raw = std::env::var("ALLO_WEB_FETCH_PROBE_RAW").as_deref() == Ok("1");
    let mut cases = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--raw" => raw = true,
            "--case" => {
                let value = args
                    .next()
                    .ok_or("--case requires a comma-separated list of case names")?;
                cases = Some(
                    value
                        .split(',')
                        .map(|name| name.trim().to_owned())
                        .filter(|name| !name.is_empty())
                        .collect(),
                );
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }
    Ok(Args { raw, cases })
}

async fn discover(client: &reqwest::Client) -> Result<Discovery, Box<dyn Error>> {
    let modern_discover = match modern_request(
        client,
        1,
        "server/discover",
        json!({}),
        None,
    )
    .await
    {
        Ok(response) => Some(response),
        Err(error) => {
            println!("transport=modern_discover outcome=failed error={error}");
            None
        }
    };

    if let Some(discover) = modern_discover.as_ref()
        && is_modern_response(discover)
    {
        let tools = modern_request(client, 2, "tools/list", json!({}), None).await?;
        let tool_count = tools
            .json
            .as_ref()
            .and_then(|value| value.get("result"))
            .and_then(|result| result.get("tools"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        return Ok(Discovery {
            protocol_version: MODERN_VERSION.to_owned(),
            session_mode: "sessionless",
            initialize_ms: 0,
            tools_list_ms: tools.elapsed.as_millis(),
            tool_count,
            tool: find_tool(&tools),
            session_id: None,
            modern: true,
        });
    }

    let initialize = legacy_request(
        client,
        10,
        "initialize",
        json!({
            "protocolVersion": LEGACY_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "allo-web-fetch-probe", "version": "0"}
        }),
        None,
    )
    .await?;
    if initialize
        .json
        .as_ref()
        .and_then(|value| value.get("result"))
        .is_none()
    {
        return Err("neither modern discovery nor legacy initialize succeeded".into());
    }

    let session_id = initialize.session_id.clone();
    let session_mode = if session_id.is_some() {
        "stateful"
    } else {
        "sessionless"
    };
    let protocol_version = initialize
        .json
        .as_ref()
        .and_then(|value| value.get("result"))
        .and_then(|result| result.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or(LEGACY_VERSION)
        .to_owned();
    let initialize_ms = initialize.elapsed.as_millis();

    let initialized = legacy_notification(
        client,
        "notifications/initialized",
        json!({}),
        session_id.as_deref(),
    )
    .await?;
    if !initialized.status.is_success() {
        return Err(format!(
            "notifications/initialized returned HTTP {}",
            initialized.status.as_u16()
        )
        .into());
    }

    let tools = legacy_request(
        client,
        11,
        "tools/list",
        json!({}),
        session_id.as_deref(),
    )
    .await?;
    let tool_count = tools
        .json
        .as_ref()
        .and_then(|value| value.get("result"))
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);

    Ok(Discovery {
        protocol_version,
        session_mode,
        initialize_ms,
        tools_list_ms: tools.elapsed.as_millis(),
        tool_count,
        tool: find_tool(&tools),
        session_id,
        modern: false,
    })
}

fn print_discovery(discovery: &Discovery) {
    println!("protocol_version={}", discovery.protocol_version);
    println!("session_mode={}", discovery.session_mode);
    println!("initialize_ms={}", discovery.initialize_ms);
    println!("tools_list_ms={}", discovery.tools_list_ms);
    println!("tool_count={}", discovery.tool_count);
    let Some(tool) = discovery.tool.as_ref() else {
        println!("tool_name=missing");
        return;
    };
    println!(
        "tool_name={}",
        tool.get("name").and_then(Value::as_str).unwrap_or("unknown")
    );
    println!(
        "input_schema={}",
        serde_json::to_string(tool.get("inputSchema").unwrap_or(&Value::Null))
            .unwrap_or_else(|error| format!("<schema serialization failed: {error}>"))
    );
    println!(
        "output_schema={}",
        serde_json::to_string(tool.get("outputSchema").unwrap_or(&Value::Null))
            .unwrap_or_else(|error| format!("<schema serialization failed: {error}>"))
    );
}

async fn call_tool(
    client: &reqwest::Client,
    modern: bool,
    id: u64,
    session_id: Option<&str>,
    urls: &[&str],
) -> Result<WireResponse, Box<dyn Error>> {
    let arguments = json!({
        "urls": urls,
        "full_content": false,
    });
    let params = json!({
        "name": FETCH_TOOL,
        "arguments": arguments,
    });
    if modern {
        modern_request(client, id, "tools/call", params, Some(FETCH_TOOL)).await
    } else {
        legacy_request(client, id, "tools/call", params, session_id).await
    }
}

fn print_call(case: &str, response: &WireResponse, raw: bool) {
    let error_code = response
        .json
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64);
    let result = response
        .json
        .as_ref()
        .and_then(|value| value.get("result"));
    let summary = summarize_result(result);
    println!(
        "case={case} http_status={} call_ms={} response_bytes={} structured_content_present={} text_content_present={} result_item_count={} partial_failure_shape={} retry_after_present={} error_code={}",
        response.status.as_u16(),
        response.elapsed.as_millis(),
        response.byte_len,
        summary.structured_content_present,
        summary.text_content_present,
        summary.item_count,
        summary.partial_shape,
        response.retry_after,
        error_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "none".into())
    );
    if raw {
        let raw_value = response
            .json
            .as_ref()
            .map(Value::to_string)
            .unwrap_or_else(|| format!("<non-json {} bytes>", response.byte_len));
        println!("case={case} raw_response={raw_value}");
    }
}

fn summarize_result(result: Option<&Value>) -> SummarizedResult {
    SummarizedResult {
        structured_content_present: result
            .and_then(|result| result.get("structuredContent"))
            .is_some(),
        text_content_present: result
            .and_then(|result| result.get("content"))
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.get("type").and_then(Value::as_str) == Some("text"))
            }),
        item_count: count_result_items(result),
        partial_shape: partial_failure_shape(result),
    }
}

fn count_result_items(result: Option<&Value>) -> usize {
    let Some(result) = result else {
        return 0;
    };
    if let Some(items) = result
        .get("structuredContent")
        .and_then(|structured| structured.get("results"))
        .and_then(Value::as_array)
    {
        return items.len();
    }
    if let Some(items) = result.get("content").and_then(Value::as_array) {
        if let Some(text) = items
            .iter()
            .find_map(|item| item.get("text").and_then(Value::as_str))
            && let Ok(parsed) = serde_json::from_str::<Value>(text)
            && let Some(parsed_items) = parsed
                .get("results")
                .or_else(|| parsed.get("items"))
                .and_then(Value::as_array)
        {
            return parsed_items.len();
        }
        return items.len();
    }
    0
}

fn partial_failure_shape(result: Option<&Value>) -> String {
    let Some(result) = result else {
        return "no_result".to_owned();
    };
    let mut parts = Vec::new();
    if result.get("isError").and_then(Value::as_bool).unwrap_or(false) {
        parts.push("is_error=true".to_owned());
    }
    if let Some(items) = result
        .get("structuredContent")
        .and_then(|structured| structured.get("results"))
        .and_then(Value::as_array)
    {
        let error_items = items
            .iter()
            .filter(|item| {
                item.get("error").is_some()
                    || item
                        .get("status")
                        .and_then(Value::as_i64)
                        .is_some_and(|status| (400..600).contains(&status))
                    || item.get("failed").and_then(Value::as_bool).unwrap_or(false)
            })
            .count();
        parts.push(format!("structured_results={} error_items={error_items}", items.len()));
    }
    if let Some(errors) = result
        .get("structuredContent")
        .and_then(|structured| structured.get("errors"))
        .and_then(Value::as_array)
    {
        parts.push(format!("structured_errors={}", errors.len()));
    }
    if let Some(items) = result.get("content").and_then(Value::as_array) {
        parts.push(format!("content_items={}", items.len()));
    }
    for key in ["errors", "warnings", "failed", "missing"] {
        if result.get(key).is_some() {
            parts.push(format!("{key}=present"));
        }
    }
    if parts.is_empty() {
        "none".to_owned()
    } else {
        parts.join(" ")
    }
}

async fn modern_request(
    client: &reqwest::Client,
    id: u64,
    method: &str,
    mut params: Value,
    tool_name: Option<&str>,
) -> Result<WireResponse, Box<dyn Error>> {
    let object = params
        .as_object_mut()
        .ok_or("probe params must be a JSON object")?;
    object.insert(
        "_meta".to_owned(),
        json!({
            "io.modelcontextprotocol/protocolVersion": MODERN_VERSION,
            "io.modelcontextprotocol/clientInfo": {
                "name": "allo-web-fetch-probe",
                "version": "0"
            },
            "io.modelcontextprotocol/clientCapabilities": {}
        }),
    );
    let mut headers = standard_headers(MODERN_VERSION);
    headers.insert(
        HeaderName::from_static("mcp-method"),
        HeaderValue::from_str(method)?,
    );
    if let Some(tool_name) = tool_name {
        headers.insert(
            HeaderName::from_static("mcp-name"),
            HeaderValue::from_str(tool_name)?,
        );
    }
    send(
        client,
        headers,
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
    )
    .await
}

async fn legacy_request(
    client: &reqwest::Client,
    id: u64,
    method: &str,
    params: Value,
    session_id: Option<&str>,
) -> Result<WireResponse, Box<dyn Error>> {
    let mut headers = standard_headers(LEGACY_VERSION);
    if let Some(session_id) = session_id {
        headers.insert(
            HeaderName::from_static("mcp-session-id"),
            HeaderValue::from_str(session_id)?,
        );
    }
    send(
        client,
        headers,
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
    )
    .await
}

async fn legacy_notification(
    client: &reqwest::Client,
    method: &str,
    params: Value,
    session_id: Option<&str>,
) -> Result<WireResponse, Box<dyn Error>> {
    let mut headers = standard_headers(LEGACY_VERSION);
    if let Some(session_id) = session_id {
        headers.insert(
            HeaderName::from_static("mcp-session-id"),
            HeaderValue::from_str(session_id)?,
        );
    }
    send(
        client,
        headers,
        json!({"jsonrpc": "2.0", "method": method, "params": params}),
    )
    .await
}

fn standard_headers(version: &'static str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(
        HeaderName::from_static("mcp-protocol-version"),
        HeaderValue::from_static(version),
    );
    headers
}

async fn send(
    client: &reqwest::Client,
    headers: HeaderMap,
    body: Value,
) -> Result<WireResponse, Box<dyn Error>> {
    let started = Instant::now();
    let response = client
        .post(PARALLEL_ENDPOINT)
        .headers(headers)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let session_id = response
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let retry_after = response.headers().get(RETRY_AFTER).is_some();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_PROBE_BODY {
        return Err(format!("response exceeded {MAX_PROBE_BODY} bytes").into());
    }
    let json = parse_wire_json(&content_type, &bytes);
    Ok(WireResponse {
        status,
        session_id,
        retry_after,
        json,
        byte_len: bytes.len(),
        elapsed: started.elapsed(),
    })
}

fn parse_wire_json(content_type: &str, bytes: &[u8]) -> Option<Value> {
    if content_type.contains("text/event-stream") {
        let text = std::str::from_utf8(bytes).ok()?;
        return text.lines().find_map(|line| {
            line.strip_prefix("data:")
                .and_then(|data| serde_json::from_str(data.trim()).ok())
        });
    }
    serde_json::from_slice(bytes).ok()
}

fn is_modern_response(response: &WireResponse) -> bool {
    let Some(json) = response.json.as_ref() else {
        return false;
    };
    json.get("result")
        .and_then(|result| result.get("supportedVersions"))
        .is_some()
        || json
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64)
            == Some(-32022)
}

fn find_tool(response: &WireResponse) -> Option<Value> {
    response
        .json
        .as_ref()?
        .get("result")?
        .get("tools")?
        .as_array()?
        .iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some(FETCH_TOOL))
        .cloned()
}

fn select_cases(filter: Option<&[String]>) -> Result<Vec<&ProbeCase>, String> {
    let Some(filter) = filter else {
        return Ok(CASES.iter().collect());
    };
    let mut selected = Vec::new();
    for name in filter {
        let case = CASES
            .iter()
            .find(|case| case.name == name)
            .ok_or_else(|| format!("unknown probe case: {name}"))?;
        if !selected.contains(&case) {
            selected.push(case);
        }
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_and_sse_responses() {
        assert!(parse_wire_json("application/json", br#"{"jsonrpc":"2.0"}"#).is_some());
        assert!(
            parse_wire_json(
                "text/event-stream",
                b"data: {\"jsonrpc\":\"2.0\"}\n\ndata: {\"jsonrpc\":\"2.0\"}\n\n",
            )
            .is_some()
        );
    }

    #[test]
    fn result_item_count_prefers_structured_content() {
        let result = json!({
            "content": [{"type": "text", "text": "ignored"}],
            "structuredContent": {"results": [{"url": "https://example.com/a"}, {"url": "https://example.com/b"}]}
        });
        assert_eq!(count_result_items(Some(&result)), 2);
    }

    #[test]
    fn result_item_count_falls_back_to_text_json() {
        let result = json!({
            "content": [{"type": "text", "text": "{\"results\":[{\"url\":\"https://example.com/a\"}]}"}]
        });
        assert_eq!(count_result_items(Some(&result)), 1);
    }

    #[test]
    fn partial_failure_shape_counts_error_items() {
        let result = json!({
            "structuredContent": {"results": [
                {"url": "https://example.com/a"},
                {"url": "https://example.com/b", "error": "not found"}
            ]}
        });
        let shape = partial_failure_shape(Some(&result));
        assert!(shape.contains("structured_results=2"));
        assert!(shape.contains("error_items=1"));
    }
}
