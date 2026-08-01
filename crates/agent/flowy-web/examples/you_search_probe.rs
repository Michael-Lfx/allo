//! PROTOTYPE — developer-run You.com MCP compatibility probe.
//!
//! Question: does the keyless You.com endpoint support sessionless MCP
//! initialization and return a schema-backed structured search result that a
//! future provider-specific decoder can consume?
//!
//! This example is isolated from production routing and `RemoteMcpPeer`.
//! It performs one real search per run:
//!
//! ```text
//! cargo run -p flowy-web --example you_search_probe -- \
//!   --query "latest Model Context Protocol changes" --count 3
//! ```
//!
//! Add `--raw` only when intentionally inspecting the full public response.

use std::time::{Duration, Instant};

use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde_json::{Value, json};

const ENDPOINT: &str = "https://api.you.com/mcp?profile=free";
const PROTOCOL_VERSION: &str = "2025-11-25";
const TOOL_NAME: &str = "you-search";
const MAX_BODY_BYTES: usize = 1024 * 1024;

struct Options {
    query: String,
    count: u64,
    raw: bool,
    admission: bool,
}

struct WireResponse {
    status: StatusCode,
    content_type: String,
    session_id: Option<String>,
    elapsed: Duration,
    bytes: Vec<u8>,
    json: Option<Value>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_options()?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    println!("PROTOTYPE: You.com sessionless MCP search");
    println!("endpoint={ENDPOINT}");

    let initialize = send(
        &client,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "allo-you-search-probe", "version": "0.1.0"}
            }
        }),
        None,
    )
    .await?;
    print_wire("initialize", &initialize);
    require_success(&initialize)?;
    let negotiated_version = initialize
        .json
        .as_ref()
        .and_then(|value| value.pointer("/result/protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let session_mode = if initialize.session_id.is_some() {
        "stateful"
    } else {
        "sessionless"
    };
    println!("negotiated_version={negotiated_version} session_mode={session_mode}");

    let initialized = send(
        &client,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
        initialize.session_id.as_deref(),
    )
    .await?;
    print_wire("notifications/initialized", &initialized);
    if initialized.status != StatusCode::ACCEPTED && !initialized.status.is_success() {
        return Err(format!("initialized notification returned {}", initialized.status).into());
    }

    let tools = send(
        &client,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }),
        initialize.session_id.as_deref(),
    )
    .await?;
    print_wire("tools/list", &tools);
    require_success(&tools)?;
    let tool = tools
        .json
        .as_ref()
        .and_then(|value| value.pointer("/result/tools"))
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("name").and_then(Value::as_str) == Some(TOOL_NAME))
        })
        .ok_or("you-search was not present in tools/list")?;
    let input_schema = tool.get("inputSchema").unwrap_or(&Value::Null);
    let output_schema = tool.get("outputSchema");
    let properties = input_schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or("you-search inputSchema omitted properties")?;
    if !properties.contains_key("query") {
        return Err("you-search inputSchema omitted query".into());
    }
    let tool_count = tools
        .json
        .as_ref()
        .and_then(|value| value.pointer("/result/tools"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    println!(
        "tool_count={tool_count} selected_tool={TOOL_NAME} input_fields={} output_schema={}",
        properties.keys().cloned().collect::<Vec<_>>().join(","),
        output_schema.is_some()
    );

    let count = options.count.clamp(1, 10);
    let search = send(
        &client,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": TOOL_NAME,
                "arguments": {
                    "query": options.query.clone(),
                    "count": count
                }
            }
        }),
        initialize.session_id.as_deref(),
    )
    .await?;
    print_wire("tools/call", &search);
    require_success(&search)?;
    print_result_shape(&search);

    if options.admission {
        run_admission(&client, initialize.session_id.as_deref(), &options.query, count).await?;
    }

    if options.raw {
        println!("raw_tools_list={}", String::from_utf8_lossy(&tools.bytes));
        println!("raw_tool_result={}", String::from_utf8_lossy(&search.bytes));
    }

    Ok(())
}

