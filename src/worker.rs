use super::*;

pub struct Worker<'p> {
    cfg: &'p Config,
    docker: &'p Docker,
    image: &'p Image,
    recipe: &'p Recipe,
}
impl<'p> Worker<'p> {
    pub async fn spawn_working(
        cfg: &'p Config,
        docker: &'p Docker,
        image: &'p Image,
        recipe: &'p Recipe,
    ) -> Result<(), Error> {
        let worker = Worker::new(&cfg, &docker, &image, &recipe);
        Ok(worker.do_work().await?)
    }
    fn new(
        cfg: &'p Config,
        docker: &'p Docker,
        image: &'p Image,
        recipe: &'p Recipe,
    ) -> Worker<'p> {
        trace!("creating a new worker for {} on {}", &recipe.info.name, &image.name);
        Worker {
            cfg,
            docker,
            image,
            recipe,
        }
    }
    async fn do_work(&self) -> Result<(), Error> {
        let mut state = ImageState::load(DEFAULT_STATE_FILE).unwrap_or_default();
        match self
            .create_container(&self.image, &self.recipe, &mut state)
            .await
        {
            Ok(container) => {
                container.start().await?;
                let os = self.determine_os(&container).await?;
                let package_manager = os.clone().package_manager();
                let (os_name, ver) = os.clone().os_ver();
                let container_bld_dir = self
                    .extract_src_in_container(&container, &self.recipe.info)
                    .await?;
                self.install_deps(&container, &self.recipe.info, &package_manager, os.clone())
                    .await?;

                // Helper env vars for recipe build execs
                let _pkger_vars = vec![
                    format!("PKGER_OS={}", &os_name),
                    format!("PKGER_OS_VER={}", &ver),
                    format!("PKGER_BLD_DIR={}", &container_bld_dir),
                ];
                let pkger_vars = _pkger_vars
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<&str>>();

                self.execute_build_steps(
                    &container,
                    &self.recipe.build,
                    &self.recipe.install,
                    &container_bld_dir,
                    &pkger_vars,
                    &self.image.name,
                )
                .await?;
                match os {
                    Os::Debian(_, _) => {
                        self.handle_deb_build(&container, self.recipe, &os_name, &ver)
                            .await?;
                    }
                    Os::Redhat(_, _) => {
                        self.handle_rpm_build(&container, self.recipe, &os_name, &ver)
                            .await?;
                    }
                }
                Self::remove_container(container).await;
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }
    async fn build_image(&self, image: &Image, state: &mut ImageState) -> Result<String, Error> {
        trace!("building image {:#?}", image);
        let image_with_tag = format!("{}:{}", &image.name, Local::now().timestamp());
        let mut opts = ImageBuilderOpts::new();
        opts.name(&image_with_tag);

        let mut archive_path = PathBuf::from(&self.cfg.images_dir);
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
        state.update(&image.name, &image_with_tag);
        state.save()?;

        map_return!(
            fs::remove_file(archive_path.as_path()),
            format!(
                "failed to delete temporary archive from {}",
                archive_path.as_path().display()
            )
        );
        Ok(image_with_tag)
    }
    async fn image_exists(&self, image: &str) -> bool {
        trace!("checking if image {} exists", image);
        let images = self.docker.images();
        images.inspect(image).await.is_ok()
    }
    async fn create_container(
        &self,
        image: &Image,
        r: &Recipe,
        mut state: &mut ImageState,
    ) -> Result<Container<'_>, Error> {
        trace!("creating container from image {}", &image.name);
        let mut opts = ContainerBuilderOpts::new();
        let mut image_name = image.name.clone();
        if let Some(cache) = state.images.get(&image_name) {
            image_name = cache.0.clone();
        }
        if !self.image_exists(&image_name).await || image.should_be_rebuilt().unwrap_or(true) {
            image_name = self.build_image(&image, &mut state).await?;
        }
        let vars = util::parse_env_vars(&r.env);
        opts.image(&image_name)
            .env(&vars.iter().map(|s| s.as_str()).collect::<Vec<&str>>())
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
        env: &[&str],
    ) -> Result<CmdOut, Error> {
        info!("executing {:?} in {}", cmd, &container.id);
        let mut opts = ExecOpts::new();
        opts.cmd(&cmd)
            .tty(true)
            .working_dir(&build_dir)
            .env(&env)
            .attach_stderr(true)
            .attach_stdout(true);

        match container.exec(&opts).await {
            Ok(out) if out.info.exit_code != 0 => {
                error!("{}\n{:?}", &out.out, &out.info);
                Err(format_err!(
                    "failed to exec step {:?} in container {}",
                    cmd,
                    &container.id,
                ))
            }
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
        if out.info.exit_code == 0 {
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
                Ok(Os::from(&os_name, version)?)
            } else {
                Err(format_err!(
                    "failed to determine containers {} os",
                    &container.id
                ))
            }
        } else {
            Err(format_err!(
                "no /etc/os-release found, can't determine container's os"
            ))
        }
    }
    async fn handle_deb_build(
        &self,
        container: &'_ Container<'_>,
        r: &Recipe,
        os: &str,
        ver: &str,
    ) -> Result<(), Error> {
        trace!(
            "creating deb package for:\nPackage: {}\nOs: {}\nVer: {}",
            &r.info.name,
            &os,
            &ver
        );
        let tmp_file = package::deb::prepare_archive(&r.info, &os).await?;
        let archive = fs::read(tmp_file.as_path())?;

        // create build dir in container
        let bld_dir = format!(
            "/tmp/pkger/{}_{}-{}",
            &r.info.name, &r.info.version, &r.info.revision
        );
        self.exec_step(
            &["mkdir", "-p", &format!("{}/DEBIAN", &bld_dir)],
            &container,
            "/",
            &[],
        )
        .await?;

        trace!("uploading control file to container:{}/DEBIAN", &bld_dir);
        let mut upload = UploadArchiveOpts::new();
        upload.path(&format!("{}/DEBIAN", &bld_dir));
        container.upload_archive(&archive, &upload).await?;

        // create all necessary directories to move files to
        let final_destination = format!("{}{}", &bld_dir, &r.finish.install_dir);
        self.exec_step(&["mkdir", "-p", &final_destination], &container, "/", &[])
            .await?;

        trace!(
            "moving final files from {} to build directory {}",
            &r.finish.files,
            &bld_dir
        );
        self.exec_step(
            &[
                "sh",
                "-c",
                &format!("cp -r {} {}", &r.finish.files, &bld_dir),
            ],
            &container,
            "/",
            &[],
        )
        .await?;

        trace!("building .deb with dpkg-deb");
        self.exec_step(&["dpkg-deb", "-b", &bld_dir], &container, "/", &[])
            .await?;
        let file_name = format!(
            "{}_{}-{}.deb",
            &r.info.name, &r.info.version, &r.info.revision
        );

        // deb archived in tar
        let deb_archive = container
            .archive_path(format!("/tmp/pkger/{}", &file_name))
            .await?;
        let mut out_path = PathBuf::from(&self.cfg.output_dir);
        out_path.push(&os);
        out_path.push(&ver);
        if !out_path.exists() {
            fs::create_dir_all(&out_path)?;
        }
        trace!("downloading .deb file to {}", out_path.as_path().display());
        // need to unpack the .deb
        let mut ar = Archive::new(Cursor::new(&deb_archive));
        map_return!(
            ar.unpack(out_path.as_path()),
            format!(
                "failed to unpack archive with .deb file in {}",
                out_path.as_path().display()
            )
        );

        trace!("cleaning up {}", tmp_file.as_path().display());
        fs::remove_file(tmp_file).unwrap();
        Ok(())
    }

    async fn handle_rpm_build(
        &self,
        container: &'_ Container<'_>,
        r: &Recipe,
        os: &str,
        ver: &str,
    ) -> Result<(), Error> {
        let archive = self.download_archive(&container, &r, &os, &ver).await?;
        let build_dir = self.prepare_build_dir(&r.info)?;
        let files = self.unpack_archive(archive.clone(), build_dir.clone())?;
        package::_rpm::build_rpm(
            &self.cfg.output_dir,
            &files,
            &r.info,
            &r.finish.install_dir,
            build_dir.as_path(),
            &os,
            &ver,
        )?;
        trace!("cleaning up build dir {}", build_dir.as_path().display());
        fs::remove_dir_all(build_dir).unwrap();
        trace!(
            "cleaning up temporary archive {}",
            archive.as_path().display()
        );
        fs::remove_file(archive).unwrap();
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
        os: Os,
    ) -> Result<(), Error> {
        let dependencies = match os {
            Os::Debian(_, _) => {
                if let Some(dependencies) = &info.depends {
                    dependencies
                        .iter()
                        .map(|s| s.as_ref())
                        .collect::<Vec<&str>>()
                } else {
                    Vec::new()
                }
            }
            Os::Redhat(_, _) => {
                if let Some(dependencies) = &info.depends_rh {
                    dependencies
                        .iter()
                        .map(|s| s.as_ref())
                        .collect::<Vec<&str>>()
                } else {
                    Vec::new()
                }
            }
        };
        trace!("installing dependencies - {:?}", dependencies);
        trace!("using {} as package manager", package_manager);
        match self
            .exec_step(&[&package_manager, "-y", "update"], &container, "/", &[])
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
            &dependencies[..],
        ]
        .concat();
        match self.exec_step(&install_cmd, &container, "/", &[]).await {
            Ok(out) => info!("{}", out.out),
            Err(e) => {
                return Err(format_err!(
                    "failed to install dependencies in container {} - {}",
                    &container.id,
                    e
                ))
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
            .exec_step(&["mkdir", &build_dir], &container, "/", &[])
            .await
        {
            return Err(format_err!(
                "failed while creating directory in {}:{}- {}",
                &container.id,
                &build_dir,
                e
            ));
        }

        let archive = self.get_src(&info).await?;

        trace!("extracting source in {}:{}", &container.id, &build_dir);
        let mut opts = UploadArchiveOpts::new();
        opts.path(&build_dir);
        container.upload_archive(&archive, &opts).await?;

        Ok(build_dir)
    }

    async fn get_src(&self, info: &Info) -> Result<Vec<u8>, Error> {
        // first we check if git is present in the recipe
        if let Some(repo) = &info.git {
            let archive_path = Self::fetch_git_src(&repo, &info.name)?;
            Ok(fs::read(archive_path.as_path())?)
        } else {
            // Then we check if it's a url
            if info.source.starts_with("http://") || info.source.starts_with("https://") {
                trace!("treating source as URL");
                let url: Uri = info.source.parse()?;
                let scheme = url.scheme_str().unwrap_or("");
                let builder = hyper::client::Client::builder();
                let mut archive = bytes::Bytes::new();
                trace!("downloading {}", &info.source);
                match scheme {
                    "http" => {
                        let client = builder.build::<_, Body>(hyper::client::HttpConnector::new());
                        let mut res = client.get(info.source.parse()?).await?;
                        if res.status().is_redirection() {
                            if let Some(new_location) = res.headers().get("location") {
                                res = client
                                    .get(str::from_utf8(new_location.as_ref())?.parse()?)
                                    .await?;
                                archive = hyper::body::to_bytes(res).await?;
                            }
                        } else {
                            archive = hyper::body::to_bytes(res).await?;
                        }
                    }
                    "https" => {
                        let client = builder.build::<_, Body>(hyper_tls::HttpsConnector::new());
                        let mut res = client.get(info.source.parse()?).await?;
                        if res.status().is_redirection() {
                            if let Some(new_location) = res.headers().get("location") {
                                res = client
                                    .get(str::from_utf8(new_location.as_ref())?.parse()?)
                                    .await?;
                                archive = hyper::body::to_bytes(res).await?;
                            }
                        } else {
                            archive = hyper::body::to_bytes(res).await?;
                        }
                    }
                    _ => return Err(format_err!("unknown url scheme {}", scheme)),
                }
                Ok(archive[..].to_vec())
            } else {
                // if it's not a url then its a file
                let src_path = format!("{}/{}/{}", &self.cfg.recipes_dir, &info.name, &info.source);
                match fs::read(&src_path) {
                    Ok(archive) => Ok(archive),
                    Err(e) => Err(format_err!("no archive in {} - {}", src_path, e)),
                }
            }
        }
    }

    fn fetch_git_src(repo: &str, package: &str) -> Result<PathBuf, Error> {
        trace!("fetching source for package {} from {}", package, repo);
        let src_dir = PathBuf::from(&format!("/tmp/{}-src", &package));
        if src_dir.exists() {
            fs::remove_dir_all(src_dir.as_path())?;
        }
        fs::create_dir_all(src_dir.as_path())?;
        let _ = git2::Repository::clone(&repo, src_dir.as_path())?;

        let archive_path = PathBuf::from(&format!(
            "/tmp/{}-{}.tar",
            package,
            Local::now().timestamp()
        ));
        let f = File::create(&archive_path)?;
        trace!(
            "creating archive with source in {}",
            archive_path.as_path().display()
        );
        let mut ar = tar::Builder::new(f);
        ar.append_dir_all(".", src_dir.as_path())?;
        ar.finish().unwrap();
        Ok(archive_path)
    }

    async fn execute_build_steps(
        &self,
        container: &'_ Container<'_>,
        build: &Build,
        install: &Install,
        build_dir: &str,
        pkgr_vars: &[&str],
        current_image: &str,
    ) -> Result<(), Error> {
        for step in build.steps.iter().chain(install.steps.iter()) {
            match Cmd::new(&step) {
                Ok(cmd) => {
                    trace!("{:?}", cmd);
                    match cmd.images {
                        Some(images) if images.contains(&current_image) => {
                            let exec = self
                                .exec_step(
                                    &["sh", "-c", &cmd.cmd],
                                    container,
                                    &build_dir,
                                    &pkgr_vars,
                                )
                                .await?;
                            trace!("{:?}", exec.info);
                            info!("{}", exec.out);
                        }
                        None => {
                            let exec = self
                                .exec_step(&["sh", "-c", &step], container, &build_dir, &pkgr_vars)
                                .await?;
                            trace!("{:?}", exec.info);
                            info!("{}", exec.out);
                        }
                        _ => {}
                    }
                }
                Err(e) => return Err(format_err!("failed while executing build step - {}", e)),
            }
        }
        Ok(())
    }

    async fn download_archive(
        &self,
        container: &'_ Container<'_>,
        r: &Recipe,
        os: &str,
        ver: &str,
    ) -> Result<PathBuf, Error> {
        trace!(
            "downloading archive from {} {}",
            &container.id,
            &r.finish.files
        );
        let archive = container.archive_path(&r.finish.files).await?;
        let mut out_path = PathBuf::from(&self.cfg.output_dir);
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
            &r.info.name, &r.info.version, &r.info.revision
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
}

