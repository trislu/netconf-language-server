use zed_extension_api::{self as zed, Os, Result};

struct NetconfExtension;

const THIS_LANGUAGE_SERVER: &'static str = "netconf-language-server";
const THIS_REPOSITORY_OWNER: &'static str = "trislu";

impl zed::Extension for NetconfExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let server_path = find_server_path(language_server_id, worktree)?;
        Ok(zed::Command {
            command: server_path,
            args: vec![],
            env: worktree.shell_env(),
        })
    }

    fn language_server_workspace_configuration(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<serde_json::Value>> {
        let settings =
            zed::settings::LspSettings::for_worktree(&language_server_id.to_string(), worktree)?;

        // Forward the language server settings (indentSize, …) to the server
        // so it receives them via workspace/configuration.
        if let Some(settings) = settings.settings {
            return Ok(Some(settings));
        }

        Ok(None)
    }
}

fn find_server_path(
    language_server_id: &zed::LanguageServerId,
    worktree: &zed::Worktree,
) -> Result<String> {
    // 1. Read a binary path from the Zed lsp setting, as the user might have
    //    configured a different server.
    if let Ok(settings) =
        zed::settings::LspSettings::for_worktree(&language_server_id.to_string(), worktree)
        && let Some(binary) = settings.binary
        && let Some(path) = binary.path
    {
        let path = path.trim().to_string();
        if !path.is_empty() {
            // A locally-resolved server has no installation status; clear any
            // stale "failed" state from a previous run.
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::None,
            );
            return Ok(path);
        }
    }

    // 2. If the language server is not customized, attempt to find this one in $PATH
    if let Some(path) = worktree.which(THIS_LANGUAGE_SERVER) {
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::None,
        );
        return Ok(path);
    }

    // 3. Try to download a prebuilt binary from GitHub releases.
    zed::set_language_server_installation_status(
        language_server_id,
        &zed::LanguageServerInstallationStatus::CheckingForUpdate,
    );

    let (os, _arch) = zed::current_platform();
    let binary_name = format!(
        "{THIS_LANGUAGE_SERVER}-{}",
        match os {
            Os::Mac => "darwin",
            Os::Linux => "linux",
            Os::Windows => "win32.exe",
        }
    );

    // Resolve the binary through the newest *stable* release. Use GitHub's
    // `/releases/latest/download/<asset>` redirect rather than
    // `latest_github_release`, so the download never depends on the
    // rate-limited unauthenticated REST API (60 req/h per IP).
    zed::set_language_server_installation_status(
        language_server_id,
        &zed::LanguageServerInstallationStatus::Downloading,
    );

    let repo = format!("{THIS_REPOSITORY_OWNER}/{THIS_LANGUAGE_SERVER}");
    let download_url = format!("https://github.com/{repo}/releases/latest/download/{binary_name}");
    zed::download_file(
        &download_url,
        &binary_name,
        zed::DownloadedFileType::Uncompressed,
    )
    .map_err(|e| {
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::Failed(format!(
                "failed to download language server: {e}"
            )),
        );
        format!(
            "failed to download {binary_name} from {download_url} ({e}); \
             install the server with `cargo install {THIS_LANGUAGE_SERVER}` or \
             set its path under lsp.{THIS_LANGUAGE_SERVER}.binary in settings"
        )
    })?;

    zed::make_file_executable(&binary_name).ok();

    Ok(binary_name)
}

zed::register_extension!(NetconfExtension);
