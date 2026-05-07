use sootie_tests::{FixturesLoader, TestEnv};

#[tokio::test]
async fn test_form_submission_basic() {
    let _env = TestEnv::launch().unwrap();
}

#[tokio::test]
async fn test_fixtures_loaded() {
    let html = FixturesLoader::load_html_page("form-test.html").unwrap();
    assert!(html.contains("<form"));
    assert!(html.contains("type=\"email\""));
}
