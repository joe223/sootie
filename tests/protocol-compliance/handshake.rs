use serde_json::json;

#[test]
fn test_mcp_handshake_request_format() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        }
    });

    assert!(request["jsonrpc"] == "2.0");
    assert!(request["method"] == "initialize");
}
