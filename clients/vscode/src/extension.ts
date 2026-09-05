// MIT License — see repository LICENSE.
//
// Activates the netconf-language-server language client for `yang` documents.

import { commands, ExtensionContext, Uri, window, workspace } from "vscode";
import {
    Executable,
    ExecuteCommandRequest,
    LanguageClient,
} from "vscode-languageclient/node";

const extension_id = "netconf";
const language_id = "yang";
const server_name = "netconf-language-server";
const output_channel = window.createOutputChannel(extension_id, { log: true });

function log(msg: string) {
    output_channel.appendLine(`[${extension_id}]: ${msg}`);
}

async function find_language_server(context: ExtensionContext): Promise<string | undefined> {
    log("finding language server binary...");
    const debug_server = process.env["__DEBUG_LSP_SERVER"];
    if (debug_server) {
        log(`found debug server: ${debug_server}`);
        return debug_server;
    }

    const platform = process.platform;
    const ext = platform === "win32" ? ".exe" : "";
    const binary_name = `${server_name}-${platform}${ext}`;
    const bundled_path = Uri.joinPath(context.extensionUri, "resources", "bin", binary_name);
    const bundled_exist = await workspace.fs.stat(bundled_path).then(
        () => true,
        () => false,
    );
    if (!bundled_exist) {
        log(`bundled server ${bundled_path} not exist`);
        return undefined;
    }
    log(`found bundled server: ${bundled_path.fsPath}`);
    return bundled_path.fsPath;
}

export async function activate(context: ExtensionContext) {
    log(`activating extension ${extension_id}...`);

    const language_server = await find_language_server(context);
    if (!language_server) {
        await window.showErrorMessage("netconf-language-server: language server not found, you may raise an issue");
        return;
    }

    const ws = workspace.workspaceFolders ?? [];
    if (ws.length > 1) {
        await window.showErrorMessage("netconf-language-server: multiple workspaces are not supported");
        return;
    }
    if (ws.length === 0) {
        await window.showErrorMessage("netconf-language-server: open a workspace folder containing .yang files");
        return;
    }
    const root_dir = ws[0].uri;

    const server_exec: Executable = {
        command: String(language_server),
        options: { env: { ...process.env } },
    };

    const client = new LanguageClient(
        extension_id,
        extension_id,
        { run: server_exec, debug: server_exec },
        {
            documentSelector: [
                // YANG modules (authoring) …
                { language: language_id, pattern: `${root_dir.fsPath}/**/*.yang`, scheme: "file" },
                // … and candidate NETCONF instance documents. The server
                // content-sniffs these against the compiled YANG library and
                // stays dormant for files that are not NETCONF (M0, D19). The
                // built-in XML/JSON extensions keep providing tokens/folding/
                // formatting because the server declines those for xml/json.
                { language: "xml", pattern: `${root_dir.fsPath}/**/*.xml`, scheme: "file" },
                { language: "json", pattern: `${root_dir.fsPath}/**/*.json`, scheme: "file" },
            ],
            diagnosticCollectionName: extension_id,
            outputChannel: output_channel,
        },
    );

    // Forward configuration changes to the server (it also re-pulls via
    // workspace/configuration on startup).
    context.subscriptions.push(
        workspace.onDidChangeConfiguration((e) => {
            if (e.affectsConfiguration(extension_id)) {
                client.sendNotification("workspace/didChangeConfiguration", {
                    settings: workspace.getConfiguration(extension_id),
                });
            }
        }),
    );

    log("starting language client...");
    await client.start();
    log("language client started.");

    // NETCONF skeleton insert commands (M2): forward the active editor + caret
    // to the server's `netconf/insertTemplate` command, which applies the
    // template via a workspace edit.
    const template_commands: Array<[string, string]> = [
        ["netconf.insertGetConfigRpc", "get-config"],
        ["netconf.insertEditConfigRpc", "edit-config"],
        ["netconf.insertHello", "hello"],
        ["netconf.insertConfigPayload", "config"],
    ];
    for (const [command, kind] of template_commands) {
        context.subscriptions.push(
            commands.registerCommand(command, async () => {
                const editor = window.activeTextEditor;
                if (!editor) {
                    return;
                }
                const sel = editor.selection.active;
                try {
                    await client.sendRequest(ExecuteCommandRequest.type, {
                        command: "netconf/insertTemplate",
                        arguments: [
                            {
                                uri: editor.document.uri.toString(),
                                kind,
                                position: { line: sel.line, character: sel.character },
                            },
                        ],
                    });
                } catch (err) {
                    log(`template insert failed: ${err}`);
                }
            }),
        );
    }
    log("template commands registered.");
}

export function deactivate() {
    log(`deactivating extension ${extension_id}...`);
}
