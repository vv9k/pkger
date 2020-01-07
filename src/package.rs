use super::*;

pub mod _rpm {
    use super::*;
    fn handle_dependencies(info: &Info, mut builder: rpm::RPMBuilder) -> rpm::RPMBuilder {
        trace!("handling dependencies");
        if let Some(dependencies) = &info.depends_rh {
            for d in dependencies {
                trace!("adding dependency {}", d);
                builder = builder.requires(rpm::Dependency::any(d));
            }
        }
        if let Some(conflicts) = &info.conflicts_rh {
            for c in conflicts {
                trace!("adding conflict {}", c);
                builder = builder.conflicts(rpm::Dependency::any(c));
            }
        }
        if let Some(obsoletes) = &info.obsoletes_rh {
            for o in obsoletes {
                trace!("adding obsolete {}", o);
                builder = builder.obsoletes(rpm::Dependency::any(o));
            }
        }
        if let Some(provides) = &info.provides_rh {
            for p in provides {
                trace!("adding provide {}", p);
                builder = builder.provides(rpm::Dependency::any(p));
            }
        }
        builder
    }
    fn add_files<P: AsRef<Path>>(
        info: &Info,
        files: &[PathBuf],
        mut builder: rpm::RPMBuilder,
        build_dir: P,
        dest_dir: P,
        parent: P,
    ) -> rpm::RPMBuilder {
        trace!("adding files to builder");
        for file in files {
            if let Ok(metadata) = fs::metadata(file.as_path()) {
                if !metadata.file_type().is_dir() {
                    let fpath = {
                        let f = file
                            .strip_prefix(build_dir.as_ref().to_str().unwrap())
                            .unwrap();
                        match f.strip_prefix(parent.as_ref()) {
                            Ok(_f) => _f,
                            Err(_e) => f,
                        }
                    };
                    let should_include = {
                        match &info.exclude {
                            Some(excl) => should_include(fpath, &excl),
                            None => true,
                        }
                    };
                    if should_include {
                        trace!("adding {}", fpath.display());
                        builder = builder
                            .with_file(
                                file.as_path().to_str().unwrap(),
                                rpm::RPMFileOptions::new(format!(
                                    "{}",
                                    dest_dir.as_ref().join(fpath).as_path().display()
                                )),
                            )
                            .unwrap();
                    } else {
                        trace!("skipping {}", fpath.display());
                    }
                }
            }
        }
        builder
    }
    fn write_rpm(
        info: &Info,
        out_dir: &str,
        os: &str,
        ver: &str,
        pkg: rpm::RPMPackage,
    ) -> Result<(), Error> {
        let mut out_path = PathBuf::from(&out_dir);
        out_path.push(os);
        out_path.push(ver);
        if !out_path.exists() {
            map_return!(
                fs::create_dir_all(&out_path),
                format!(
                    "failed to create output directory in {}",
                    &out_path.as_path().display()
                )
            );
        }
        out_path.push(format!(
            "{}-{}-{}.{}.rpm",
            &info.name, &info.version, &info.revision, &info.arch
        ));
        trace!("saving to {}", out_path.as_path().display());
        let mut f = map_return!(
            File::create(out_path.as_path()),
            format!(
                "failed to create a file in {}",
                out_path.as_path().display()
            )
        );
        match pkg.write(&mut f) {
            Ok(_) => Ok(()),
            Err(e) => Err(format_err!(
                "failed to create rpm for {} - {}",
                &info.name,
                e
            )),
        }
    }
    pub fn build_rpm<P: AsRef<Path>>(
        out_dir: &str,
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
        builder = handle_dependencies(&info, builder);
        let dest_dir = PathBuf::from(dest);
        let _path = files[0].clone();
        let path = _path.strip_prefix(build_dir.as_ref()).unwrap();
        let parent = find_penultimate_ancestor(path);
        builder = add_files(
            &info,
            &files,
            builder,
            build_dir.as_ref(),
            dest_dir.as_path(),
            parent.as_path(),
        );
        let pkg = builder.build()?;
        Ok(write_rpm(&info, &out_dir, &os, &ver, pkg)?)
    }
}

