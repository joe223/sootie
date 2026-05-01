use anyhow::Result;
use std::process::{Child, Command};
use std::net::TcpListener;

pub struct ChromeProcess {
    process: Child,
    port: u16,
}

pub struct HttpServer {
    process: Child,
    port: u16,
}

pub struct TestEnv {
    chrome: Option<ChromeProcess>,
    http_server: Option<HttpServer>,
}

impl TestEnv {
    pub fn launch() -> Result<Self> {
        Ok(Self {
            chrome: None,
            http_server: None,
        })
    }
    
    pub fn launch_chrome(&mut self) -> Result<u16> {
        let port = find_available_port(9222, 9230)?;
        let process = Command::new("google-chrome-stable")
            .args([
                "--headless",
                "--disable-gpu",
                &format!("--remote-debugging-port={}", port),
                "--no-first-run",
            ])
            .spawn()?;
        
        self.chrome = Some(ChromeProcess { process, port });
        Ok(port)
    }
    
    pub fn chrome_url(&self) -> Option<String> {
        self.chrome.as_ref().map(|c| format!("http://localhost:{}", c.port))
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        if let Some(mut chrome) = self.chrome.take() {
            let _ = chrome.process.kill();
        }
        if let Some(mut server) = self.http_server.take() {
            let _ = server.process.kill();
        }
    }
}

fn find_available_port(start: u16, end: u16) -> Result<u16> {
    for port in start..=end {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    anyhow::bail!("No available port in range {}-{}", start, end)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_env_launch_creates_empty_env() {
        let env = TestEnv::launch().unwrap();
        assert!(env.chrome.is_none());
        assert!(env.http_server.is_none());
    }
    
    #[test]
    fn test_find_available_port() {
        let port = find_available_port(8080, 8090).unwrap();
        assert!(port >= 8080 && port <= 8090);
    }
}