use std::io::{BufRead, BufReader, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::joplin::JoplinClient;
use crate::tools;

const PROTOCOL_VERSION: &str = "2024-11-05";

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
    let client_version = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION);

    serde_json::json!({
        "protocolVersion": client_version,
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "joplin-mcp",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn handle_tools_list() -> Value {
    serde_json::json!({
        "tools": tools::all_tools()
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
        "tools/list" => Ok(handle_tools_list()),
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

    fn test_client() -> JoplinClient {
        JoplinClient::new(Config {
            base_url: "http://127.0.0.1:41184".to_string(),
            token: Some("test-token".to_string()),
            timeout_secs: 5,
        })
    }

    #[test]
    fn rejects_invalid_jsonrpc_version() {
        let client = test_client();
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
        let client = test_client();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: None,
        };
        assert!(process_request(&client, req).is_none());
    }

    #[test]
    fn initialize_and_tools_list_succeed() {
        let client = test_client();
        let init_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({ "protocolVersion": "2024-11-05" })),
        };
        let init_resp = process_request(&client, init_req);
        assert!(init_resp.is_some());
        if let Some(resp) = init_resp {
            assert!(resp.error.is_none());
            assert!(resp.result.is_some());
        }

        let list_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::from(2)),
            method: "tools/list".to_string(),
            params: None,
        };
        let list_resp = process_request(&client, list_req);
        assert!(list_resp.is_some());
        if let Some(resp) = list_resp {
            assert!(resp.error.is_none());
            let count = resp
                .result
                .as_ref()
                .and_then(|r| r.get("tools"))
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            assert!(count >= 20);
        }
    }
}
