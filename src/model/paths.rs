//! File-URI conversions for both real files and browser virtual files.
use lsp_types::Url;
use std::path::{Path, PathBuf};

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