fn parse_options() -> Result<Options, Box<dyn std::error::Error>> {
    let mut query = "latest Model Context Protocol changes".to_owned();
    let mut count = 3_u64;
    let mut raw = false;
    let mut admission = false;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--query" => query = args.next().ok_or("--query requires a value")?,
            "--count" => {
                count = args
                    .next()
                    .ok_or("--count requires a value")?
                    .parse::<u64>()?
            }
            "--raw" => raw = true,
            "--admission" => admission = true,
            unknown => return Err(format!("unknown argument: {unknown}").into()),
        }
    }
    if query.trim().is_empty() {
        return Err("query must not be blank".into());
    }
    Ok(Options {
        query,
        count,
        raw,
        admission,
    })
}

async fn run_admission(
    client: &Client,
    session_id: Option<&str>,
    query: &str,
    count: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("admission=sequential_calls count=3");
    for id in 10..13_u64 {
        let response = call_search(client, session_id, id, query, count).await?;
        print_wire("admission_sequential", &response);
        require_success(&response)?;
        print_result_shape(&response);
    }

    println!("admission=concurrent_calls count=2");
    let left = call_search(client, session_id, 20, query, count);
    let right = call_search(client, session_id, 21, query, count);
    let (left, right) = tokio::join!(left, right);
    for response in [left?, right?] {
        print_wire("admission_concurrent", &response);
        require_success(&response)?;
        print_result_shape(&response);
    }
    Ok(())
}

async fn call_search(
    client: &Client,
    session_id: Option<&str>,
    id: u64,
    query: &str,
    count: u64,
) -> Result<WireResponse, Box<dyn std::error::Error>> {
    send(
        client,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": TOOL_NAME,
                "arguments": {"query": query, "count": count}
            }
        }),
        session_id,
    )
    .await
}

async fn send(
    client: &Client,
    body: Value,
    session_id: Option<&str>,
) -> Result<WireResponse, Box<dyn std::error::Error>> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(
        "MCP-Protocol-Version",
        HeaderValue::from_static(PROTOCOL_VERSION),
    );
    if let Some(session_id) = session_id {
        headers.insert("Mcp-Session-Id", HeaderValue::from_str(session_id)?);
    }

    let started = Instant::now();
    let response = client
        .post(ENDPOINT)
        .headers(headers)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let session_id = response
        .headers()
        .get("Mcp-Session-Id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = response.bytes().await?.to_vec();
    if bytes.len() > MAX_BODY_BYTES {
        return Err(format!("response exceeded {MAX_BODY_BYTES} bytes").into());
    }
    let json = if bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(&bytes)?)
    };
    Ok(WireResponse {
        status,
        content_type,
        session_id,
        elapsed: started.elapsed(),
        bytes,
        json,
    })
}

fn require_success(response: &WireResponse) -> Result<(), Box<dyn std::error::Error>> {
    if !response.status.is_success() {
        return Err(format!("HTTP {}", response.status).into());
    }
    if let Some(error) = response
        .json
        .as_ref()
        .and_then(|value| value.get("error"))
    {
        return Err(format!("JSON-RPC error: {error}").into());
    }
    Ok(())
}

fn print_wire(operation: &str, response: &WireResponse) {
    println!(
        "operation={operation} status={} content_type={} session_id={} elapsed_ms={} bytes={}",
        response.status.as_u16(),
        response.content_type,
        response.session_id.is_some(),
        response.elapsed.as_millis(),
        response.bytes.len()
    );
}

fn print_result_shape(response: &WireResponse) {
    let result = response
        .json
        .as_ref()
        .and_then(|value| value.get("result"));
    let structured = result.and_then(|value| value.get("structuredContent"));
    let web_count = structured
        .and_then(|value| value.pointer("/results/web"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let news_count = structured
        .and_then(|value| value.pointer("/results/news"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
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
    println!(
        "structured_content={} structured_keys={} web_results={web_count} news_results={news_count} content_items={} text_chars={text_chars}",
        structured.is_some(),
        structured
            .and_then(Value::as_object)
            .map(|object| object.keys().cloned().collect::<Vec<_>>().join(","))
            .unwrap_or_default(),
        content.map_or(0, Vec::len)
    );
}
