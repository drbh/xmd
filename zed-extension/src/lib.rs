use zed_extension_api as zed;

struct WtfExtension;

impl zed::Extension for WtfExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
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
        let root = worktree.root_path();
        let binary = format!("{root}/target/debug/wtf");

        Ok(zed::Command {
            command: binary,
            args: vec!["lsp".into()],
            env: vec![],
        })
    }
}

zed::register_extension!(WtfExtension);
