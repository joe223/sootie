pub struct TestEnv;

impl TestEnv {
    pub fn launch() -> anyhow::Result<Self> {
        Ok(Self)
    }
}