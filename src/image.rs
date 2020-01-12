use super::*;

pub type Images = HashMap<String, Image>;

#[derive(Debug)]
pub struct Image {
    pub name: String,
    pub path: PathBuf,
    pub has_dockerfile: bool,
}
impl Image {
    pub fn new(entry: DirEntry) -> Image {
        let path = entry.path();
        let has_dockerfile = Image::has_dockerfile(path.clone());
        Image {
            name: entry.file_name().into_string().unwrap_or_default(),
            path,
            has_dockerfile,
        }
    }
    pub fn has_dockerfile(mut p: PathBuf) -> bool {
        p.push("Dockerfile");
        p.as_path().exists()
    }
    pub fn should_be_rebuilt(&self, state: &State) -> Result<bool, Error> {
        trace!("checking if image {} should be rebuilt", &self.name);
        let _state = state.clone();
        let state = _state.lock().unwrap();
        if let Some(prvs_bld_time) = state.images.get(&self.name) {
            match fs::metadata(self.path.as_path()) {
                Ok(metadata) => match metadata.modified() {
                    Ok(mod_time) => {
                        if mod_time > prvs_bld_time.1 {
                            trace!("image directory was modified since last build so marking for rebuild");
                            return Ok(true);
                        } else {
                            return Ok(false);
                        }
                    }
                    Err(e) => error!(
                        "failed to retrive modification date of {} - {}",
                        self.path.as_path().display(),
                        e
                    ),
                },
                Err(e) => error!(
                    "failed to read metadata of {} - {}",
                    self.path.as_path().display(),
                    e
                ),
            }
        }
        Ok(true)
    }
}

#[derive(Deserialize, Debug, Serialize)]
pub struct ImageState {
    pub images: HashMap<String, (String, SystemTime)>,
    pub statef: String,
}
impl Default for ImageState {
    fn default() -> Self {
        ImageState {
            images: HashMap::new(),
            statef: DEFAULT_STATE_FILE.to_string(),
        }
    }
}
impl ImageState {
    pub fn load<P: AsRef<Path>>(statef: P) -> Result<Self, Error> {
        let path = format!("{}", statef.as_ref().display());
        if !statef.as_ref().exists() {
            trace!("no previous state file, creating new in {}", &path);
            if let Err(e) = File::create(statef.as_ref()) {
                return Err(format_err!(
                    "failed to create state file in {} - {}",
                    &path,
                    e
                ));
            }
            return Ok(ImageState {
                images: HashMap::new(),
                statef: path,
            });
        }
        trace!("loading image state file from {}", &path);
        let contents = fs::read(statef.as_ref())?;
        let mut s: ImageState = serde_json::from_slice(&contents)?;
        trace!("{:?}", s);
        s.statef = path;
        Ok(s)
    }
    pub fn update(&mut self, image: &str, current_tag: &str) {
        trace!("updating build time of {}", image);
        self.images.insert(
            image.to_string(),
            (current_tag.to_string(), SystemTime::now()),
        );
    }
    pub fn save(&self) -> Result<(), Error> {
        trace!("saving images state to {}", &self.statef);
        trace!("{:#?}", &self);
        if !Path::new(&self.statef).exists() {
            map_return!(
                fs::File::create(&self.statef),
                format!("failed to create state file in {}", &self.statef)
            );
        }
        match serde_json::to_vec(&self) {
            Ok(d) => map_return!(
                fs::write(&self.statef, d),
                format!("failed to save state file in {}", &self.statef)
            ),
            Err(e) => return Err(format_err!("failed to serialize image state - {}", e)),
        }
        Ok(())
    }
}

// enum holding version of os
#[derive(Clone)]
pub enum Os {
    Debian(String, String),
    Redhat(String, String),
}
impl Os {
    pub fn from(os: &str, version: Option<String>) -> Result<Os, Error> {
        trace!("os: {}, version {:?}", os, version);
        let version = version.unwrap_or_default();
        match os {
            "ubuntu" | "debian" => Ok(Os::Debian(os.to_string(), version)),
            "centos" | "redhat" | "fedora" => Ok(Os::Redhat(os.to_string(), version)),
            os => Err(format_err!("unknown os {}", os)),
        }
    }
    pub fn os_ver(self) -> (String, String) {
        match self {
            Os::Debian(os, v) => (os, v),
            Os::Redhat(os, v) => (os, v),
        }
    }
    pub fn package_manager(self) -> String {
        match self {
            Os::Debian(_, _) => "apt".to_string(),
            Os::Redhat(_, v) if v == "8" => "dnf".to_string(),
            Os::Redhat(_, _) => "yum".to_string(),
        }
    }
}
