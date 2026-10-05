mod release;
mod settings;

use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, ValueEnum};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::{NamedTempFile, TempDir};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Editor {
    Zed,
    Vscode,
}

#[derive(Parser)]
#[command(
    version,
    about = "Install a prebuilt xmd release and optional editor integration"
)]
struct Args {
    #[arg(long)]
    editor: Option<Editor>,
    /// Authenticate private release downloads with the GitHub CLI.
    #[arg(long)]
    github_auth: bool,
    #[arg(long = "release", env = "XMD_VERSION", default_value = "latest")]
    release: String,
    #[arg(long, env = "XMD_BIN_DIR")]
    bin_dir: Option<PathBuf>,
}

fn main() {
    if let Err(error) = install(Args::parse()) {
        eprintln!("xmd-installer: {error:#}");
        std::process::exit(1);
    }
}

fn output(command: &mut Command) -> Result<String> {
    let result = command
        .output()
        .with_context(|| format!("running {}", command.get_program().to_string_lossy()))?;
    ensure!(
        result.status.success(),
        "{} failed: {}",
        command.get_program().to_string_lossy(),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

fn path_env(name: &str, default: PathBuf) -> PathBuf {
    env::var_os(name)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or(default)
}

fn executable(name: &Path) -> bool {
    if name.is_absolute() || name.components().count() > 1 {
        return name.is_file();
    }
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|p| p.join(name).is_file()))
}

fn platform() -> Result<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") => Ok("Darwin-arm64"),
        ("macos", "x86_64") => Ok("Darwin-x86_64"),
        ("linux", "aarch64") => Ok("Linux-aarch64"),
        ("linux", "x86_64") => Ok("Linux-x86_64"),
        _ => bail!("no prebuilt release for this platform"),
    }
}

struct EditorSetup {
    editor: Editor,
    settings: PathBuf,
    data: PathBuf,
    code: PathBuf,
}
impl EditorSetup {
    fn new(editor: Editor, home: &Path) -> Result<Self> {
        let config = path_env("XDG_CONFIG_HOME", home.join(".config"));
        let data = path_env("XDG_DATA_HOME", home.join(".local/share"));
        match editor {
            Editor::Zed => {
                let data = path_env(
                    "XMD_ZED_DATA_DIR",
                    if cfg!(target_os = "macos") {
                        home.join("Library/Application Support/Zed")
                    } else {
                        data.join("zed")
                    },
                );
                let config = path_env(
                    "XMD_ZED_CONFIG_DIR",
                    if cfg!(target_os = "macos") {
                        home.join(".config/zed")
                    } else {
                        config.join("zed")
                    },
                );
                Ok(Self {
                    editor,
                    settings: config.join("settings.json"),
                    data,
                    code: PathBuf::new(),
                })
            }
            Editor::Vscode => {
                let mut code = path_env("XMD_CODE_BIN", "code".into());
                if !executable(&code)
                    && env::var_os("XMD_CODE_BIN").is_none()
                    && cfg!(target_os = "macos")
                {
                    code = "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
                        .into();
                }
                ensure!(
                    executable(&code),
                    "VS Code CLI not found; install VS Code or set XMD_CODE_BIN"
                );
                let data = path_env(
                    "XMD_VSCODE_USER_DATA_DIR",
                    if cfg!(target_os = "macos") {
                        home.join("Library/Application Support/Code")
                    } else {
                        config.join("Code")
                    },
                );
                Ok(Self {
                    editor,
                    settings: data.join("User/settings.json"),
                    data,
                    code,
                })
            }
        }
    }
}

