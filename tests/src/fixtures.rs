pub struct FixturesLoader;

impl FixturesLoader {
    pub fn load_html_page(name: &str) -> anyhow::Result<String> {
        Ok(format!("Placeholder for {}", name))
    }
}