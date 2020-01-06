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
    pub fn should_be_rebuilt(&self) -> Result<bool, Error> {
        trace!("checking if image {} should be rebuilt", &self.name);
        let state = ImageState::load(DEFAULT_STATE_FILE)?;
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

#[derive(Deserialize, Debug, Default, Serialize)]
pub struct ImageState {
    pub images: HashMap<String, (String, SystemTime)>,
    #[serde(skip)]
    pub statef: String,
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
        fs::write(&self.statef, serde_json::to_vec(&self)?).unwrap();
        Ok(())
    }
}
