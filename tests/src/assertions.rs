use sootie_core::selector::Bounds;
use sootie_core::selector::Coordinate;
use serde_json::Value;

pub fn assert_coordinate_in_bounds(coord: Coordinate, bounds: Bounds) {
    assert!(
        coord.x >= bounds.x && coord.x <= bounds.x + bounds.width,
        "Coordinate x={} not in bounds [{}, {}]",
        coord.x, bounds.x, bounds.x + bounds.width
    );
    assert!(
        coord.y >= bounds.y && coord.y <= bounds.y + bounds.height,
        "Coordinate y={} not in bounds [{}, {}]",
        coord.y, bounds.y, bounds.y + bounds.height
    );
}

pub fn assert_jsonrpc_compliant(response: &Value) {
    assert!(response.get("jsonrpc").is_some(), "Missing jsonrpc version");
    assert!(
        response["jsonrpc"] == "2.0",
        "Invalid jsonrpc version: {}",
        response["jsonrpc"]
    );
}

pub fn assert_tool_success(response: &Value) {
    assert_jsonrpc_compliant(response);
    assert!(
        response.get("error").is_none(),
        "Tool call failed with error: {:?}",
        response.get("error")
    );
}

pub fn assert_tool_error(response: &Value, expected_code: i32) {
    assert_jsonrpc_compliant(response);
    let error = response.get("error").expect("Expected error field");
    assert!(
        error["code"] == expected_code,
        "Error code mismatch: expected {}, got {}",
        expected_code, error["code"]
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_assert_coordinate_in_bounds() {
        let coord = Coordinate { x: 50.0, y: 50.0 };
        let bounds = Bounds { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };
        assert_coordinate_in_bounds(coord, bounds);
    }
    
    #[test]
    fn test_assert_jsonrpc_compliant() {
        let response = serde_json::json!({"jsonrpc": "2.0", "result": {}});
        assert_jsonrpc_compliant(&response);
    }
}