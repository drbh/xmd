use std::fs;
use zed_extension_api::{self as zed, LanguageServerInstallationStatus as Status};

const REPO: &str = "drbh/jot";

struct WtfExtension {
    /// The server this extension downloaded, once it is known to run.
    downloaded: Option<String>,
}

impl WtfExtension {
    /// Where the language server comes from, in order: the `lsp.wtf.binary`
    /// setting, `wtf` on PATH, a server downloaded from the GitHub release
    /// (cached per release in the extension's directory), and inside this
    /// repository a debug build, so the extension works while developing it.
    fn server_path(
        &mut self,
        id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<String> {
        if let Some(path) = worktree.which("wtf") {
            return Ok(path);
        }
        if let Some(path) = &self.downloaded {
            if fs::metadata(path).is_ok_and(|m| m.is_file()) {
                return Ok(path.clone());
            }
        }
        let local = format!("{}/target/debug/wtf", worktree.root_path());
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
        let (target, kind) = match (os, arch) {
            (zed::Os::Mac, zed::Architecture::Aarch64) => ("aarch64-apple-darwin", "tar.gz"),
            (zed::Os::Mac, zed::Architecture::X8664) => ("x86_64-apple-darwin", "tar.gz"),
            (zed::Os::Linux, zed::Architecture::Aarch64) => ("aarch64-unknown-linux-gnu", "tar.gz"),
            (zed::Os::Linux, zed::Architecture::X8664) => ("x86_64-unknown-linux-gnu", "tar.gz"),
            (zed::Os::Windows, zed::Architecture::X8664) => ("x86_64-pc-windows-msvc", "zip"),
            (os, arch) => {
                return Err(format!(
                    "no prebuilt wtf for {os:?}/{arch:?}; install it with `cargo install --git https://github.com/{REPO} wtf` so it is on PATH"
                ));
            }
        };
        let name = format!("wtf-{target}.{kind}");
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .ok_or_else(|| format!("release {} has no asset {name}", release.version))?;

        let dir = format!("wtf-{}", release.version);
        let exe = if os == zed::Os::Windows {
            "wtf.exe"
        } else {
            "wtf"
        };
        let path = format!("{dir}/{exe}");
        if fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            return Ok(path);
        }

        zed::set_language_server_installation_status(id, &Status::Downloading);
        let file_type = if kind == "zip" {
            zed::DownloadedFileType::Zip
        } else {
            zed::DownloadedFileType::GzipTar
        };
        zed::download_file(&asset.download_url, &dir, file_type)
            .map_err(|error| format!("failed to download {}: {error}", asset.download_url))?;
        zed::make_file_executable(&path)?;

        // Earlier releases are not needed once this one is in place.
        if let Ok(entries) = fs::read_dir(".") {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                if file_name.starts_with("wtf-") && file_name != dir {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
        Ok(path)
    }
}

impl zed::Extension for WtfExtension {
    fn new() -> Self {
        Self { downloaded: None }
    }

    fn language_server_command(
        &mut self,
        id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        if let Some(binary) = zed::settings::LspSettings::for_worktree("wtf", worktree)?.binary {
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

zed::register_extension!(WtfExtension);
