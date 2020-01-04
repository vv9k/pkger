#[macro_use]
extern crate failure;
extern crate tar;
use chrono::prelude::Local;
use failure::Error;
use log::*;
use rpm;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, DirBuilder, DirEntry, File};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tar::Archive;
use wharf::api::Container;
use wharf::opts::{
    ContainerBuilderOpts, ExecOpts, ImageBuilderOpts, RmContainerOpts, UploadArchiveOpts,
};
use wharf::result::CmdOut;
use wharf::Docker;

const DEFAULT_STATE_FILE: &str = ".pkger.state";
const TEMPORARY_BUILD_DIR: &str = "/tmp";

macro_rules! map_return {
    ($f:expr, $e:expr) => {
        match $f {
            Ok(d) => d,
            Err(e) => return Err(format_err!("{} - {}", $e, e)),
        }
    };
}

// enum holding version of os
#[derive(Clone)]
enum Os {
    Debian(String, String),
    Redhat(String, String),
}
impl Os {
    fn from(s: &str, version: Option<String>) -> Result<Os, Error> {
        trace!("os: {}, version {:?}", s, version);
        let version = version.unwrap_or_default();
        match s {
            "ubuntu" | "debian" => Ok(Os::Debian(s.to_string(), version)),
            "centos" | "redhat" | "fedora" => Ok(Os::Redhat(s.to_string(), version)),
            os => Err(format_err!("unknown os {}", os)),
        }
    }
    fn os_ver(self) -> (String, String) {
        match self {
            Os::Debian(os, v) => (os, v),
            Os::Redhat(os, v) => (os, v),
        }
    }
    fn package_manager(self) -> String {
        match self {
            Os::Debian(_, _) => "apt".to_string(),
            Os::Redhat(_, v) if v == "8" => "dnf".to_string(),
            Os::Redhat(_, _) => "yum".to_string(),
        }
    }
}

#[derive(Deserialize, Debug)]
struct Info {
    name: String,
    version: String,
    arch: String,
    revision: String,
    description: String,
    license: String,
    source: String,
    images: Vec<String>,
    vendor: Option<String>,
    depends: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}
#[derive(Deserialize, Debug)]
struct Build {
    steps: Vec<String>,
}
#[derive(Deserialize, Debug)]
struct Install {
    steps: Vec<String>,
    destdir: String,
}
#[derive(Deserialize, Debug)]
struct Recipe {
    info: Info,
    build: Build,
    install: Install,
}
impl Recipe {
    fn new(entry: DirEntry) -> Result<Recipe, Error> {
        let mut path = entry.path();
        path.push("recipe.toml");
        Ok(toml::from_str::<Recipe>(&fs::read_to_string(&path)?)?)
    }
}
type Recipes = HashMap<String, Recipe>;

#[derive(Deserialize, Debug)]
pub struct Config {
    images_dir: String,
    recipes_dir: String,
    output_dir: String,
}

