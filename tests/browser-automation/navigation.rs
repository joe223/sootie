use sootie_tests::FixturesLoader;

#[tokio::test]
async fn test_navigation_click_link() {
    let html = FixturesLoader::load_html_page("navigation-test.html").unwrap();
    assert!(html.contains("next-link"));
}