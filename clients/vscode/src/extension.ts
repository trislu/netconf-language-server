// MIT License — see repository LICENSE.
//
// Activates the netconf-language-server language client for `yang` documents.

import { commands, ExtensionContext, ProgressLocation, RelativePattern, Uri, window, workspace } from "vscode"
import type { WorkspaceFolder } from "vscode"
import {
    Executable,
    ExecuteCommandRequest,
    LanguageClient,
    LanguageClientOptions,
} from "vscode-languageclient/node"

const extension_id = "netconf"
const language_id = "yang"
const server_name = "netconf-language-server"
const output_channel = window.createOutputChannel(extension_id, { log: true })

function log(msg: string) {
    output_channel.appendLine(`[${extension_id}]: ${msg}`)
}

// Whether `folder` holds at least one `.yang` file. This searches the tree the
// way the activation glob does (honouring the user's search excludes) and stops
// at the first hit.
async function holds_yang(folder: WorkspaceFolder): Promise<boolean> {
    try {
        const found = await workspace.findFiles(new RelativePattern(folder, "**/*.yang"), undefined, 1)
        return found.length > 0
    } catch (err) {
        log(`cannot search ${folder.uri.fsPath} for YANG modules: ${err}`)
        return false
    }
}

// The folder to index, or `undefined` when this window has nothing to serve.
//
// The server indexes exactly one root, so the folder is picked before the
// client starts: a workspace folder without a `.yang` file has nothing the
// server can resolve, and several YANG folders are ambiguous — see
// docs/multi-root-workspaces.md.
async function served_folder(folders: readonly WorkspaceFolder[]): Promise<WorkspaceFolder | undefined> {
    const candidates: WorkspaceFolder[] = []
    for (const folder of folders) {
        if (await holds_yang(folder)) {
            candidates.push(folder)
        }
    }
    if (candidates.length > 1) {
        // A warning, not an error: the window is usable, the user just has to
        // decide which folder the server should serve.
        const names = candidates.map((folder) => folder.name).join(", ")
        void window.showWarningMessage(
            `${extension_id}: several workspace folders hold YANG modules (${names}); open one of them to have it served`,
        )
        return undefined
    }
    return candidates[0]
}

// The VS Code target this host runs, spelled the way the packaging matrix and
// the Marketplace name it (`<platform>-<arch>`) — 32-bit Linux is `armhf`, not
// `arm`.
function host_target(): string {
    const arch = process.arch === "arm" ? "armhf" : process.arch
    return `${process.platform}-${arch}`
}

async function find_language_server(context: ExtensionContext): Promise<string | undefined> {
    log("finding language server binary...")
    const debug_server = process.env["__DEBUG_LSP_SERVER"]
    if (debug_server) {
        log(`found debug server: ${debug_server}`)
        return debug_server
    }

    // One VSIX carries one binary: the Marketplace records its target platform
    // in the manifest and the archive holds only that target's build.
    const ext = process.platform === "win32" ? ".exe" : ""
    const binary_name = `${server_name}${ext}`
    const bundled_path = Uri.joinPath(context.extensionUri, "resources", "bin", host_target(), binary_name)
    const bundled_exist = await workspace.fs.stat(bundled_path).then(
        () => true,
        () => false,
    )
    if (!bundled_exist) {
        log(`bundled server ${bundled_path} not exist`)
        return undefined
    }
    log(`found bundled server: ${bundled_path.fsPath}`)
    return bundled_path.fsPath
}

// Glob patterns are still glob syntax when VS Code matches them against a
// document's `fsPath`, so a folder named `netconf [2026]` would turn into a
// character class and match nothing — silently, with the extension serving no
// file at all. `RelativePattern` compares its base literally and normalises it
// for the platform's separators; the protocol's `DocumentFilter.pattern` type
// only admits a string, hence the cast.
function in_folder(folder: Uri, glob: string): string {
    return new RelativePattern(folder, glob) as unknown as string
}

