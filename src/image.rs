use crate::{map_return, Error};
use anyhow::Result;
use futures::StreamExt;
use log::{debug, error};
use moby::{tty::TtyChunk, ContainerOptions, Docker, LogsOptions, RmContainerOptions};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::AsRef;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Default)]
pub struct Images(HashMap<String, Image>);

impl Images {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let mut images = Images::default();
        let path = path.as_ref();

        if !path.is_dir() {
            return Ok(images);
        }

        for entry in fs::read_dir(path)? {
            match entry {
                Ok(entry) => {
                    let filename = entry.file_name().to_string_lossy().to_string();
                    match Image::new(entry.path()) {
                        Ok(image) => {
                            images.0.insert(filename, image);
                        }
                        Err(e) => error!("failed to read image from path - {}", e),
                    }
                }
                Err(e) => error!("invalid entry - {}", e),
            }
        }

        Ok(images)
    }

    pub fn images(&self) -> &HashMap<String, Image> {
        &self.0
    }
}

#[derive(Debug)]
pub struct Image {
    pub name: String,
    pub path: PathBuf,
}

impl Image {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Image, Error> {
        let path = path.as_ref().to_path_buf();
        if !path.join("Dockerfile").exists() {
            return Err(anyhow!("Dockerfile missing from image"));
        }
        Ok(Image {
            // we can unwrap here because we know Dockerfile exists
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            path,
        })
    }
    pub fn should_be_rebuilt(&self, state: &ImagesState) -> Result<bool, Error> {
        if let Some(state) = state.images.get(&self.name) {
            let metadata = fs::metadata(self.path.as_path())?;
            let mod_time = metadata.modified()?;
            if mod_time > state.timestamp {
                return Ok(true);
            } else {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[derive(Deserialize, Clone, Debug, Serialize)]
pub struct ImageState {
    pub id: String,
    pub image: String,
    pub tag: String,
    pub os: Os,
    pub timestamp: SystemTime,
}

impl ImageState {
    pub async fn new(
        id: &str,
        image: &str,
        tag: &str,
        timestamp: &SystemTime,
        docker: &Docker,
    ) -> Result<ImageState> {
        let name = format!(
            "pkger-{}-{}",
            image,
            timestamp.duration_since(UNIX_EPOCH)?.as_secs()
        );
        let handle = docker
            .containers()
            .create(
                &ContainerOptions::builder(image.as_ref())
                    .name(&name)
                    .cmd(vec!["cat", "/etc/issue", "/etc/os-release"])
                    .build(),
            )
            .await
            .map(|info| docker.containers().get(info.id))
            .map_err(|e| anyhow!("failed to create a container - {}", e))?;

        handle.start().await?;

        let mut logs_stream = handle.logs(&LogsOptions::builder().stdout(true).build());
        let mut out = String::new();
        while let Some(chunk) = logs_stream.next().await {
            match chunk? {
                TtyChunk::StdOut(_chunk) => out.push_str(&String::from_utf8_lossy(&_chunk)),
                _ => {}
            }
        }
        debug!("{:?}", out);
        let os_name = extract_key(&out, "ID");
        let version = extract_key(&out, "VERSION_ID");

        if let Err(e) = handle
            .remove(
                &RmContainerOptions::builder()
                    .force(true)
                    .volumes(true)
                    .build(),
            )
            .await
        {
            error!("failed to delete container - {}", e);
        }

        Ok(ImageState {
            id: id.to_string(),
            image: image.to_string(),
            os: Os::from(os_name, version)?,
            tag: tag.to_string(),
            timestamp: *timestamp,
        })
    }
}

fn extract_key(out: &str, key: &str) -> Option<String> {
    let key = format!("{}=", key);
    if let Some(line) = out.lines().find(|line| line.starts_with(&key)) {
        let line = line.strip_prefix(&key).unwrap();
        if line.starts_with('"') {
            return Some(line.trim_matches('"').to_string());
        }
        return Some(line.to_string());
    }
    None
}

#[derive(Deserialize, Debug, Serialize)]
pub struct ImagesState {
    /// Contains historical build data of images. Keys are image names and corresponding values are
    /// tag with a timestamp -> images[IMAGE] = (TAG, TIMESTAMP)
    pub images: HashMap<String, ImageState>,
    /// Path to a file containing image state
    pub state_file: PathBuf,
}

impl Default for ImagesState {
    fn default() -> Self {
        ImagesState {
            images: HashMap::new(),
            state_file: PathBuf::from(crate::DEFAULT_STATE_FILE),
        }
    }
}
impl ImagesState {
    pub fn try_from_path<P: AsRef<Path>>(state_file: P) -> Result<Self, Error> {
        if !state_file.as_ref().exists() {
            File::create(state_file.as_ref())?;

            return Ok(ImagesState {
                images: HashMap::new(),
                state_file: state_file.as_ref().to_path_buf(),
            });
        }
        let contents = fs::read(state_file.as_ref())?;
        Ok(serde_json::from_slice(&contents)?)
    }
    pub fn update(&mut self, image: &str, state: &ImageState) {
        self.images.insert(image.to_string(), state.clone());
    }
    pub fn save(&self) -> Result<(), Error> {
        if !Path::new(&self.state_file).exists() {
            map_return!(
                fs::File::create(&self.state_file),
                format!(
                    "failed to create state file in {}",
                    self.state_file.display()
                )
            );
        }
        match serde_json::to_vec(&self) {
            Ok(d) => map_return!(
                fs::write(&self.state_file, d),
                format!("failed to save state file in {}", self.state_file.display())
            ),
            Err(e) => return Err(format_err!("failed to serialize image state - {}", e)),
        }
        Ok(())
    }
}

// enum holding version of os
#[derive(Debug, Deserialize, Clone, Serialize)]
pub enum Os {
    Debian(String),
    Redhat(String),
    Arch(String),
    Unknown,
}

impl AsRef<str> for Os {
    fn as_ref(&self) -> &str {
        match self {
            Os::Debian(_) => "debian",
            Os::Arch(_) => "arch",
            Os::Redhat(_) => "redhat",
            Os::Unknown => "unknown",
        }
    }
}

impl Os {
    pub fn from(os: Option<String>, version: Option<String>) -> Result<Os, Error> {
        if let Some(os) = os {
            let version = version.unwrap_or_default();
            match &os[..] {
                "ubuntu" | "debian" => Ok(Os::Debian(version)),
                "centos" | "redhat" | "fedora" => Ok(Os::Redhat(version)),
                os => Err(format_err!("unknown os {}", os)),
            }
        } else {
            Ok(Os::Unknown)
        }
    }
    pub fn os_ver(&self) -> &str {
        match self {
            Os::Debian(v) => v.as_str(),
            Os::Redhat(v) => v.as_str(),
            Os::Arch(v) => v.as_str(),
            Os::Unknown => "",
        }
    }
    pub fn package_manager(&self) -> &str {
        match self {
            Os::Debian(_) => "apt-get",
            Os::Redhat(v) if v == "8" => "dnf",
            Os::Redhat(_) => "yum",
            Os::Arch(_) => "pacman",
            Os::Unknown => "",
        }
    }
}
