//! A simple Artifactory client

use std::io::Write;
use std::path::Path;

use chrono::offset::FixedOffset;
use chrono::DateTime;
use serde_derive::*;
use thiserror::Error;
use url::Url;
use reqwest::Method;
use reqwest::{IntoUrl, RequestBuilder, Response};
use reqwest::header::{self, HeaderValue};

/// An unauthenticated Artifactory client.
pub struct Client {
    origin: String,
    bearer: Option<String>,
}

impl Client {
    /// Create a new client without any authentication.
    ///
    /// This can be used for read-only access of artifacts.
    pub fn new<S: AsRef<str>>(origin: S) -> Self {
        Self {
            origin: origin.as_ref().to_string(),
            bearer: None,
        }
    }

    pub fn with_bearer<S: AsRef<str>>(mut self, bearer: S) -> Self {
        self.bearer = Some(bearer.as_ref().to_string());
        self
    }

    async fn request<U: IntoUrl>(&self, method: Method, url: U) -> Result<Response, reqwest::Error> {
        let mut headers = header::HeaderMap::new();
        if let Some(bearer) = &self.bearer {
            let mut s = "Bearer ".to_owned();
            s.push_str(&bearer);
            headers.insert("Authorization", HeaderValue::from_str(s.as_ref()).unwrap());
        }
        let client = reqwest::ClientBuilder::new();
        client.default_headers(headers).build().unwrap().request(method, url).send().await
    }

    /// Fetch metadata about a remote artifact.
    pub async fn file_info(&self, path: ArtifactoryPath) -> Result<FileInfo, Error> {
        let url = format!("{}/artifactory/api/storage/{}", self.origin, path.0);
        let url = Url::parse(&url).unwrap();

        let info: FileInfo = self.request(Method::GET, url).await?.json().await?;

        Ok(info)
    }

    /// Fetch a remote artifact.
    ///
    /// An optional progress closure can be provided to get updates on the
    /// transfer.
    pub async fn pull<F>(
        &self,
        path: ArtifactoryPath,
        dest: &Path,
        mut progress: F,
    ) -> Result<(), Error>
    where
        F: FnMut(DownloadProgress),
    {
        let url = format!("{}/artifactory/{}", self.origin, path.0);
        let url = Url::parse(&url).unwrap();

        let mut dest = std::fs::File::create(dest)?;

        let mut res = self.request(Method::GET, url).await?;

        let expected_bytes_downloaded = res.content_length().unwrap_or(0);
        let mut bytes_downloaded = 0;
        while let Some(chunk) = res.chunk().await? {
            bytes_downloaded += chunk.as_ref().len() as u64;
            dest.write_all(chunk.as_ref())?;
            let status = DownloadProgress {
                expected_bytes_downloaded,
                bytes_downloaded,
            };
            progress(status);
        }

        Ok(())
    }
}

/// A path on the remote Artifactory instance.
pub struct ArtifactoryPath(String);

impl<S: AsRef<str>> From<S> for ArtifactoryPath {
    fn from(s: S) -> Self {
        Self(s.as_ref().to_string())
    }
}

/// Metadata on a remote artifact.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    uri: Url,
    download_uri: Url,
    repo: String,
    path: String,
    remote_url: Option<Url>,
    created: DateTime<FixedOffset>,
    created_by: String,
    last_modified: DateTime<FixedOffset>,
    modified_by: String,
    last_updated: DateTime<FixedOffset>,
    size: String,
    mime_type: String,
    pub checksums: Checksums,
    original_checksums: OriginalChecksums,
}

/// File checksums that should always be sent.
#[derive(Debug, Deserialize)]
pub struct Checksums {
    #[serde(with = "hex")]
    pub md5: Vec<u8>,
    #[serde(with = "hex")]
    pub sha1: Vec<u8>,
    #[serde(with = "hex")]
    pub sha256: Vec<u8>,
}

/// File checksums that are only sent if they were originally uploaded.
#[derive(Debug, Deserialize)]
pub struct OriginalChecksums {
    md5: Option<String>,
    sha1: Option<String>,
    sha256: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct DownloadProgress {
    pub expected_bytes_downloaded: u64,
    pub bytes_downloaded: u64,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("An IO error occurred.")]
    Io(#[from] std::io::Error),
    #[error("A HTTP related error occurred.")]
    Reqwest(#[from] reqwest::Error),
}
