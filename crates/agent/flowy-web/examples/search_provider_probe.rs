//! Explicit, developer-run compatibility probe for managed search providers.
//!
//! This example is deliberately independent from production MCP code. It logs
//! protocol/status/schema shape only, never the fixed query or response text.

use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};

const MODERN_VERSION: &str = "2026-07-28";
const LEGACY_VERSION: &str = "2025-11-25";
const MAX_PROBE_BODY: usize = 1_048_576;

#[derive(Clone, Copy)]
struct Provider {
    name: &'static str,
    endpoint: &'static str,
    tool: &'static str,
}

#[derive(Debug)]
struct WireResponse {
    status: reqwest::StatusCode,
    session_id: Option<String>,
    json: Option<Value>,
    byte_len: usize,
    elapsed: Duration,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()?;

    let providers = [
        Provider {
            name: "parallel",
            endpoint: "https://search.parallel.ai/mcp",
            tool: "web_search",
        },
        Provider {
            name: "exa",
            endpoint: "https://mcp.exa.ai/mcp?tools=web_search_exa",
            tool: "web_search_exa",
        },
    ];

    for provider in providers {
        println!("provider={} endpoint={}", provider.name, provider.endpoint);
        if let Err(error) = probe_provider(&client, provider).await {
            println!("provider={} outcome=failed error={error}", provider.name);
        }
    }

    Ok(())
}

async fn probe_provider(
    client: &reqwest::Client,
    provider: Provider,
) -> Result<(), Box<dyn std::error::Error>> {
    let discover = modern_request(
        client,
        provider,
        1,
        "server/discover",
        json!({}),
        None,
    )
    .await?;
    print_response(provider.name, "modern_discover", &discover);

    if is_modern_response(&discover) {
        let tools = modern_request(client, provider, 2, "tools/list", json!({}), None).await?;
        print_response(provider.name, "modern_tools_list", &tools);
        print_tool_catalog_shape(provider, &tools);
        let tool = find_tool(&tools, provider.tool)?;
        print_tool_shape(provider.name, tool);
        let arguments = minimum_arguments(tool);
        exercise_calls(client, provider, true, None, arguments).await?;
        return Ok(());
    }

    let initialize = legacy_request(
        client,
        provider,
        10,
        "initialize",
        json!({
            "protocolVersion": LEGACY_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "allo-search-probe", "version": "0"}
        }),
        None,
    )
    .await?;
    print_response(provider.name, "legacy_initialize", &initialize);
    let session_id = initialize.session_id.clone();
    if initialize
        .json
        .as_ref()
        .and_then(|value| value.get("result"))
        .is_none()
    {
        return Err("neither modern discovery nor legacy initialize succeeded".into());
    }

    let initialized = legacy_notification(
        client,
        provider,
        "notifications/initialized",
        json!({}),
        session_id.as_deref(),
    )
    .await?;
    print_response(provider.name, "legacy_initialized", &initialized);

    let tools = legacy_request(
        client,
        provider,
        11,
        "tools/list",
        json!({}),
        session_id.as_deref(),
    )
    .await?;
    print_response(provider.name, "legacy_tools_list", &tools);
    print_tool_catalog_shape(provider, &tools);
    let tool = find_tool(&tools, provider.tool)?;
    print_tool_shape(provider.name, tool);
    let arguments = minimum_arguments(tool);
    exercise_calls(
        client,
        provider,
        false,
        session_id.as_deref(),
        arguments,
    )
    .await?;
    Ok(())
}

async fn exercise_calls(
    client: &reqwest::Client,
    provider: Provider,
    modern: bool,
    session_id: Option<&str>,
    arguments: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    for sequence in 0..3_u64 {
        let response = call_tool(
            client,
            provider,
            modern,
            100 + sequence,
            session_id,
            arguments.clone(),
        )
        .await?;
        print_response(provider.name, "search_call", &response);
        print_tool_result_shape(provider.name, &response);
    }

    let left = call_tool(
        client,
        provider,
        modern,
        200,
        session_id,
        arguments.clone(),
    );
    let right = call_tool(client, provider, modern, 201, session_id, arguments);
    let (left, right) = tokio::join!(left, right);
    let left = left?;
    let right = right?;
    print_response(provider.name, "concurrent_search_call", &left);
    print_response(provider.name, "concurrent_search_call", &right);
    Ok(())
}

