use crate::recipe::MetadataRep;
use crate::Result;

use chrono::Local;
use std::fs;
use std::path::{Path, PathBuf};

const TEMPORARY_BUILD_DIR: &str = "/tmp";

pub fn prepare_archive(info: &MetadataRep, os: &str) -> Result<PathBuf> {
    // generate and upload control file
    let control_file = generate_deb_control(&info);
    let mut tmp_file = PathBuf::from(TEMPORARY_BUILD_DIR);
    if !Path::new(TEMPORARY_BUILD_DIR).exists() {
        fs::create_dir_all(TEMPORARY_BUILD_DIR).unwrap();
    }
    let fname = format!("{}-{}-deb-{}", &info.name, &os, Local::now().timestamp());
    tmp_file.push(fname);
    let f = fs::File::create(tmp_file.as_path())?;
    let mut ar = tar::Builder::new(f);
    let mut header = tar::Header::new_gnu();
    header.set_size(control_file.as_bytes().iter().count() as u64);
    header.set_cksum();
    ar.append_data(&mut header, "./control", control_file.as_bytes())
        .unwrap();
    ar.finish().unwrap();
    Ok(tmp_file)
}

// # TODO
// Find a nicer way to generate this
pub fn generate_deb_control(info: &MetadataRep) -> String {
    let arch = match &info.arch[..] {
        "x86_64" => "amd64",
        // #TODO
        _ => "all",
    };
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
            deps.push_str(&format!("{}, ", d));
        }
        control.push_str(deps.trim_end_matches(", "));
        control.push('\n');
    }
    if let Some(conflicts) = &info.conflicts {
        control.push_str("Conflicts: ");
        let mut confs = String::new();
        for c in conflicts {
            confs.push_str(&format!("{}, ", c));
        }
        control.push_str(confs.trim_end_matches(", "));
        control.push('\n');
    }
    if let Some(obsoletes) = &info.obsoletes {
        control.push_str("Breaks: ");
        let mut obs = String::new();
        for o in obsoletes {
            obs.push_str(&format!("{}, ", o));
        }
        control.push_str(obs.trim_end_matches(", "));
        control.push('\n');
    }
    if let Some(provides) = &info.provides {
        control.push_str("Provides: ");
        let mut prvds = String::new();
        for p in provides {
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

    control
}
