use std::io::{BufRead, BufReader, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::joplin::JoplinClient;
use crate::tools;

const PROTOCOL_VERSION: &str = "2024-11-05";
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[PROTOCOL_VERSION];

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

fn write_message<T: Serialize>(message: &T) {
    if let Ok(json) = serde_json::to_string(message) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{json}");
        let _ = out.flush();
    }
}

fn handle_initialize(params: Option<&Value>) -> Value {
    let requested = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(Value::as_str);

    let negotiated = requested
        .filter(|v| SUPPORTED_PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(PROTOCOL_VERSION);

    serde_json::json!({
        "protocolVersion": negotiated,
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "joplin-mcp",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn handle_tools_list(client: &JoplinClient) -> Value {
    serde_json::json!({
        "tools": tools::all_tools(client.allow_delete())
    })
}

fn handle_tools_call(client: &JoplinClient, params: Option<&Value>) -> Result<Value, JsonRpcError> {
    let Some(params) = params else {
        return Err(JsonRpcError {
            code: -32602,
            message: "Missing params for tools/call".to_string(),
        });
    };

    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err(JsonRpcError {
            code: -32602,
            message: "Missing tool name in params".to_string(),
        });
    };

    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

    match tools::dispatch(client, name, &args) {
        Ok(text) => Ok(serde_json::json!({
            "content": [{ "type": "text", "text": text }],
            "isError": false
        })),
        Err(err) => Ok(serde_json::json!({
            "content": [{ "type": "text", "text": err.to_string() }],
            "isError": true
        })),
    }
}

fn process_request(client: &JoplinClient, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
    if request.jsonrpc != "2.0" {
        return Some(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: format!("Invalid JSON-RPC version: {}", request.jsonrpc),
            }),
        });
    }

    // Notifications have no `id` and expect no response.
    let id = request.id?;

    let outcome = match request.method.as_str() {
        "initialize" => Ok(handle_initialize(request.params.as_ref())),
        "ping" => Ok(serde_json::json!({})),
        "tools/list" => Ok(handle_tools_list(client)),
        "tools/call" => handle_tools_call(client, request.params.as_ref()),
        other => Err(JsonRpcError {
            code: -32601,
            message: format!("Method not found: {other}"),
        }),
    };

    Some(match outcome {
        Ok(result) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            result: Some(result),
            error: None,
        },
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            result: None,
            error: Some(error),
        },
    })
}

/// Run the MCP JSON-RPC 2.0 server over stdio until EOF.
pub fn serve(client: &JoplinClient) {
    let stdin = std::io::stdin();
    let reader = BufReader::new(stdin.lock());

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("joplin-mcp: error reading stdin: {e}");
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                write_message(&JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {e}"),
                    }),
                });
                continue;
            }
        };

        if let Some(response) = process_request(client, request) {
            write_message(&response);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_client(allow_delete: bool) -> JoplinClient {
        JoplinClient::new(Config {
            url_override: None,
            host: "127.0.0.1".to_string(),
            port_override: None,
            base_url: "http://127.0.0.1:41184".to_string(),
            token: Some("test-token".to_string()),
            allow_delete,
            timeout_secs: 5,
        })
    }

    #[test]
    fn rejects_invalid_jsonrpc_version() {
        let client = test_client(false);
        let req = JsonRpcRequest {
            jsonrpc: "1.0".to_string(),
            id: Some(Value::from(1)),
            method: "initialize".to_string(),
            params: None,
        };
        let resp = process_request(&client, req);
        assert!(resp.is_some());
        if let Some(resp) = resp {
            assert!(resp.error.is_some());
        }
    }

    #[test]
    fn notifications_return_none() {
        let client = test_client(false);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: None,
        };
        assert!(process_request(&client, req).is_none());
    }

    #[test]
    fn initialize_negotiates_supported_protocol_version() {
        let client = test_client(false);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({ "protocolVersion": "2099-01-01" })),
        };
        let resp = process_request(&client, req);
        assert!(resp.is_some());
        if let Some(resp) = resp {
            let negotiated = resp
                .result
                .as_ref()
                .and_then(|r| r.get("protocolVersion"))
                .and_then(Value::as_str);
            assert_eq!(negotiated, Some(PROTOCOL_VERSION));
        }
    }

    #[test]
    fn delete_tools_gated_behind_allow_delete() {
        let safe_client = test_client(false);
        let list_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(2)),
            method: "tools/list".to_string(),
            params: None,
        };
        let list_resp = process_request(&safe_client, list_req);
        assert!(list_resp.is_some());
        if let Some(resp) = list_resp {
            let tools = resp
                .result
                .as_ref()
                .and_then(|r| r.get("tools"))
                .and_then(Value::as_array);
            assert_eq!(tools.map(Vec::len), Some(22));
            let has_delete = tools.is_some_and(|arr| {
                arr.iter().any(|t| {
                    t.get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|n| n.starts_with("delete_"))
                })
            });
            assert!(!has_delete);
        }

        let call_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(3)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "delete_note",
                "arguments": { "note_id": "abc" }
            })),
        };
        let call_resp = process_request(&safe_client, call_req);
        assert!(call_resp.is_some());
        if let Some(resp) = call_resp {
            let is_err = resp
                .result
                .as_ref()
                .and_then(|r| r.get("isError"))
                .and_then(Value::as_bool);
            assert_eq!(is_err, Some(true));
        }

        let full_client = test_client(true);
        let full_list_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(4)),
            method: "tools/list".to_string(),
            params: None,
        };
        let full_resp = process_request(&full_client, full_list_req);
        assert!(full_resp.is_some());
        if let Some(resp) = full_resp {
            let count = resp
                .result
                .as_ref()
                .and_then(|r| r.get("tools"))
                .and_then(Value::as_array)
                .map(Vec::len);
            assert_eq!(count, Some(25));
        }
    }
}