#[derive(Debug)]
struct Image {
    name: String,
    path: PathBuf,
    has_dockerfile: bool,
}
impl Image {
    fn new(entry: DirEntry) -> Image {
        let path = entry.path();
        let has_dockerfile = Image::has_dockerfile(path.clone());
        Image {
            name: entry.file_name().into_string().unwrap_or_default(),
            path,
            has_dockerfile,
        }
    }
    fn has_dockerfile(mut p: PathBuf) -> bool {
        p.push("Dockerfile");
        p.as_path().exists()
    }
    fn should_be_rebuilt(&self) -> Result<bool, Error> {
        trace!("checking if image {} should be rebuilt", &self.name);
        let state = ImageState::load(DEFAULT_STATE_FILE)?;
        if let Some(prvs_bld_time) = state.images.get(&self.name) {
            match fs::metadata(self.path.as_path()) {
                Ok(metadata) => match metadata.modified() {
                    Ok(mod_time) => {
                        if mod_time > *prvs_bld_time {
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
type Images = HashMap<String, Image>;

#[derive(Deserialize, Debug, Default, Serialize)]
struct ImageState {
    images: HashMap<String, SystemTime>,
    #[serde(skip)]
    statef: String,
}
impl ImageState {
    fn load<P: AsRef<Path>>(statef: P) -> Result<Self, Error> {
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
        let mut s: ImageState = toml::from_slice(&contents)?;
        trace!("{:?}", s);
        s.statef = path;
        Ok(s)
    }
    fn update(&mut self, image: &str) {
        trace!("updating build time of {}", image);
        self.images.insert(image.to_string(), SystemTime::now());
    }
    fn save(&self) -> Result<(), Error> {
        trace!("saving images state to {}", &self.statef);
        fs::write(&self.statef, toml::to_vec(&self)?).unwrap();
        Ok(())
    }
}

#[derive(Debug)]
pub struct Pkger {
    docker: Docker,
    pub config: Config,
    images: Images,
    recipes: Recipes,
}
impl Pkger {
    pub fn new(docker_addr: &str, conf_file: &str) -> Result<Self, Error> {
        let content = map_return!(
            fs::read(&conf_file),
            format!("failed to read config file from {}", conf_file)
        );
        let config: Config = map_return!(toml::from_slice(&content), "failed to parse config file");
        trace!("{:?}", config);
        let images = Pkger::parse_images_dir(&config.images_dir)?;
        let recipes = Pkger::parse_recipes_dir(&config.recipes_dir)?;
        Ok(Pkger {
            docker: Docker::new(docker_addr)?,
            config,
            images,
            recipes,
        })
    }

    fn parse_images_dir(p: &str) -> Result<Images, Error> {
        trace!("parsing images dir - {}", p);
        let mut images = HashMap::new();
        for _entry in map_return!(fs::read_dir(p), format!("failed to read images_dir {}", p)) {
            if let Ok(entry) = _entry {
                if let Ok(ftype) = entry.file_type() {
                    if ftype.is_dir() {
                        let image = Image::new(entry);
                        trace!("{:?}", image);
                        if image.has_dockerfile {
                            images.insert(image.name.clone(), image);
                        } else {
                            error!(
                                "image {} doesn't have Dockerfile in it's root directory",
                                image.name
                            );
                        }
                    }
                }
            }
        }
        trace!("{:?}", images);
        Ok(images)
    }

    fn parse_recipes_dir(p: &str) -> Result<Recipes, Error> {
        trace!("parsing recipes dir - {}", p);
        let mut recipes = HashMap::new();
        for _entry in map_return!(fs::read_dir(p), "failed to read recipes_dir") {
            if let Ok(entry) = _entry {
                if let Ok(ftype) = entry.file_type() {
                    if ftype.is_dir() {
                        let path = entry.path();
                        match Recipe::new(entry) {
                            Ok(recipe) => {
                                trace!("{:?}", recipe);
                                recipes.insert(recipe.info.name.clone(), recipe);
                            }
                            Err(e) => error!(
                                "directory {} doesn't have a recipe.toml or the recipe is wrong - {}",
                                path.as_path().display(),
                                e
                            ),
                        }
                    }
                }
            }
        }
        trace!("{:?}", recipes);
        Ok(recipes)
    }

    async fn build_image(&self, image: &Image, state: &mut ImageState) -> Result<(), Error> {
        trace!("building image {:#?}", image);
        let mut opts = ImageBuilderOpts::new();
        opts.name(&image.name);

        let mut archive_path = PathBuf::from(&self.config.images_dir);
        archive_path.push(format!("{}.tar", &image.name));
        trace!("creating archive in {}", archive_path.as_path().display());
        let file = map_return!(
            File::create(archive_path.as_path()),
            format!(
                "failed to create temporary archive for image {} in {}",
                &image.name,
                archive_path.as_path().display()
            )
        );
        let mut archive = tar::Builder::new(file);
        archive.append_dir_all(".", image.path.as_path()).unwrap();
        archive.finish().unwrap();

        let archive_content = map_return!(
            fs::read(archive_path.as_path()),
            format!(
                "failed to read archived image {} from {}",
                &image.name,
                archive_path.as_path().display()
            )
        );
        let images = self.docker.images();
        map_return!(
            images.build(&archive_content, &opts).await,
            format!("failed to build image {}", &image.name)
        );
        state.update(&image.name);
        state.save()?;

        Ok(map_return!(
            fs::remove_file(archive_path.as_path()),
            format!(
                "failed to delete temporary archive from {}",
                archive_path.as_path().display()
            )
        ))
    }

    async fn image_exists(&self, image: &str) -> bool {
        trace!("checking if image {} exists", image);
        let images = self.docker.images();
        images.inspect(image).await.is_ok()
    }
    async fn create_container(
        &self,
        image: &Image,
        mut state: &mut ImageState,
    ) -> Result<Container<'_>, Error> {
        trace!("creating container from image {}", &image.name);
        let mut opts = ContainerBuilderOpts::new();
        if !self.image_exists(&image.name).await || image.should_be_rebuilt().unwrap_or(true) {
            self.build_image(&image, &mut state).await?;
        }
        opts.image(&image.name)
            .shell(&["/bin/bash"])
            .cmd(&["/bin/bash"])
            .tty(true)
            .attach_stdout(true)
            .open_stdin(true)
            .attach_stderr(true)
            .attach_stdin(true);
        let name = format!("pkger-{}-{}", &image.name, Local::now().timestamp());
        match self.docker.containers().create(&name, &opts).await {
            Ok(_) => Ok(self.docker.container(&name)),
            Err(e) => Err(format_err!(
                "failed to create container {} with image {} - {}",
                name,
                &image.name,
                e
            )),
        }
    }
    async fn exec_step(
        &self,
        cmd: &[&str],
        container: &'_ Container<'_>,
        build_dir: &str,
    ) -> Result<CmdOut, Error> {
        info!("executing {:?} in {}", cmd, &container.id);
        let mut opts = ExecOpts::new();
        opts.cmd(&cmd)
            .tty(true)
            .working_dir(&build_dir)
            .attach_stderr(true)
            .attach_stdout(true);

        match container.exec(&opts).await {
            Ok(out) if out.info.exit_code != 0 => Err(format_err!(
                "failed to exec step {:?} in container {} - {:?}",
                cmd,
                &container.id,
                out
            )),
            Ok(out) => Ok(out),
            Err(e) => Err(format_err!(
                "failed to exec step {:?} in container {} - {:?}",
                cmd,
                &container.id,
                e
            )),
        }
    }

    async fn determine_os(&self, container: &'_ Container<'_>) -> Result<Os, Error> {
        trace!("determining container {} os", &container.id);
        let mut os_release = ExecOpts::new();
        os_release
            .cmd(&["cat", "/etc/os-release"])
            .tty(true)
            .attach_stdout(true)
            .attach_stderr(true);
        let out = container.exec(&os_release).await?;
        trace!("{}", &out.out);
        let mut id = None;
        let mut version = None;
        for line in out.out.split('\n').into_iter() {
            if line.starts_with("ID=") {
                id = Some(
                    line[3..]
                        .trim_end_matches('\r')
                        .trim_matches('"')
                        .to_string(),
                );
            } else if line.starts_with("VERSION_ID=") {
                version = Some(
                    line[11..]
                        .trim_end_matches('\r')
                        .trim_matches('"')
                        .to_string(),
                );
            }
        }
        if let Some(os_name) = id {
            return Ok(Os::from(&os_name, version)?);
        }
        Err(format_err!(
            "failed to determine containers {} os",
            &container.id
        ))
    }

    pub async fn build_recipe<S: AsRef<str>>(&self, recipe: S) -> Result<(), Error> {
        trace!("building recipe {}", recipe.as_ref());
        match self.recipes.get(recipe.as_ref()) {
            Some(r) => {
                let mut state = ImageState::load(DEFAULT_STATE_FILE).unwrap_or_default();
                for image_name in r.info.images.iter() {
                    let image = match self.images.get(image_name) {
                        Some(i) => i,
                        None => {
                            error!(
                                "image {} not found in {}",
                                image_name, &self.config.images_dir
                            );
                            continue;
                        }
                    };
                    trace!("using image - {}", image_name);
                    match self.create_container(&image, &mut state).await {
                        Ok(container) => {
                            container.start().await?;
                            let os = self.determine_os(&container).await?;
                            let package_manager = os.clone().package_manager();
                            let (os, ver) = os.os_ver();
                            let build_dir =
                                self.extract_src_in_container(&container, &r.info).await?;
                            self.install_deps(&container, &r.info, &package_manager)
                                .await?;
                            self.execute_build_steps(&container, &r.build, &r.install, &build_dir)
                                .await?;
                            let archive = self
                                .download_archive(&container, &r.info, &r.install, &os, &ver)
                                .await?;
                            let build_dir = self.prepare_build_dir(&r.info)?;
                            let files = self.unpack_archive(archive, build_dir.clone())?;
                            self.build_rpm(
                                &files,
                                &r.info,
                                &r.install.destdir,
                                build_dir.as_path(),
                                &os,
                                &ver,
                            )?;
                            Pkger::remove_container(container).await;
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            None => error!(
                "no recipe named {} found in recipes directory {}",
                recipe.as_ref(),
                self.config.recipes_dir
            ),
        }

        Ok(())
    }

    async fn remove_container(container: Container<'_>) {
        trace!("removing container {}", &container.id);
        let mut opts = RmContainerOpts::new();
        opts.force(true).volumes(true);
        if let Err(e) = container.remove(&opts).await {
            error!("failed to remove container {} - {}", &container.id, e);
        }
    }

    async fn install_deps(
        &self,
        container: &'_ Container<'_>,
        info: &Info,
        package_manager: &str,
    ) -> Result<(), Error> {
        if let Some(dependencies) = &info.depends {
            trace!("installing dependencies - {:?}", dependencies);
            trace!("using {} as package manager", package_manager);
            match self
                .exec_step(&[&package_manager, "-y", "update"], &container, "/")
                .await
            {
                Ok(out) => info!("{}", out.out),
                Err(e) => {
                    return Err(format_err!(
                        "failed to update container {} - {}",
                        &container.id,
                        e
                    ))
                }
            }

            let install_cmd = [
                &vec![package_manager, "-y", "install"][..],
                &dependencies
                    .iter()
                    .map(|s| s.as_ref())
                    .collect::<Vec<&str>>()[..],
            ]
            .concat();
            match self.exec_step(&install_cmd, &container, "/").await {
                Ok(out) => info!("{}", out.out),
                Err(e) => {
                    return Err(format_err!(
                        "failed to install dependencies in container {} - {}",
                        &container.id,
                        e
                    ))
                }
            }
        }
        Ok(())
    }

    // Returns a path to build directory
    async fn extract_src_in_container(
        &self,
        container: &'_ Container<'_>,
        info: &Info,
    ) -> Result<String, Error> {
        let build_dir = format!("/tmp/{}-{}/", info.name, Local::now().timestamp());
        if let Err(e) = self
            .exec_step(&["mkdir", &build_dir], &container, "/")
            .await
        {
            return Err(format_err!(
                "failed while creating directory in {}:{}- {}",
                &container.id,
                &build_dir,
                e
            ));
        }

        let mut opts = UploadArchiveOpts::new();
        opts.path(&build_dir);

        let src_path = format!(
            "{}/{}/{}",
            &self.config.recipes_dir, &info.name, &info.source
        );
        match fs::read(&src_path) {
            Ok(archive) => {
                container.upload_archive(&archive, &opts).await?;
                Ok(build_dir)
            }
            Err(e) => Err(format_err!("no archive in {} - {}", src_path, e)),
        }
    }

    async fn execute_build_steps(
        &self,
        container: &'_ Container<'_>,
        build: &Build,
        install: &Install,
        build_dir: &str,
    ) -> Result<(), Error> {
        for step in build.steps.iter().chain(install.steps.iter()) {
            info!(
                "{}",
                self.exec_step(
                    &step.split_ascii_whitespace().collect::<Vec<&str>>(),
                    container,
                    &build_dir,
                )
                .await?
                .out
            );
        }
        Ok(())
    }

    async fn download_archive(
        &self,
        container: &'_ Container<'_>,
        info: &Info,
        install: &Install,
        os: &str,
        ver: &str,
    ) -> Result<PathBuf, Error> {
        trace!(
            "downloading archive from {} {}",
            &container.id,
            &install.destdir
        );
        let archive = container.archive_path(&install.destdir).await?;
        let mut out_path = PathBuf::from(&self.config.output_dir);
        out_path.push(os);
        out_path.push(ver);
        if !out_path.as_path().exists() {
            trace!("creating directory {}", out_path.as_path().display());
            let mut builder = DirBuilder::new();
            builder.recursive(true);
            builder.create(out_path.as_path())?;
        }
        out_path.push(format!(
            "{}-{}-{}.tar",
            &info.name, &info.version, &info.revision
        ));

        trace!("saving archive to {}", out_path.as_path().display());
        fs::write(out_path.as_path(), archive)?;
        Ok(out_path)
    }

    // returns a path to created directory
    fn prepare_build_dir(&self, info: &Info) -> Result<PathBuf, Error> {
        let mut build_dir = PathBuf::from(TEMPORARY_BUILD_DIR);
        build_dir.push(format!("{}-{}", &info.name, Local::now().timestamp()));
        trace!(
            "creating temporary build dir in {}",
            build_dir.as_path().display()
        );
        let mut builder = DirBuilder::new();
        map_return!(
            builder.recursive(true).create(build_dir.as_path()),
            format!(
                "failed to create a build directory in {}",
                build_dir.as_path().display()
            )
        );

        Ok(build_dir)
    }

    fn unpack_archive(&self, archive: PathBuf, build_dir: PathBuf) -> Result<Vec<PathBuf>, Error> {
        trace!("unpacking archive {}", archive.as_path().display());
        let mut paths = Vec::new();
        match File::open(archive.as_path()) {
            Ok(f) => {
                let mut ar = Archive::new(f);
                match ar.entries() {
                    Ok(entries) => {
                        for file in entries {
                            match file {
                                Ok(mut file) => {
                                    paths.push(build_dir.join(file.path().unwrap()));
                                    file.unpack_in(build_dir.as_path()).unwrap();
                                }
                                Err(e) => {
                                    return Err(format_err!(
                                        "failed to unpack file from {} - {}",
                                        archive.as_path().display(),
                                        e
                                    ))
                                }
                            }
                        }
                    }
                    Err(e) => {
                        return Err(format_err!(
                            "failed to read entries of {} - {}",
                            archive.as_path().display(),
                            e
                        ))
                    }
                }
            }
            Err(e) => {
                return Err(format_err!(
                    "failed to open archive {} - {}",
                    archive.as_path().display(),
                    e
                ))
            }
        }
        trace!("finished unpacking");
        Ok(paths)
    }

    fn build_rpm<P: AsRef<Path>>(
        &self,
        files: &[PathBuf],
        info: &Info,
        dest: &str,
        build_dir: P,
        os: &str,
        ver: &str,
    ) -> Result<(), Error> {
        trace!(
            "building rpm for:\npackage: {}\nos: {} {}\nver: {}-{}\narch: {}",
            &info.name,
            os,
            ver,
            &info.version,
            &info.revision,
            &info.arch,
        );
        let mut builder = rpm::RPMBuilder::new(
            &info.name,
            &info.version,
            &info.license,
            &info.arch,
            &info.description,
        )
        .compression(rpm::Compressor::from_str("gzip")?);
        let dest_dir = PathBuf::from(dest);
        let _path = files[0].clone();
        let path = _path.strip_prefix(build_dir.as_ref()).unwrap();
        let parent = find_penultimate_ancestor(path);
        trace!("adding files to builder");
        for file in files {
            if let Ok(metadata) = fs::metadata(file.as_path()) {
                if !metadata.file_type().is_dir() {
                    let fpath = {
                        let f = file
                            .strip_prefix(build_dir.as_ref().to_str().unwrap())
                            .unwrap();
                        match f.strip_prefix(parent.as_path()) {
                            Ok(_f) => _f,
                            Err(_e) => f,
                        }
                    };
                    trace!("adding {}", fpath.display());

                    builder = builder
                        .with_file(
                            file.as_path().to_str().unwrap(),
                            rpm::RPMFileOptions::new(format!(
                                "{}",
                                dest_dir.join(fpath).as_path().display()
                            )),
                        )
                        .unwrap();
                }
            }
        }
        let pkg = builder.build()?;
        let mut out_path = PathBuf::from(&self.config.output_dir);
        out_path.push(os);
        out_path.push(ver);
        out_path.push(format!(
            "{}-{}-{}.{}.rpm",
            &info.name, &info.version, &info.revision, &info.arch
        ));
        trace!("saving to {}", out_path.as_path().display());
        let mut f = File::create(out_path.as_path())?;
        match pkg.write(&mut f) {
            Ok(_) => Ok(()),
            Err(e) => Err(format_err!(
                "failed to create rpm for {} - {}",
                &info.name,
                e
            )),
        }
    }
}

fn find_penultimate_ancestor<P: AsRef<Path>>(path: P) -> PathBuf {
    trace!("finding parent of {}", path.as_ref().display());
    let mut ancestors = path.as_ref().ancestors();
    loop {
        match ancestors.next() {
            Some(ancestor) => {
                if ancestors.next() == Some(Path::new("")) {
                    trace!("found {}", ancestor.display());
                    return ancestor.to_path_buf();
                }
            }
            None => return PathBuf::from(""),
        }
    }
}
