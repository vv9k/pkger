//! Helper functions that don't fit anywhere else

use crate::Result;

use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;
use tracing::{info_span, trace};

pub fn unpack_archive<T: io::Read, P: AsRef<Path>>(
    archive: &mut tar::Archive<T>,
    output_dir: P,
) -> Result<()> {
    let span = info_span!("unpack-archive");
    let _enter = span.enter();

    let output_dir = output_dir.as_ref();

    for entry in archive.entries()? {
        let mut entry = entry?;
        if let tar::EntryType::Regular = entry.header().entry_type() {
            let path = entry.header().path()?.to_path_buf();
            trace!(parent: &span, entry = %path.display(), to = %output_dir.display(), "unpacking");
            let name = path.file_name().unwrap_or_default();

            entry.unpack(output_dir.join(name))?;
        }
    }

    Ok(())
}

pub fn save_tar_gz<T: io::Read>(
    archive: tar::Archive<T>,
    name: &str,
    output_dir: &Path,
) -> Result<()> {
    let path = output_dir.join(name);

    let span = info_span!("unpack-archive", path = %path.display());
    let _enter = span.enter();

    trace!("creating a gzipped tarball");
    let f = File::create(path.as_path())?;
    let mut e = GzEncoder::new(f, Compression::default());
    let mut archive = archive.into_inner();
    let mut bytes = Vec::new();
    archive.read_to_end(&mut bytes)?;

    e.write_all(&bytes)?;

    e.finish()?;

    Ok(())
}
