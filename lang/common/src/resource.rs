//! What a resource target is and where it points, without fetching, caching
//! or presenting anything: that half lives one layer up, in
//! `evaluate::resources`, next to the link registry it depends on.
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub target: String,
    pub origin: Option<PathBuf>,
}
impl Resource {
    pub fn parse(s: &str) -> Option<Self> {
        (s.starts_with("https://")
            || s.starts_with("http://")
            || s.starts_with("geo:")
            || s.starts_with("./")
            || s.starts_with("../")
            || s.starts_with("~/")
            || s.starts_with('/')
            || s.starts_with("file://")
            || bare_file_path(s))
        .then(|| Self {
            target: s.into(),
            origin: None,
        })
    }
    pub fn url(&self, document: &Path) -> Result<Url, String> {
        let document = self.origin.as_deref().unwrap_or(document);
        if let Some(coords) = self.target.strip_prefix("geo:") {
            let (lat, lon) = coords
                .split_once(',')
                .ok_or("geo:latitude,longitude".to_string())?;
            let lat: f64 = lat.parse().map_err(|_| "Invalid latitude".to_string())?;
            let lon: f64 = lon.parse().map_err(|_| "Invalid longitude".to_string())?;
            if !lat.is_finite()
                || !lon.is_finite()
                || !(-90.0..=90.0).contains(&lat)
                || !(-180.0..=180.0).contains(&lon)
            {
                return Err("Coordinates are out of range".into());
            }
            return Url::parse(&format!(
                "https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=16/{lat}/{lon}"
            ))
            .map_err(|e| e.to_string());
        }
        if self.target.starts_with("http://")
            || self.target.starts_with("https://")
            || self.target.starts_with("file://")
        {
            // A malformed URL is an error, never a relative file path.
            return Url::parse(&self.target).map_err(|e| e.to_string());
        }
        if let Ok(url) = Url::parse(&self.target) {
            if matches!(url.scheme(), "https" | "http" | "file") {
                return Ok(url);
            }
            return Err("Unsupported link scheme".into());
        }
        if let Some(relative) = self.target.strip_prefix("~/") {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let home_dir =
                    std::env::var_os("HOME").ok_or("Home directory is unavailable".to_string())?;
                return resolved_file_url(&PathBuf::from(home_dir).join(relative));
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = relative;
                return Err(
                    "Home-directory paths can be opened in the native editor, not the browser"
                        .into(),
                );
            }
        }
        let path = document
            .parent()
            .unwrap_or(Path::new("."))
            .join(&self.target);
        resolved_file_url(&path)
    }
    pub fn is_image(&self) -> bool {
        let target = self
            .target
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_lowercase();
        [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"]
            .iter()
            .any(|e| target.ends_with(e))
    }
}

fn resolved_file_url(path: &Path) -> Result<Url, String> {
    let url = crate::file_url(path)?;
    // from_file_path preserves dot segments; parsing normalizes them without IO.
    // Native and browser links must use the same canonical URI to find open notes.
    Url::parse(url.as_str()).map_err(|e| e.to_string())
}

/// Recognize unprefixed paths without turning fractions, domains, or ordinary
/// prose such as "and/or" into file links. Use ./ for ambiguous extensionless paths.
fn bare_file_path(s: &str) -> bool {
    if s.is_empty()
        || s.chars()
            .any(|c| c.is_whitespace() || "<>\"`|:?!*".contains(c))
    {
        return false;
    }
    if matches!(s, "Makefile" | "Dockerfile" | "LICENSE") {
        return true;
    }
    let file = s.rsplit('/').next().unwrap_or(s);
    if file.starts_with('.')
        && !file.starts_with("..")
        && file.chars().any(|c| c.is_alphabetic() || c == '_')
    {
        return true;
    }
    let Some((stem, extension)) = file.rsplit_once('.') else {
        return false;
    };
    if !stem.chars().any(char::is_alphabetic)
        || extension.is_empty()
        || !extension.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return false;
    }
    s.contains('/')
        || matches!(
            extension.to_ascii_lowercase().as_str(),
            "md" | "txt"
                | "pdf"
                | "rs"
                | "js"
                | "mjs"
                | "cjs"
                | "jsx"
                | "ts"
                | "tsx"
                | "json"
                | "jsonc"
                | "toml"
                | "yaml"
                | "yml"
                | "lock"
                | "html"
                | "css"
                | "scss"
                | "py"
                | "go"
                | "rb"
                | "sh"
                | "zsh"
                | "c"
                | "h"
                | "cpp"
                | "hpp"
                | "swift"
                | "java"
                | "kt"
                | "sql"
                | "csv"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "svg"
                | "mp4"
                | "mov"
                | "mp3"
                | "wav"
                | "zip"
                | "tar"
                | "gz"
                | "log"
                | "wasm"
                | "env"
                | "ini"
        )
}
