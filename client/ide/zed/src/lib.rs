use std::fs;
use zed_extension_api::{self as zed, LanguageServerInstallationStatus as Status};

const REPO: &str = "drbh/xmd";

struct XmdExtension {
    /// The server this extension downloaded, once it is known to run.
    downloaded: Option<String>,
}

impl XmdExtension {
    /// Where the language server comes from, in order: the `lsp.xmd.binary`
    /// setting, `xmd` on PATH, a server downloaded from the GitHub release
    /// (cached per release in the extension's directory), and inside this
    /// repository a debug build, so the extension works while developing it.
    fn server_path(
        &mut self,
        id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<String> {
        if let Some(path) = worktree.which("xmd") {
            return Ok(path);
        }
        if let Some(path) = &self.downloaded {
            if fs::metadata(path).is_ok_and(|m| m.is_file()) {
                return Ok(path.clone());
            }
        }
        let local = format!("{}/target/debug/xmd", worktree.root_path());
        match self.download(id) {
            Ok(path) => {
                self.downloaded = Some(path.clone());
                Ok(path)
            }
            Err(_) if fs::metadata(&local).is_ok() => Ok(local),
            Err(error) => {
                zed::set_language_server_installation_status(id, &Status::Failed(error.clone()));
                Err(error)
            }
        }
    }

    fn download(&self, id: &zed::LanguageServerId) -> zed::Result<String> {
        zed::set_language_server_installation_status(id, &Status::CheckingForUpdate);
        let release = zed::latest_github_release(
            REPO,
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;
        let (os, arch) = zed::current_platform();
        let target = match (os, arch) {
            (zed::Os::Mac, zed::Architecture::Aarch64) => "aarch64-apple-darwin",
            (zed::Os::Mac, zed::Architecture::X8664) => "x86_64-apple-darwin",
            (zed::Os::Linux, zed::Architecture::Aarch64) => "aarch64-unknown-linux-gnu",
            (zed::Os::Linux, zed::Architecture::X8664) => "x86_64-unknown-linux-gnu",
            (os, arch) => {
                return Err(format!(
                    "no prebuilt xmd for {os:?}/{arch:?}; install it with `cargo install --git https://github.com/{REPO} xmd` so it is on PATH"
                ));
            }
        };
        let name = format!("xmd-{target}.tar.gz");
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .ok_or_else(|| format!("release {} has no asset {name}", release.version))?;

        let dir = format!("xmd-{}", release.version);
        let path = format!("{dir}/xmd");
        if fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            return Ok(path);
        }

        zed::set_language_server_installation_status(id, &Status::Downloading);
        zed::download_file(&asset.download_url, &dir, zed::DownloadedFileType::GzipTar)
            .map_err(|error| format!("failed to download {}: {error}", asset.download_url))?;
        zed::make_file_executable(&path)?;

        // Earlier releases are not needed once this one is in place.
        if let Ok(entries) = fs::read_dir(".") {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                if file_name.starts_with("xmd-") && file_name != dir {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
        Ok(path)
    }
}

impl zed::Extension for XmdExtension {
    fn new() -> Self {
        Self { downloaded: None }
    }

    fn language_server_command(
        &mut self,
        id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        if let Some(binary) = zed::settings::LspSettings::for_worktree("xmd", worktree)?.binary {
            if let Some(path) = binary.path {
                return Ok(zed::Command {
                    command: path,
                    args: binary.arguments.unwrap_or_else(|| vec!["lsp".into()]),
                    env: binary.env.unwrap_or_default().into_iter().collect(),
                });
            }
        }
        let command = self.server_path(id, worktree)?;
        zed::set_language_server_installation_status(id, &Status::None);
        Ok(zed::Command {
            command,
            args: vec!["lsp".into()],
            env: vec![],
        })
    }
}

zed::register_extension!(XmdExtension);