fn read_settings(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("{}\n".into()),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

// Stage on the destination filesystem and rename, including when replacing a symlink.
fn write_atomic(path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
    let parent = path.parent().context("destination has no parent")?;
    fs::create_dir_all(parent)?;
    let mut stage = NamedTempFile::new_in(parent)?;
    stage.write_all(bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable {
            0o755
        } else {
            fs::metadata(path)
                .map(|m| m.permissions().mode())
                .unwrap_or(0o600)
        };
        stage
            .as_file()
            .set_permissions(fs::Permissions::from_mode(mode))?;
    }
    stage.persist(path)?;
    Ok(())
}

fn backup_file(path: &Path, backup: &Path) -> Result<()> {
    if path.exists() {
        fs::copy(path, backup)?;
    }
    Ok(())
}

fn install(args: Args) -> Result<()> {
    let platform = platform()?;
    let home = PathBuf::from(env::var_os("HOME").context("HOME is not set")?);
    let setup = args
        .editor
        .map(|e| EditorSetup::new(e, &home))
        .transpose()?;
    let release = release::Release::resolve(&args.release, args.github_auth)?;
    println!("downloading xmd v{}", release.version);
    let temp = TempDir::new()?;
    let archive = release.download(&format!("xmd-{platform}.tar.gz"), temp.path())?;
    output(
        Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(temp.path())
            .arg("xmd"),
    )?;
    let binary = temp.path().join("xmd");
    ensure!(
        output(Command::new(&binary).arg("--version"))? == format!("xmd {}", release.version),
        "binary version mismatch"
    );
    let bin = args.bin_dir.unwrap_or_else(|| home.join(".local/bin"));
    fs::create_dir_all(&bin)?;
    let destination = fs::canonicalize(bin)?.join("xmd");
    let mut settings_text = None;
    if let Some(setup) = &setup {
        match setup.editor {
            Editor::Zed => {
                let archive = release.download("xmd-zed.tar.gz", temp.path())?;
                let unpacked = temp.path().join("zed");
                fs::create_dir(&unpacked)?;
                output(
                    Command::new("tar")
                        .arg("-xzf")
                        .arg(archive)
                        .arg("-C")
                        .arg(&unpacked),
                )?;
                ensure!(
                    unpacked.join("extension.wasm").is_file()
                        && unpacked.join("extension.toml").is_file(),
                    "incomplete Zed package"
                );
            }
            Editor::Vscode => {
                release.download("xmd.vsix", temp.path())?;
            }
        }
        settings_text = Some(settings::configure(
            &read_settings(&setup.settings)?,
            setup.editor,
            &destination,
        )?);
    }
    let backups = path_env(
        "XMD_BACKUP_DIR",
        path_env("XDG_DATA_HOME", home.join(".local/share")).join("xmd/backups"),
    );
    fs::create_dir_all(&backups)?;
    let backup = tempfile::Builder::new()
        .prefix("install.")
        .tempdir_in(backups)?
        .keep();
    println!("backups: {}", backup.display());
    backup_file(&destination, &backup.join("xmd"))?;
    write_atomic(&destination, &fs::read(binary)?, true)?;
    println!(
        "installed xmd {} to {}",
        release.version,
        destination.display()
    );
    if let Some(setup) = &setup {
        if matches!(setup.editor, Editor::Zed) {
            let installed = setup.data.join("extensions/installed");
            fs::create_dir_all(&installed)?;
            let stage = tempfile::Builder::new()
                .prefix(".xmd.")
                .tempdir_in(setup.data.join("extensions"))?;
            output(
                Command::new("cp")
                    .arg("-R")
                    .arg(temp.path().join("zed/."))
                    .arg(stage.path()),
            )?;
            let target = installed.join("xmd");
            if fs::symlink_metadata(&target).is_ok() {
                // mv preserves a development symlink, and supports separate filesystems.
                output(
                    Command::new("mv")
                        .arg(&target)
                        .arg(backup.join("zed-extension")),
                )?;
            }
            fs::rename(stage.path(), &target)?;
            println!("installed Zed extension to {}", target.display());
        }
        let editor = match setup.editor {
            Editor::Zed => "zed",
            Editor::Vscode => "vscode",
        };
        backup_file(
            &setup.settings,
            &backup.join(format!("{editor}-settings.json")),
        )?;
        write_atomic(
            &setup.settings,
            settings_text.as_ref().unwrap().as_bytes(),
            false,
        )?;
        if matches!(setup.editor, Editor::Vscode) {
            let mut command = Command::new(&setup.code);
            command.arg("--user-data-dir").arg(&setup.data);
            if let Some(dir) = env::var_os("XMD_VSCODE_EXTENSIONS_DIR") {
                command.arg("--extensions-dir").arg(dir);
            }
            command
                .arg("--install-extension")
                .arg(temp.path().join("xmd.vsix"))
                .arg("--force");
            println!("{}", output(&mut command)?);
        }
        println!(
            "configured {editor} highlighting and server path (existing preferences preserved)"
        );
        println!(
            "reopen your note or restart its language server; approve normal workspace trust if prompted"
        );
    }
    let parent = destination.parent().unwrap();
    if !env::var_os("PATH").is_some_and(|p| env::split_paths(&p).any(|p| p == parent)) {
        println!("for terminal use, add {} to PATH", parent.display());
    }
    Ok(())
}
