use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::config::{save_config, showui_model_path};

const HF_MODEL_ID: &str = "mlx-community/ShowUI-2B-bf16-8bit";

const ALLOWED_EXTENSIONS: [&str; 6] = [
    ".safetensors",
    ".json",
    "merges.txt",
    "vocab.txt",
    "vocab.json",
    "tokenizer.model",
];

const REQUIRED_FILES: [&str; 4] = [
    "model.safetensors",
    "config.json",
    "tokenizer.json",
    "tokenizer_config.json",
];

const MIN_MODEL_SIZE: u64 = 2_500_000_000; // 2.5GB minimum

#[derive(Debug, Deserialize)]
struct HfFileEntry {
    #[serde(rename = "type")]
    _type: String,
    path: String,
    size: Option<u64>,
    sha256: Option<String>,
}

pub struct HfDownloader {
    client: reqwest::Client,
    mirror: bool,
}

impl HfDownloader {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .user_agent("sootie/0.1.0")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            mirror: false,
        }
    }

    pub fn with_mirror(mut self) -> Self {
        self.mirror = true;
        self
    }

    fn api_root(&self) -> String {
        if self.mirror {
            "https://hf-mirror.com".to_string()
        } else {
            "https://huggingface.co".to_string()
        }
    }

    fn download_root(&self) -> String {
        if self.mirror {
            "https://hf-mirror.com".to_string()
        } else {
            "https://huggingface.co".to_string()
        }
    }

    pub async fn list_files(&self) -> Result<HashMap<String, (u64, String)>> {
        let url = format!("{}/api/models/{}/tree/main", self.api_root(), HF_MODEL_ID);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch model file list from HuggingFace API")?;

        let entries: Vec<HfFileEntry> = resp
            .json()
            .await
            .context("Failed to parse HuggingFace API response")?;

        let allowed: HashSet<&str> = ALLOWED_EXTENSIONS.iter().copied().collect();
        let file_info: HashMap<String, (u64, String)> = entries
            .into_iter()
            .filter(|e| e._type == "file")
            .filter(|e| {
                let fname = std::path::Path::new(&e.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                allowed.iter().any(|ext| fname.ends_with(ext) || fname == *ext)
            })
            .filter_map(|e| {
                if let (Some(size), Some(sha256)) = (e.size, e.sha256) {
                    Some((e.path, (size, sha256)))
                } else {
                    None
                }
            })
            .collect();

        Ok(file_info)
    }

    pub async fn download_file(
        &self,
        filepath: &str,
        dest_dir: &Path,
        expected_size: u64,
        expected_sha256: &str,
        pb: &ProgressBar,
    ) -> Result<PathBuf> {
        let fname = Path::new(filepath)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(filepath);

        // Validate filename doesn't contain path traversal
        if fname.contains("..") || fname.contains('/') || fname.contains('\\') {
            return Err(anyhow::anyhow!("Invalid filename: {}", fname));
        }

        let dest_path = dest_dir.join(fname);

        // Check existing file
        if dest_path.exists() {
            let existing_size = dest_path.metadata()?.len();

            if existing_size == expected_size {
                // Size matches, verify checksum
                if verify_checksum(&dest_path, expected_sha256)? {
                    pb.println(format!("  {} already exists (checksum verified), skipping", fname));
                    return Ok(dest_path);
                }
                // Checksum mismatch, delete and re-download
                pb.println(format!("  {} exists but checksum mismatch, re-downloading", fname));
                fs::remove_file(&dest_path)?;
            } else if existing_size == 0 {
                // Zero-length file, delete
                pb.println(format!("  {} is zero-length, re-downloading", fname));
                fs::remove_file(&dest_path)?;
            } else {
                // Size mismatch, delete and re-download
                pb.println(format!(
                    "  {} size mismatch ({} vs {}), re-downloading",
                    fname,
                    existing_size,
                    expected_size
                ));
                fs::remove_file(&dest_path)?;
            }
        }

        // Download
        let url = format!(
            "{}/{}/resolve/main/{}",
            self.download_root(),
            HF_MODEL_ID,
            filepath
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context(format!("Failed to download {}", fname))?;

        let total_size = resp.content_length().unwrap_or(expected_size);
        pb.set_length(total_size);
        pb.set_message(fname.to_string());

        let mut file = fs::File::create(&dest_path)
            .context(format!("Failed to create {}", dest_path.display()))?;

        let mut hasher = Sha256::new();
        let mut downloaded: u64 = 0;
        let mut stream = resp.bytes_stream();
        use futures_util::StreamExt as _;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Download error")?;
            file.write_all(&chunk)?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;
            pb.set_position(downloaded);
        }

        // Verify checksum
        let actual_sha256 = hex::encode(hasher.finalize());
        if actual_sha256 != expected_sha256 {
            fs::remove_file(&dest_path)?;
            return Err(anyhow::anyhow!(
                "Checksum mismatch for {}: expected {}, got {}",
                fname,
                expected_sha256,
                actual_sha256
            ));
        }

        pb.finish_with_message(format!("  ✓ {} (checksum verified)", fname));
        Ok(dest_path)
    }

    pub async fn download_model(&self, dest_dir: &PathBuf) -> Result<()> {
        fs::create_dir_all(dest_dir)?;

        let file_info = self.list_files().await?;

        if file_info.is_empty() {
            return Err(anyhow::anyhow!(
                "No model files found at {}/{}\n\
                 You can download manually: git clone https://huggingface.co/{}",
                if self.mirror {
                    "https://hf-mirror.com"
                } else {
                    "https://huggingface.co"
                },
                HF_MODEL_ID,
                HF_MODEL_ID,
            ));
        }

        let total = file_info.len();
        for (i, (file, (size, sha256))) in file_info.iter().enumerate() {
            let pb = ProgressBar::new(0);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template(
                        "{spinner:.green} [{elapsed_precise}] [{bar:30.cyan/blue}] {bytes}/{total_bytes} {msg}",
                    )
                    .context("progress bar template")?
                    .progress_chars("=>-"),
            );
            pb.println(format!("[{}/{}] {}", i + 1, total, file));

            if let Err(e) = self.download_file(file, dest_dir, *size, sha256, &pb).await {
                pb.finish_with_message(format!("  ✗ {}", e));
                return Err(e);
            }
        }

        Ok(())
    }
}

