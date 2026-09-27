use zed_extension_api as zed;

const SERVER_NAME: &str = "zed-vertical-align-lsp";

struct VerticalAlignExtension;

impl zed::Extension for VerticalAlignExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let path = worktree.which(SERVER_NAME).ok_or_else(|| {
            format!(
                "{SERVER_NAME} was not found in Zed's PATH. Build it with scripts/build.sh, then run scripts/install-helper.sh --bin-dir <directory-on-Zed-PATH>."
            )
        })?;

        Ok(zed::Command {
            command: path,
            args: vec!["--stdio".to_string()],
            env: Default::default(),
        })
    }
}

zed::register_extension!(VerticalAlignExtension);
