use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

pub const MODEL_DOWNLOAD_BYTES: u64 = 173_369_156;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceModelStatus {
    NotInstalled,
    Installing,
    Ready,
}

#[derive(Clone, Debug)]
pub struct VoiceModelProgress {
    pub file: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Clone)]
pub struct VoiceModelManager {
    root: PathBuf,
}

#[derive(Clone, Copy)]
struct Artifact {
    name: &'static str,
    url: &'static str,
    bytes: u64,
    sha256: &'static str,
}

const MOONSHINE_BASE: &str = "https://download.moonshine.ai/model/small-streaming-en/quantized";
static INSTALL_LOCK: Mutex<()> = Mutex::new(());

const ARTIFACTS: &[Artifact] = &[
    Artifact {
        name: "moonshine/adapter.ort",
        url: concat!(
            "https://download.moonshine.ai/model/small-streaming-en/quantized",
            "/adapter.ort"
        ),
        bytes: 2_867_424,
        sha256: "d8493e0ac76a198b309a8be6f74b3101e235f773ffe5d6b378278cd7e4177992",
    },
    Artifact {
        name: "moonshine/cross_kv.ort",
        url: concat!(
            "https://download.moonshine.ai/model/small-streaming-en/quantized",
            "/cross_kv.ort"
        ),
        bytes: 5_298_736,
        sha256: "6e57d1361717e00d73336a0c3beafedae784b1e537905ad253dee33db4007466",
    },
    Artifact {
        name: "moonshine/decoder_kv.ort",
        url: concat!(
            "https://download.moonshine.ai/model/small-streaming-en/quantized",
            "/decoder_kv.ort"
        ),
        bytes: 81_435_904,
        sha256: "d5adfcfaa6e582144791f1568bd0f683852c7bfbb8c79acad97499da05e4ffcf",
    },
    Artifact {
        name: "moonshine/encoder.ort",
        url: concat!(
            "https://download.moonshine.ai/model/small-streaming-en/quantized",
            "/encoder.ort"
        ),
        bytes: 43_853_224,
        sha256: "3b21d02eff6aa5651524ada4271d37c1d7bba4eb3d256415074f2cfdbaeb526a",
    },
    Artifact {
        name: "moonshine/frontend.ort",
        url: concat!(
            "https://download.moonshine.ai/model/small-streaming-en/quantized",
            "/frontend.ort"
        ),
        bytes: 30_984_200,
        sha256: "e086451043c1c8652a9614e4a4a81d5807221b611584a3cf31f73779d5900003",
    },
    Artifact {
        name: "moonshine/streaming_config.json",
        url: concat!(
            "https://download.moonshine.ai/model/small-streaming-en/quantized",
            "/streaming_config.json"
        ),
        bytes: 512,
        sha256: "26f02b6afb22d60871a5efd85c3d38e569cc0ddb6c5eb6e93d3260152ae8a47a",
    },
    Artifact {
        name: "moonshine/tokenizer.bin",
        url: concat!(
            "https://download.moonshine.ai/model/small-streaming-en/quantized",
            "/tokenizer.bin"
        ),
        bytes: 249_974,
        sha256: "6884b35fd6377d4c4d32336a0bc152f36b64d1e45b6503683cdc238250a8472d",
    },
    Artifact {
        name: "smart-turn-v3.2-cpu.onnx",
        url:
            "https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/main/smart-turn-v3.2-cpu.onnx",
        bytes: 8_679_182,
        sha256: "2bb026316b14a660486a75b1733cd3fbab8c2fd0314dc9af7be49f8cca967e4f",
    },
];

impl VoiceModelManager {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default() -> Self {
        let root = ide_core::local_store::LocalStore::open_default()
            .map(|store| store.app_data_dir().join("voice").join("models-v1"))
            .unwrap_or_else(|_| ide_core::AppConfig::config_root().join("data/voice/models-v1"));
        Self::new(root)
    }

    pub fn moonshine_dir(&self) -> PathBuf {
        self.root.join("moonshine")
    }