async fn call_tool(
    client: &reqwest::Client,
    provider: Provider,
    modern: bool,
    id: u64,
    session_id: Option<&str>,
    arguments: Value,
) -> Result<WireResponse, Box<dyn std::error::Error>> {
    let params = json!({"name": provider.tool, "arguments": arguments});
    if modern {
        modern_request(
            client,
            provider,
            id,
            "tools/call",
            params,
            Some(provider.tool),
        )
        .await
    } else {
        legacy_request(
            client,
            provider,
            id,
            "tools/call",
            params,
            session_id,
        )
        .await
    }
}

async fn modern_request(
    client: &reqwest::Client,
    provider: Provider,
    id: u64,
    method: &str,
    mut params: Value,
    tool_name: Option<&str>,
) -> Result<WireResponse, Box<dyn std::error::Error>> {
    let object = params
        .as_object_mut()
        .ok_or("probe params must be a JSON object")?;
    object.insert(
        "_meta".into(),
        json!({
            "io.modelcontextprotocol/protocolVersion": MODERN_VERSION,
            "io.modelcontextprotocol/clientInfo": {
                "name": "allo-search-probe",
                "version": "0"
            },
            "io.modelcontextprotocol/clientCapabilities": {}
        }),
    );
    let mut headers = standard_headers();
    headers.insert(
        HeaderName::from_static("mcp-protocol-version"),
        HeaderValue::from_static(MODERN_VERSION),
    );
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
        provider.endpoint,
        headers,
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
    )
    .await
}

async fn legacy_request(
    client: &reqwest::Client,
    provider: Provider,
    id: u64,
    method: &str,
    params: Value,
    session_id: Option<&str>,
) -> Result<WireResponse, Box<dyn std::error::Error>> {
    let mut headers = standard_headers();
    headers.insert(
        HeaderName::from_static("mcp-protocol-version"),
        HeaderValue::from_static(LEGACY_VERSION),
    );
    if let Some(session_id) = session_id {
        headers.insert(
            HeaderName::from_static("mcp-session-id"),
            HeaderValue::from_str(session_id)?,
        );
    }
    send(
        client,
        provider.endpoint,
        headers,
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
    )
    .await
}

async fn legacy_notification(
    client: &reqwest::Client,
    provider: Provider,
    method: &str,
    params: Value,
    session_id: Option<&str>,
) -> Result<WireResponse, Box<dyn std::error::Error>> {
    let mut headers = standard_headers();
    headers.insert(
        HeaderName::from_static("mcp-protocol-version"),
        HeaderValue::from_static(LEGACY_VERSION),
    );
    if let Some(session_id) = session_id {
        headers.insert(
            HeaderName::from_static("mcp-session-id"),
            HeaderValue::from_str(session_id)?,
        );
    }
    send(
        client,
        provider.endpoint,
        headers,
        json!({"jsonrpc": "2.0", "method": method, "params": params}),
    )
    .await
}

fn standard_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers
}

async fn send(
    client: &reqwest::Client,
    endpoint: &str,
    headers: HeaderMap,
    body: Value,
) -> Result<WireResponse, Box<dyn std::error::Error>> {
    let started = Instant::now();
    let response = client
        .post(endpoint)
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

fn find_tool<'a>(
    response: &'a WireResponse,
    tool_name: &str,
) -> Result<&'a Value, Box<dyn std::error::Error>> {
    response
        .json
        .as_ref()
        .and_then(|json| json.get("result"))
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .and_then(|tools| {
            tools
                .iter()
                .find(|tool| tool.get("name").and_then(Value::as_str) == Some(tool_name))
        })
        .ok_or_else(|| format!("expected tool {tool_name} was not discovered").into())
}

fn print_tool_catalog_shape(provider: Provider, response: &WireResponse) {
    let tools = response
        .json
        .as_ref()
        .and_then(|json| json.get("result"))
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array);
    let tool_count = tools.map_or(0, Vec::len);
    let unexpected_tools = tools.map_or(0, |tools| {
        tools
            .iter()
            .filter(|tool| tool.get("name").and_then(Value::as_str) != Some(provider.tool))
            .count()
    });
    println!(
        "provider={} tool_count={tool_count} unexpected_tools={unexpected_tools}",
        provider.name
    );
}

