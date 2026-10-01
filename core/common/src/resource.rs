//! What a resource target is and where it points, without fetching, caching
//! or presenting anything: that half lives one layer up, in
//! `evaluate::resources`, next to the link registry it depends on.
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Resource {
    pub target: String,
    pub origin: Option<PathBuf>,
}
impl Resource {
    /// `target` as written, with no note of its own to resolve against.
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            origin: None,
        }
    }
    /// The same target, resolved against the note `origin` rather than the
    /// one it is read in.
    pub fn with_origin(mut self, origin: impl Into<PathBuf>) -> Self {
        self.origin = Some(origin.into());
        self
    }
    pub fn parse(s: &str) -> Option<Self> {
        let prefixes = [
            "https://", "http://", "geo:", "./", "../", "~/", "/", "file://",
        ];
        (prefixes.iter().any(|p| s.starts_with(p)) || bare_file_path(s)).then(|| Self::new(s))
    }
    /// Where the target points, relative to `document` (or the note it came
    /// from). `home` is the user's home directory, which `~/` paths resolve
    /// against; the host supplies it, and a browser has none.
    pub fn url(&self, document: &Path, home: Option<&Path>) -> Result<Url, String> {
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
            let home = home.ok_or(if cfg!(target_arch = "wasm32") {
                "Home-directory paths can be opened in the native editor, not the browser"
            } else {
                "Home directory is unavailable"
            })?;
            return resolved_file_url(&home.join(relative));
        }
        let directory = document.parent().unwrap_or(Path::new("."));
        resolved_file_url(&directory.join(&self.target))
    }
    pub fn is_image(&self) -> bool {
        let path = self.target.split(['?', '#']).next().unwrap_or("");
        let target = path.to_lowercase();
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
    s.contains('/') || KNOWN_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
}

/// The extensions that make a bare `name.ext`, with no `/`, a file path.
const KNOWN_EXTENSIONS: &[&str] = &[
    "md", "txt", "pdf", "rs", "js", "mjs", "cjs", "jsx", "ts", "tsx", "json", "jsonc", "toml",
    "yaml", "yml", "lock", "html", "css", "scss", "py", "go", "rb", "sh", "zsh", "c", "h", "cpp",
    "hpp", "swift", "java", "kt", "sql", "csv", "png", "jpg", "jpeg", "gif", "webp", "svg", "mp4",
    "mov", "mp3", "wav", "zip", "tar", "gz", "log", "wasm", "env", "ini",
];
