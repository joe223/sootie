use sootie_tests::{TestEnv, FixturesLoader, assert_tool_success};

#[tokio::test]
async fn test_form_submission_basic() {
    let mut env = TestEnv::launch().unwrap();
    assert!(env.chrome.is_none());
}

#[tokio::test]
async fn test_fixtures_loaded() {
    let html = FixturesLoader::load_html_page("form-test.html").unwrap();
    assert!(html.contains("<form"));
    assert!(html.contains("type=\"email\""));
}