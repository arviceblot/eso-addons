use snafu::Snafu;
use std::{io, path::PathBuf};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    #[snafu(display("Unable to read configuration from {}: {}", path.display(), source))]
    ReadConfiguration { source: io::Error, path: PathBuf },

    #[snafu(display("Unable to write result to {}: {}", path.display(), source))]
    WriteResult { source: io::Error, path: PathBuf },

    #[snafu(display("Unable to load config at {}: {}", path.display(), source))]
    ConfigLoad { source: io::Error, path: PathBuf },

    #[snafu(display("Unable to parse config at {}: {}", path.display(), source))]
    ConfigParse {
        source: serde_json::Error,
        path: PathBuf,
    },

    #[snafu(display("Unable to serialize config at {}: {}", path.display(), source))]
    ConfigWriteFormat {
        source: serde_json::Error,
        path: PathBuf,
    },

    #[snafu(display("Unable to write config at {}: {}", path.display(), source))]
    ConfigWrite { source: io::Error, path: PathBuf },

    #[snafu(display("DbGet error: {}", source))]
    DbGet { source: sea_orm::DbErr },

    #[snafu(display("DbPut error: {}", source))]
    DbPut { source: sea_orm::DbErr },

    #[snafu(display("DbDelete error: {}", source))]
    DbDelete { source: sea_orm::DbErr },

    #[snafu(display("Error '{}' getting url {}: {}", get_status_code(source), url, source))]
    ApiGetUrl { source: reqwest::Error, url: String },

    #[snafu(display("Error parsing response from url {}: {}", url, source))]
    ApiParseResponse { source: reqwest::Error, url: String },

    #[snafu(display("Error deserializing response: {}", source))]
    ApiDeserialize { source: serde_json::Error },

    #[snafu(display("Error with addon directory metadata {}: {}", dir.display(), source))]
    AddonDirMetadata { source: io::Error, dir: PathBuf },

    #[snafu(display("Error with addon metadata file {}: {}", path.display(), source))]
    AddonMetadataFile { source: io::Error, path: PathBuf },

    #[snafu(display("Missing metadata file for addon: {}", addon))]
    AddonMetadataFileMissing { addon: String },

    #[snafu(display("Error deleting addon directory {}: {}", dir.display(), source))]
    AddonDelete { source: io::Error, dir: PathBuf },

    #[snafu(display("Error creating temp file: {}", source))]
    AddonDownloadTmpFile { source: io::Error },

    #[snafu(display("Error reading temp file: {}", source))]
    AddonDownloadTmpFileRead { source: io::Error },

    #[snafu(display("Error writing temp file: {}", source))]
    AddonDownloadTmpFileWrite { source: io::Error },

    #[snafu(display("Error getting zip file: {}", source))]
    AddonDownloadZipCreate { source: zip::result::ZipError },

    #[snafu(display("Error reading zip file at {}: {}", file, source))]
    AddonDownloadZipRead {
        source: zip::result::ZipError,
        file: usize,
    },

    #[snafu(display("Error extracting from zip file at {}: {}", path.display(), source))]
    AddonDownloadZipExtract { source: io::Error, path: PathBuf },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

// From: https://github.com/awslabs/tough/blob/develop/tuftool/src/error.rs
// Extracts the status code from a reqwest::Error and converts it to a string to be displayed
fn get_status_code(source: &reqwest::Error) -> String {
    source
        .status()
        .as_ref()
        .map_or("Unknown", reqwest::StatusCode::as_str)
        .to_string()
}
