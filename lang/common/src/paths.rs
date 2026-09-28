//! File-URI conversions for both real files and browser virtual files.
use lsp_types::Uri;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use url::Url;

/// Convert to the LSP wire type at the boundary where an `lsp_types` struct is
/// built. `url::Url` and `lsp_types::Uri` (`fluent_uri`) both store an
/// already percent-encoded string, so reparsing it is a lossless bridge: no
/// re-encoding happens, so serialized URIs are byte-for-byte identical to
/// what `url::Url` would have produced.
pub fn uri_from_url(url: &Url) -> Uri {
    Uri::from_str(url.as_str()).unwrap_or_else(|e| panic!("Invalid URI from {url}: {e}"))
}

/// Convert back from the LSP wire type at the boundary where an incoming
/// `lsp_types` struct is read. See [`uri_from_url`] for why this is lossless.
pub fn url_from_uri(uri: &Uri) -> Url {
    Url::parse(uri.as_str()).unwrap_or_else(|e| panic!("Invalid URL from {}: {e}", uri.as_str()))
}

pub fn file_url(path: impl AsRef<Path>) -> Result<Url, String> {
    #[cfg(not(target_arch = "wasm32"))]
    return Url::from_file_path(path).map_err(|_| "Expected an absolute file path".into());
    #[cfg(target_arch = "wasm32")]
    {
        let path = path.as_ref().to_str().ok_or("Expected a UTF-8 path")?;
        let path = path
            .strip_prefix('/')
            .ok_or("Expected an absolute virtual path")?;
        let mut url = Url::parse("file:///").unwrap();
        url.path_segments_mut()
            .unwrap()
            .clear()
            .extend(path.split('/'));
        Ok(url)
    }
}

/// The URI for a path a host already knows to be absolute: a note it read from
/// disk, or a buffer an editor opened. A relative path is a bug in the caller.
pub fn uri(path: impl AsRef<Path>) -> Url {
    let path = path.as_ref();
    file_url(path).unwrap_or_else(|e| panic!("{e}: {}", path.display()))
}

pub fn file_path(url: &Url) -> Result<PathBuf, String> {
    #[cfg(not(target_arch = "wasm32"))]
    return url
        .to_file_path()
        .map_err(|_| "Expected a local file URI".into());
    #[cfg(target_arch = "wasm32")]
    {
        if url.scheme() != "file"
            || url
                .host_str()
                .is_some_and(|h| !h.is_empty() && h != "localhost")
        {
            return Err("Expected a local virtual-file URI".into());
        }
        let path = percent_encoding::percent_decode_str(url.path())
            .decode_utf8()
            .map_err(|e| e.to_string())?;
        Ok(PathBuf::from(path.as_ref()))
    }
}
