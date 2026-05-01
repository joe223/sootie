pub fn assert_tool_success(response: &serde_json::Value) {
    assert!(response.get("error").is_none());
}