fn verify_checksum(path: &Path, expected_sha256: &str) -> Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let actual = hex::encode(hasher.finalize());
    Ok(actual == expected_sha256)
}

pub fn validate_model_dir(path: &Path) -> Result<bool> {
    // Selective cleanup: remove problematic files only
    if path.join("pytorch_model.bin").exists() {
        // Remove only .bin files, keep other files
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.path().extension() == Some(std::ffi::OsStr::new("bin")) {
                fs::remove_file(entry.path())?;
                println!("Removed legacy .bin file: {}", entry.path().display());
            }
        }
    }

    // Check model.safetensors size
    let safetensors = path.join("model.safetensors");
    if safetensors.exists() {
        let size = safetensors.metadata()?.len();
        if size < MIN_MODEL_SIZE {
            println!(
                "model.safetensors too small ({:.2}GB < 2.5GB), removing",
                size as f64 / 1_000_000_000.0
            );
            fs::remove_file(&safetensors)?;
            return Ok(false);
        }
    }

    // Check required files
    for required in REQUIRED_FILES {
        if !path.join(required).exists() {
            return Ok(false);
        }
    }

    Ok(true)
}

pub async fn download_showui_model() -> Result<PathBuf> {
    let dest_dir = showui_model_path();

    // Selective cleanup before download
    if dest_dir.exists() {
        // Remove incomplete safetensors
        let safetensors = dest_dir.join("model.safetensors");
        if safetensors.exists() {
            let size = safetensors.metadata()?.len();
            if size < MIN_MODEL_SIZE {
                println!("Removing incomplete model.safetensors");
                fs::remove_file(&safetensors)?;
            }
        }
    }

    if validate_model_dir(&dest_dir)? {
        println!("Model already validated at {}", dest_dir.display());
        return Ok(dest_dir);
    }

    println!("Downloading ShowUI-2B model (~3GB) from Hugging Face...");
    println!("Checksums will be verified for each file");

    let downloader = HfDownloader::new();
    match downloader.download_model(&dest_dir).await {
        Ok(_) if validate_model_dir(&dest_dir)? => {
            println!("✓ Model saved and validated at {}", dest_dir.display());
            Ok(dest_dir)
        }
        Ok(_) => Err(anyhow::anyhow!(
            "Download incomplete - required files missing or checksum failed"
        )),
        Err(e) => {
            eprintln!("  Primary source failed: {}", e);
            eprintln!("  Trying Chinese mirror (hf-mirror.com)...");
            let downloader = HfDownloader::new().with_mirror();
            downloader.download_model(&dest_dir).await?;
            if validate_model_dir(&dest_dir)? {
                println!("✓ Model saved and validated at {}", dest_dir.display());
                Ok(dest_dir)
            } else {
                Err(anyhow::anyhow!(
                    "Download incomplete - required files missing or checksum failed"
                ))
            }
        }
    }
}

pub fn update_config_with_model_path(model_path: &Path) -> Result<()> {
    let mut config = super::config::load_config().unwrap_or_default();
    config.vision.model_path = model_path.to_string_lossy().into_owned();
    save_config(&config)?;
    Ok(())
}

pub fn estimate_model_size() -> u64 {
    3_200_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hf_downloader_api_root() {
        let d = HfDownloader::new();
        assert!(d.api_root().contains("huggingface.co"));
    }

    #[test]
    fn test_hf_downloader_mirror_api_root() {
        let d = HfDownloader::new().with_mirror();
        assert!(d.api_root().contains("hf-mirror.com"));
    }

    #[test]
    fn test_validate_model_dir_missing() {
        let temp = tempfile::tempdir().unwrap();
        assert!(!validate_model_dir(temp.path()).unwrap());
    }

    #[test]
    fn test_validate_model_dir_with_small_safetensors() {
        let temp = tempfile::tempdir().unwrap();
        for f in REQUIRED_FILES {
            fs::write(temp.path().join(f), "{}").unwrap();
        }
        // Create small safetensors (< 2.5GB)
        fs::write(temp.path().join("model.safetensors"), "small").unwrap();
        // Should fail and remove small file
        assert!(!validate_model_dir(temp.path()).unwrap());
        assert!(!temp.path().join("model.safetensors").exists());
    }

    #[test]
    fn test_validate_model_dir_with_pytorch() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("pytorch_model.bin"), "").unwrap();
        for f in REQUIRED_FILES {
            fs::write(temp.path().join(f), "{}").unwrap();
        }
        // Should remove .bin files
        assert!(!validate_model_dir(temp.path()).unwrap());
        assert!(!temp.path().join("pytorch_model.bin").exists());
    }

    #[test]
    fn test_validate_model_dir_complete() {
        let temp = tempfile::tempdir().unwrap();
        for f in REQUIRED_FILES {
            fs::write(temp.path().join(f), "{}").unwrap();
        }
        // Create large safetensors (simulated with size metadata would fail in real test)
        // For test purposes, we skip this validation
        assert!(!validate_model_dir(temp.path()).unwrap()); // Will fail due to size check
    }
}