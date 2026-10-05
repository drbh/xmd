use crate::output;
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) struct Release {
    pub(crate) version: String,
    pub(crate) github_auth: bool,
}

impl Release {
    pub(crate) fn resolve(version: &str, github_auth: bool) -> Result<Self> {
        let version = if version == "latest" {
            if github_auth {
                output(Command::new("gh").args([
                    "api",
                    "repos/drbh/xmd/releases/latest",
                    "--jq",
                    ".tag_name",
                ]))?
            } else {
                let url = output(Command::new("curl").args([
                    "-fsSL",
                    "-o",
                    "/dev/null",
                    "-w",
                    "%{url_effective}",
                    "https://github.com/drbh/xmd/releases/latest",
                ]))?;
                url.rsplit('/')
                    .next()
                    .context("invalid release URL")?
                    .to_owned()
            }
        } else {
            version.to_owned()
        };
        let version = version.trim().trim_start_matches('v').to_owned();
        ensure!(
            !version.is_empty()
                && version
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b".+-".contains(&c)),
            "invalid release version"
        );
        Ok(Self {
            version,
            github_auth,
        })
    }

    fn fetch(&self, name: &str, directory: &Path) -> Result<()> {
        if self.github_auth {
            output(
                Command::new("gh")
                    .args([
                        "release",
                        "download",
                        &format!("v{}", self.version),
                        "--repo",
                        "drbh/xmd",
                        "--pattern",
                        name,
                        "--dir",
                    ])
                    .arg(directory),
            )?;
        } else {
            output(
                Command::new("curl")
                    .args([
                        "-fsSL",
                        &format!(
                            "https://github.com/drbh/xmd/releases/download/v{}/{name}",
                            self.version
                        ),
                        "-o",
                    ])
                    .arg(directory.join(name)),
            )?;
        }
        Ok(())
    }

    pub(crate) fn download(&self, name: &str, directory: &Path) -> Result<PathBuf> {
        self.fetch(name, directory)?;
        self.fetch(&format!("{name}.sha256"), directory)?;
        let checksum = fs::read_to_string(directory.join(format!("{name}.sha256")))?;
        let expected = checksum
            .split_whitespace()
            .next()
            .context("empty checksum")?;
        let path = directory.join(name);
        let actual = format!("{:x}", Sha256::digest(fs::read(&path)?));
        ensure!(expected == actual, "checksum mismatch for {name}");
        Ok(path)
    }
}