// Commands contributed in the manifest have to be registered even when this
// window ends up serving nothing: VS Code reports a command whose activation
// left it unregistered as "command not found", which is exactly the kind of
// notification an extension that serves one language should not produce.
function register_commands(context: ExtensionContext, get_client: () => LanguageClient | undefined) {
    // NETCONF skeleton insert commands (M2): forward the active editor + caret
    // to the server's `netconf/insertTemplate` command, which applies the
    // template via a workspace edit.
    const template_commands: Array<[string, string]> = [
        ["netconf.insertGetConfigRpc", "get-config"],
        ["netconf.insertEditConfigRpc", "edit-config"],
        ["netconf.insertHello", "hello"],
        ["netconf.insertConfigPayload", "config"],
    ]
    for (const [command, kind] of template_commands) {
        context.subscriptions.push(
            commands.registerCommand(command, async () => {
                const client = get_client()
                if (!client) {
                    log(`${command}: no language server in this window`)
                    return
                }
                const editor = window.activeTextEditor
                if (!editor) {
                    return
                }
                const sel = editor.selection.active
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
                    })
                } catch (err) {
                    log(`template insert failed: ${err}`)
                }
            }),
        )
    }

    // Restart the language server (useful after changing configuration, or
    // when the server gets into a bad state). `client.restart()` re-runs the
    // initialize handshake, so the client-driven progress bar covers the
    // server's workspace rescan again.
    context.subscriptions.push(
        commands.registerCommand("netconf.restart", async () => {
            const client = get_client()
            if (!client) {
                log("netconf.restart: no language server in this window")
                return
            }
            log("restarting language client...")
            try {
                await window.withProgress(
                    {
                        location: ProgressLocation.Notification,
                        title: "NETCONF Language Server",
                        cancellable: false,
                    },
                    async (progress) => {
                        progress.report({ message: "Restarting Language Server..." })
                        await client.restart()
                    },
                )
                log("language client restarted.")
            } catch (err) {
                log(`language client restart failed: ${err}`)
                await window.showErrorMessage(`netconf-language-server: restart failed: ${err}`)
            }
        }),
    )
    log("commands registered.")
}

export async function activate(context: ExtensionContext) {
    log(`activating extension ${extension_id}...`)

    // Registered first so that even a window which serves nothing answers its
    // own commands.
    let client: LanguageClient | undefined
    register_commands(context, () => client)

    const ws = workspace.workspaceFolders ?? []
    if (ws.length === 0) {
        // Reached through `onLanguage:yang`: a lone module opened outside a
        // workspace has no folder to index. Every other activation route needs
        // a folder, so this is a window with nothing to serve either way.
        log("no workspace folder is open; not starting the language server")
        return
    }

    // Decide before touching the binary: a window that has nothing to serve
    // must stay quiet, including when this platform ships no server at all.
    const root = await served_folder(ws)
    if (!root) {
        log("no single folder with YANG modules to serve; not starting the language server")
        return
    }

    const language_server = await find_language_server(context)
    if (!language_server) {
        await window.showErrorMessage(
            `${extension_id}: no language server for ${host_target()}, you may raise an issue`,
        )
        return
    }

    const root_dir = root.uri

    const server_exec: Executable = {
        command: String(language_server),
        options: { env: { ...process.env } },
    }

    const clientOptions: LanguageClientOptions = {
        documentSelector: [
            // YANG modules (authoring) …
            { language: language_id, pattern: in_folder(root_dir, "**/*.yang"), scheme: "file" },
            // … and candidate NETCONF instance documents. The server
            // content-sniffs these against the compiled YANG library and
            // stays dormant for files that are not NETCONF (M0, D19). The
            // built-in XML/JSON extensions keep providing tokens/folding/
            // formatting because the server declines those for xml/json.
            { language: "xml", pattern: in_folder(root_dir, "**/*.xml"), scheme: "file" },
            { language: "json", pattern: in_folder(root_dir, "**/*.json"), scheme: "file" },
        ],
        diagnosticCollectionName: extension_id,
        outputChannel: output_channel,
    }

    const language_client = new LanguageClient(
        extension_id,
        extension_id,
        { run: server_exec, debug: server_exec },
        clientOptions,
    )
    client = language_client

    // Forward configuration changes to the server (it also re-pulls via
    // workspace/configuration on startup).
    context.subscriptions.push(
        workspace.onDidChangeConfiguration((e) => {
            if (client && e.affectsConfiguration(extension_id)) {
                client.sendNotification("workspace/didChangeConfiguration", {
                    settings: workspace.getConfiguration(extension_id),
                })
            }
        }),
    )

    log("starting language client...")
    // Show a visible progress while the server performs its initial workspace
    // scan. The server blocks its `initialize` response until that scan
    // finishes, and `client.start()` only resolves once that response arrives —
    // so this bar's duration matches the scan exactly. (The server cannot send
    // `$/progress` during `initialize`: tower-lsp drops pre-initialize
    // notifications, so a client-driven bar is the reliable way to show it.)
    await window.withProgress(
        {
            location: ProgressLocation.Notification,
            title: "NETCONF Language Server",
            cancellable: false,
        },
        async (progress) => {
            progress.report({ message: "Indexing Workspace..." })
            await language_client.start()
        },
    )
    log("language client started.")
}

export function deactivate() {
    log(`deactivating extension ${extension_id}...`)
}
