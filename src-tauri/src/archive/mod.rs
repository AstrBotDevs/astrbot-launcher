//! Shared archive extraction utilities.

mod extract;
mod links;
mod path;
mod tar_gz;
mod zip_ops;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ArchiveFormat {
    Zip,
    TarGz,
}

pub(crate) use tar_gz::{extract_tar_gz_flat, extract_tar_gz_mapped};
pub(crate) use zip_ops::{extract_zip_flat, extract_zip_mapped};
