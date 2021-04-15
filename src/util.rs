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

    let span = info_span!("save-tar-gz", path = %path.display());
    let _enter = span.enter();

    trace!(parent: &span, "creating a gzipped tarball");
    let f = File::create(path.as_path())?;
    let mut e = GzEncoder::new(f, Compression::default());
    let mut archive = archive.into_inner();
    let mut bytes = Vec::new();
    archive.read_to_end(&mut bytes)?;

    e.write_all(&bytes)?;

    e.finish()?;

    Ok(())
}

pub fn create_tar_archive<'archive, E, P>(entries: E) -> Result<Vec<u8>>
where
    E: IntoIterator<Item = (P, &'archive [u8])>,
    P: AsRef<Path>,
{
    let span = info_span!("create-TAR-archive");
    let _enter = span.enter();

    let archive_buf = Vec::new();
    let mut archive = tar::Builder::new(archive_buf);

    for entry in entries {
        let path = entry.0.as_ref();
        let size = entry.1.iter().count() as u64;
        trace!(entry = %path.display(), size = %size, "adding to archive");
        let mut header = tar::Header::new_gnu();
        header.set_size(size);
        header.set_cksum();
        archive.append_data(&mut header, path, entry.1)?;
    }

    archive.finish()?;

    archive
        .into_inner()
        .map_err(|e| anyhow!("failed to create tar archive - {}", e))
}
