use super::*;

pub mod _rpm {
    use super::*;
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
        if let Some(dependencies) = &info.depends {
            for d in dependencies {
                trace!("adding dependency {}", d);
                builder = builder.requires(rpm::Dependency::any(d));
            }
        }
        if let Some(conflicts) = &info.conflicts {
            for c in conflicts {
                trace!("adding conflict {}", c);
                builder = builder.conflicts(rpm::Dependency::any(c));
            }
        }
        if let Some(obsoletes) = &info.obsoletes {
            for o in obsoletes {
                trace!("adding obsolete {}", o);
                builder = builder.obsoletes(rpm::Dependency::any(o));
            }
        }
        if let Some(provides) = &info.provides {
            for p in provides {
                trace!("adding provide {}", p);
                builder = builder.provides(rpm::Dependency::any(p));
            }
        }
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
                                    dest_dir.join(fpath).as_path().display()
                                )),
                            )
                            .unwrap();
                    } else {
                        trace!("skipping {}", fpath.display());
                    }
                }
            }
        }
        let pkg = builder.build()?;
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
}

pub mod deb {}
