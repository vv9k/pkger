//! Helper functions that don't fit anywhere else

pub use flate2;
pub use tar;

use crate::log::{debug, trace, BoxedCollector};
use crate::{ErrContext, Result};

use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;

/// Unpacks a given tar archive to the path specified by `output_dir`
pub fn unpack_tarball<T: io::Read, P: AsRef<Path>>(
    archive: &mut tar::Archive<T>,
    output_dir: P,
    logger: &mut BoxedCollector,
) -> Result<()> {
    let output_dir = output_dir.as_ref();
    debug!(logger => "unpacking archive to {}", output_dir.display());

    for entry in archive.entries()? {
        let mut entry = entry?;
        if let tar::EntryType::Regular = entry.header().entry_type() {
            let path = entry.header().path()?.to_path_buf();
            trace!(logger => "unpacking {}", path.display());
            let name = path.file_name().unwrap_or_default();

            entry.unpack(output_dir.join(name))?;
        }
    }

    Ok(())
}

/// Save the given tar archive as gzip encoded tar to path specified by `output_dir` with the
/// filename set to `name`.
pub fn save_tar_gz<T: io::Read>(
    archive: tar::Archive<T>,
    name: &str,
    output_dir: &Path,
    logger: &mut BoxedCollector,
) -> Result<()> {
    let path = output_dir.join(name);
    debug!(logger => "creating a gzipped tar archive, name: {}, path: {}", name, output_dir.display());

    let f = File::create(path.as_path())?;
    let mut e = GzEncoder::new(f, Compression::default());
    let mut archive = archive.into_inner();
    let mut bytes = Vec::new();
    archive.read_to_end(&mut bytes)?;

    e.write_all(&bytes)?;

    e.finish()?;

    Ok(())
}

/// Creates a tar archive from an iterator of entries consisting of a path and the content of the
/// entry corresponding to the path.
pub fn create_tarball<'archive, E, P>(entries: E, logger: &mut BoxedCollector) -> Result<Vec<u8>>
where
    E: Iterator<Item = (P, &'archive [u8])>,
    P: AsRef<Path>,
{
    debug!(logger => "creating a tar archive");

    let archive_buf = Vec::new();
    let mut archive = tar::Builder::new(archive_buf);

    for entry in entries {
        let path = entry.0.as_ref();
        let size = entry.1.len() as u64;
        trace!(logger => "adding '{}' to archive, size: {}", path.display(), size);
        let mut header = tar::Header::new_gnu();
        header.set_size(size);
        header.set_cksum();
        archive.append_data(&mut header, path, entry.1)?;
    }

    archive.finish()?;

    archive.into_inner().context("failed to create tar archive")
}