    pub fn smart_turn_path(&self) -> PathBuf {
        self.root.join("smart-turn-v3.2-cpu.onnx")
    }

    pub fn status(&self) -> VoiceModelStatus {
        if self.is_installed() {
            VoiceModelStatus::Ready
        } else {
            VoiceModelStatus::NotInstalled
        }
    }

    pub fn is_installed(&self) -> bool {
        self.root.join(".complete").is_file()
            && ARTIFACTS.iter().all(|artifact| {
                self.root
                    .join(artifact.name)
                    .metadata()
                    .is_ok_and(|metadata| metadata.len() == artifact.bytes)
            })
    }

    pub fn install(
        &self,
        cancelled: &AtomicBool,
        mut progress: impl FnMut(VoiceModelProgress),
    ) -> Result<()> {
        let _install_guard = INSTALL_LOCK
            .lock()
            .map_err(|_| anyhow::anyhow!("voice model installer lock was poisoned"))?;
        anyhow::ensure!(
            !cancelled.load(Ordering::Relaxed),
            "voice model installation cancelled"
        );
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "could not create voice model directory {}",
                self.root.display()
            )
        })?;
        let client = reqwest::blocking::Client::builder()
            .user_agent("Choro Voice/1")
            .build()?;
        let mut completed = 0_u64;
        for artifact in ARTIFACTS {
            anyhow::ensure!(
                !cancelled.load(Ordering::Relaxed),
                "voice model installation cancelled"
            );
            let target = self.root.join(artifact.name);
            if verified_file(&target, artifact)? {
                completed += artifact.bytes;
                progress(VoiceModelProgress {
                    file: artifact.name.to_string(),
                    downloaded_bytes: completed,
                    total_bytes: MODEL_DOWNLOAD_BYTES,
                });
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let temporary = target.with_extension("download");
            let mut response = client
                .get(artifact.url)
                .send()
                .and_then(reqwest::blocking::Response::error_for_status)
                .with_context(|| format!("could not download {}", artifact.name))?;
            let mut output = fs::File::create(&temporary)?;
            let mut hash = Sha256::new();
            let mut file_bytes = 0_u64;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                if cancelled.load(Ordering::Relaxed) {
                    drop(output);
                    let _ = fs::remove_file(&temporary);
                    anyhow::bail!("voice model installation cancelled");
                }
                let count = response.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                output.write_all(&buffer[..count])?;
                hash.update(&buffer[..count]);
                file_bytes += count as u64;
                progress(VoiceModelProgress {
                    file: artifact.name.to_string(),
                    downloaded_bytes: completed + file_bytes,
                    total_bytes: MODEL_DOWNLOAD_BYTES,
                });
            }
            output.sync_all()?;
            anyhow::ensure!(
                file_bytes == artifact.bytes,
                "{} download was {} bytes, expected {}",
                artifact.name,
                file_bytes,
                artifact.bytes
            );
            let actual = format!("{:x}", hash.finalize());
            anyhow::ensure!(
                actual == artifact.sha256,
                "{} failed SHA-256 verification",
                artifact.name
            );
            fs::rename(&temporary, &target)?;
            completed += artifact.bytes;
        }
        fs::write(
            self.root.join(".complete"),
            format!("moonshine-small-streaming-en\nsmart-turn-v3.2\n{MOONSHINE_BASE}\n"),
        )?;
        Ok(())
    }

    pub fn remove(&self) -> Result<()> {
        if self.root.exists() {
            fs::remove_dir_all(&self.root)
                .with_context(|| format!("could not remove {}", self.root.display()))?;
        }
        Ok(())
    }
}

fn verified_file(path: &Path, artifact: &Artifact) -> Result<bool> {
    let Ok(metadata) = path.metadata() else {
        return Ok(false);
    };
    if metadata.len() != artifact.bytes {
        return Ok(false);
    }
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()) == artifact.sha256)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_total_is_exact() {
        assert_eq!(
            ARTIFACTS.iter().map(|artifact| artifact.bytes).sum::<u64>(),
            MODEL_DOWNLOAD_BYTES
        );
    }
}