pub mod deb {
    use super::*;
    pub async fn prepare_archive(info: &Info, os: &str) -> Result<PathBuf, Error> {
        // generate and upload control file
        let control_file = generate_deb_control(&info);
        let mut tmp_file = PathBuf::from(TEMPORARY_BUILD_DIR);
        if !Path::new(TEMPORARY_BUILD_DIR).exists() {
            fs::create_dir_all(TEMPORARY_BUILD_DIR).unwrap();
        }
        let fname = format!("{}-{}-deb-{}", &info.name, &os, Local::now().timestamp());
        tmp_file.push(fname);
        trace!(
            "saving control file to {} temporarily",
            tmp_file.as_path().display()
        );
        let f = File::create(tmp_file.as_path())?;
        trace!("creating archive with control file");
        let mut ar = tar::Builder::new(f);
        let mut header = tar::Header::new_gnu();
        header.set_size(control_file.as_bytes().iter().count() as u64);
        header.set_cksum();
        ar.append_data(&mut header, "./control", control_file.as_bytes())
            .unwrap();
        ar.finish().unwrap();
        Ok(tmp_file)
    }
    pub fn prepare_helper_scripts(r: &Recipe, bld_dir: &str) -> Result<Vec<u8>, Error> {
        let script = format!(
            "#!/bin/bash\n\nfor file in {}/*; do mv $file {}$file; done\n",
            &r.finish.files, &bld_dir
        );
        let scripts_archive = format!(
            "{}/scripts_archive_{}.tar",
            TEMPORARY_BUILD_DIR,
            Local::now().timestamp()
        );
        let f = File::create(&scripts_archive)?;
        let mut ar = tar::Builder::new(f);
        let mut header = tar::Header::new_gnu();
        header.set_size(script.as_bytes().iter().count() as u64);
        header.set_cksum();
        ar.append_data(&mut header, "./move_files.sh", script.as_bytes())
            .unwrap();
        ar.finish().unwrap();
        let archive = fs::read(&scripts_archive)?;
        fs::remove_file(scripts_archive)?;

        Ok(archive)
    }
    // # TODO
    // Find a nicer way to generate this
    pub fn generate_deb_control(info: &Info) -> String {
        let arch = match &info.arch[..] {
            "x86_64" => "amd64",
            // #TODO
            _ => "all",
        };
        trace!("generating control file");
        let mut control = format!(
            "Package: {}
Version: {}-{}
Architecture: {}
",
            &info.name, &info.version, &info.revision, &arch
        );
        control.push_str("Section: ");
        match &info.section {
            Some(section) => control.push_str(section),
            None => control.push_str("custom"),
        }
        control.push_str("\nPriority: ");
        match &info.priority {
            Some(priority) => control.push_str(priority),
            None => control.push_str("optional"),
        }
        control.push('\n');

        if let Some(dependencies) = &info.depends {
            control.push_str("Depends: ");
            let mut deps = String::new();
            for d in dependencies {
                trace!("adding dependency {}", d);
                deps.push_str(&format!("{}, ", d));
            }
            control.push_str(deps.trim_end_matches(", "));
            control.push('\n');
        }
        if let Some(conflicts) = &info.conflicts {
            control.push_str("Conflicts: ");
            let mut confs = String::new();
            for c in conflicts {
                trace!("adding conflict {}", c);
                confs.push_str(&format!("{}, ", c));
            }
            control.push_str(confs.trim_end_matches(", "));
            control.push('\n');
        }
        if let Some(obsoletes) = &info.obsoletes {
            control.push_str("Breaks: ");
            let mut obs = String::new();
            for o in obsoletes {
                trace!("adding obsolete {}", o);
                obs.push_str(&format!("{}, ", o));
            }
            control.push_str(obs.trim_end_matches(", "));
            control.push('\n');
        }
        if let Some(provides) = &info.provides {
            control.push_str("Provides: ");
            let mut prvds = String::new();
            for p in provides {
                trace!("adding provide {}", p);
                prvds.push_str(&format!("{}, ", p));
            }
            control.push_str(prvds.trim_end_matches(", "));
            control.push('\n');
        }

        control.push_str("Maintainer: ");
        match &info.maintainer {
            Some(maintainer) => control.push_str(maintainer),
            None => control.push_str("null <null@email.com>"),
        }

        control.push_str(&format!("\nDescription: {}\n", &info.description));

        trace!("{}", &control);
        control
    }
}