fn minimum_arguments(tool: &Value) -> Value {
    let schema = tool.get("inputSchema").unwrap_or(&Value::Null);
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut arguments = serde_json::Map::new();
    for field in required.iter().filter_map(Value::as_str) {
        let property_type = properties
            .get(field)
            .and_then(|property| property.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("string");
        let value = match property_type {
            "integer" | "number" => json!(1),
            "boolean" => json!(false),
            "array" => json!(["managed web search compatibility"]),
            "object" => json!({}),
            _ => json!("managed web search compatibility"),
        };
        arguments.insert(field.to_owned(), value);
    }
    for query_name in ["query", "search_query"] {
        if properties.contains_key(query_name) && !arguments.contains_key(query_name) {
            arguments.insert(
                query_name.to_owned(),
                json!("managed web search compatibility"),
            );
        }
    }
    for count_name in ["count", "num_results", "numResults"] {
        if properties.contains_key(count_name) && !arguments.contains_key(count_name) {
            arguments.insert(count_name.to_owned(), json!(1));
        }
    }
    Value::Object(arguments)
}

fn print_response(provider: &str, operation: &str, response: &WireResponse) {
    let error_code = response
        .json
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64);
    println!(
        "provider={provider} operation={operation} status={} bytes={} elapsed_ms={} session_id={} json={} error_code={}",
        response.status.as_u16(),
        response.byte_len,
        response.elapsed.as_millis(),
        response.session_id.is_some(),
        response.json.is_some(),
        error_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "none".into())
    );
}

fn print_tool_shape(provider: &str, tool: &Value) {
    let required = tool
        .get("inputSchema")
        .and_then(|schema| schema.get("required"))
        .and_then(Value::as_array)
        .map(|items| items.len())
        .unwrap_or(0);
    let property_names = tool
        .get("inputSchema")
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    println!(
        "provider={provider} tool={} required_fields={required} properties={}",
        tool.get("name").and_then(Value::as_str).unwrap_or("unknown"),
        property_names.join(",")
    );
}

fn print_tool_result_shape(provider: &str, response: &WireResponse) {
    let result = response
        .json
        .as_ref()
        .and_then(|value| value.get("result"));
    let is_error = result
        .and_then(|value| value.get("isError"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let content = result
        .and_then(|value| value.get("content"))
        .and_then(Value::as_array);
    let text_chars = content
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .map(str::chars)
        .map(Iterator::count)
        .sum::<usize>();
    let parsed_text = content
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .find_map(|text| serde_json::from_str::<Value>(text).ok());
    let text_values = content
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let url_lines = text_values
        .iter()
        .flat_map(|text| text.lines())
        .filter(|line| line.contains("http://") || line.contains("https://"))
        .count();
    let labelled = text_values.iter().any(|text| {
        text.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("Title:") || line.starts_with("URL:") || line.starts_with("Text:")
        })
    });
    let top_level = parsed_text
        .as_ref()
        .map(|value| match value {
            Value::Object(object) => object.keys().cloned().collect::<Vec<_>>().join(","),
            Value::Array(_) => "array".to_owned(),
            _ => "scalar".to_owned(),
        })
        .unwrap_or_else(|| "non_json".to_owned());
    let url_objects = parsed_text.as_ref().map(count_url_objects).unwrap_or(0);
    println!(
        "provider={provider} result_is_error={is_error} content_items={} text_chars={text_chars} text_shape={top_level} url_objects={url_objects} url_lines={url_lines} labelled={labelled}",
        content.map_or(0, |items| items.len())
    );
}

fn count_url_objects(value: &Value) -> usize {
    match value {
        Value::Object(object) => {
            usize::from(
                ["url", "link", "href"]
                    .iter()
                    .any(|key| object.get(*key).and_then(Value::as_str).is_some()),
            ) + object.values().map(count_url_objects).sum::<usize>()
        }
        Value::Array(items) => items.iter().map(count_url_objects).sum(),
        _ => 0,
    }
}
