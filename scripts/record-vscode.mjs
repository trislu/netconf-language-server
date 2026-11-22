// Record the README demos from a real VS Code window on a virtual display.
//
//   node scripts/record-vscode.mjs              # every story
//   node scripts/record-vscode.mjs yang xml     # just these
//
// The script starts Xvfb, launches an Extension Development Host carrying this
// checkout's extension (`clients/vscode`) and debug server, drives it over the
// DevTools protocol, writes PNG frames under `assets/frames/<story>/`, and stops
// everything again. Frames become GIFs with:
//
//   python3 scripts/assemble-gif.py assets/frames/yang assets/demos/yang.gif
//
// Nothing here is needed to build or ship the extension: `scripts/package.json`
// exists so the recorder can borrow playwright-core, which talks the DevTools
// protocol to the window — the same mechanism a debugger uses, so typing and
// hovering go through the real UI.
//
// The buffer of every demo file is rewritten from the copy on disk before each
// story, so a previous run leaves no state behind, and no file is ever saved.
import { spawn, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const frames_dir = join(root, "assets", "frames");
const scratch = join(root, "scratch", "demo");
const demos = join(scratch, "examples");
const user_data = join(scratch, "user-data");
const extensions = join(scratch, "extensions");
// VS Code keeps its single-instance socket in XDG_RUNTIME_DIR, which the host
// session owns; give this window a private one inside the checkout.
const runtime = join(scratch, "runtime");
const server = join(root, "target", "debug", "netconf-language-server");
const extension = join(root, "clients", "vscode");
const display = process.env.DEMO_DISPLAY ?? ":126";
const port = Number(process.env.DEMO_PORT ?? 9355);
// Launch Electron directly: the `code` shell wrapper hands a start request to
// any running window in the user's session, which would record that window
// instead of a fresh one.
const launcher = process.env.DEMO_CODE_LAUNCHER ?? "/usr/share/code/code";
const width = 1440;
const height = 900;

// The Problems view is toggled on once per session, by the first story.
let problems_shown = false;

// A window with no interruptions in it: no welcome tab, no trust dialog, no
// minimap, no update prompt, and a solid caret so frames do not catch it
// mid-blink.
const SETTINGS = {
    "workbench.startupEditor": "none",
    "workbench.secondarySideBar.defaultVisibility": "hidden",
    "chat.disableAIFeatures": true,
    "chat.commandCenter.enabled": false,
    "editor.autoIndent": "none",
    "editor.formatOnPaste": false,
    "editor.autoClosingBrackets": "never",
    "editor.autoClosingQuotes": "never",
    "editor.autoClosingOvertype": "never",
    "editor.autoSurround": "never",
    "workbench.welcomePage.walkthroughs.openOnInstall": false,
    "workbench.tips.enabled": false,
    "workbench.colorTheme": "Default Dark Modern",
    "window.restoreWindows": "none",
    "window.titleBarStyle": "custom",
    "security.workspaceTrust.enabled": false,
    "telemetry.telemetryLevel": "off",
    "update.mode": "none",
    "extensions.ignoreRecommendations": true,
    "editor.minimap.enabled": false,
    "editor.stickyScroll.enabled": false,
    "editor.cursorBlinking": "solid",
    "editor.hover.delay": 100,
    "editor.occurrencesHighlight": "off",
    "editor.selectionHighlight": false,
    "editor.renderLineHighlight": "none",
    "breadcrumbs.enabled": false,
    "explorer.compactFolders": false,
    "git.enabled": false,
    "git.decorations.enabled": false,
    "workbench.activityBar.location": "default",
};

const stories = {
    yang: record_yang,
    xml: record_xml,
    json: record_json,
    xcomp: record_xcomp,
    jcomp: record_jcomp,
};

const wanted = process.argv.slice(2);
const selected = wanted.length > 0 ? wanted : Object.keys(stories);
for (const name of selected) {
    if (!(name in stories)) {
        console.error(`unknown story ${name}; pick from ${Object.keys(stories).join(", ")}`);
        process.exit(2);
    }
}
if (!existsSync(server)) {
    console.error(`no server at ${server}; build it with \`cargo build\` first`);
    process.exit(2);
}
if (!existsSync(join(extension, "dist", "extension.js"))) {
    console.error(`no ${join(extension, "dist", "extension.js")}; run \`npm run package\` in clients/vscode`);
    process.exit(2);
}

// The demo workspace is a copy of `examples/`, named `examples` so the window
// title and Explorer show what the README talks about.
rmSync(demos, { recursive: true, force: true });
rmSync(user_data, { recursive: true, force: true });
rmSync(runtime, { recursive: true, force: true });
mkdirSync(runtime, { recursive: true, mode: 0o700 });
mkdirSync(demos, { recursive: true });
for (const entry of readdirSync(join(root, "examples"))) {
    cpSync(join(root, "examples", entry), join(demos, entry));
}
mkdirSync(join(user_data, "User"), { recursive: true });
writeFileSync(join(user_data, "User", "settings.json"), `${JSON.stringify(SETTINGS, null, 4)}\n`);
rmSync(extensions, { recursive: true, force: true });

const fixtures = {};
for (const name of readdirSync(join(root, "examples"))) {
    fixtures[name] = readFileSync(join(root, "examples", name), "utf8");
}

// Xvfb wants /tmp/.X11-unix; a container gives it a fresh, writable /tmp. GLX
// goes off because the virtual display has no GL driver worth loading.
mkdirSync("/tmp/.X11-unix", { recursive: true });
const xvfb = spawn(
    "Xvfb",
    [display, "-screen", "0", "1600x1000x24", "-nolisten", "tcp", "-extension", "GLX"],
    { stdio: "ignore" },
);
const vscode = spawn(
    launcher,
    [
        "--no-sandbox",
        "--disable-gpu",
        "--disable-workspace-trust",
        "--skip-welcome",
        "--skip-release-notes",
        "--ozone-platform=x11",
        `--extensions-dir=${extensions}`,
        `--user-data-dir=${user_data}`,
        `--extensionDevelopmentPath=${extension}`,
        `--remote-debugging-port=${port}`,
        demos,
    ],
    {
        env: {
            ...process.env,
            DISPLAY: display,
            XDG_RUNTIME_DIR: runtime,
            __DEBUG_LSP_SERVER: server,
        },
        stdio: ["ignore", "ignore", "pipe"],
    },
);

let failure;
try {
    const browser = await connect();
    const page = await workbench(browser);
    await resize(page);
    for (const name of selected) {
        rmSync(join(frames_dir, name), { recursive: true, force: true });
        mkdirSync(join(frames_dir, name), { recursive: true });
        console.log(`recording ${name}`);
        await stories[name](page, (label) => shot(page, name, label));
    }
    await browser.close();
} catch (error) {
    failure = error;
    console.error(error);
} finally {
    vscode.kill("SIGTERM");
    xvfb.kill("SIGTERM");
    spawnSync("pkill", ["-f", `user-data-dir=${user_data}`], { stdio: "ignore" });
}
if (failure) {
    process.exit(1);
}

async function connect() {
    const deadline = Date.now() + 60_000;
    for (;;) {
        try {
            const response = await fetch(`http://127.0.0.1:${port}/json/version`);
            if (response.ok) {
                break;
            }
        } catch {
            // Not listening yet.
        }
        if (Date.now() > deadline) {
            throw new Error("VS Code never opened its DevTools port");
        }
        await sleep(500);
    }
    return chromium.connectOverCDP(`http://127.0.0.1:${port}`);
}

async function workbench(browser) {
    const deadline = Date.now() + 60_000;
    for (;;) {
        for (const context of browser.contexts()) {
            for (const page of context.pages()) {
                if (page.url().includes("workbench.html")) {
                    await page.setDefaultTimeout(30_000);
                    await page.waitForSelector(".monaco-workbench", { timeout: 60_000 });
                    // Never drive a window that belongs to the user's session:
                    // this one has to be the folder copied from `examples/`.
                    await wait_for_title(page, "examples");
                    // The server scans the workspace before answering
                    // `initialize`; the client shows a progress notification
                    // until it does.
                    await wait_idle(page);
                    await page.keyboard.press("Escape");
                    return page;
                }
            }
        }
        if (Date.now() > deadline) {
            throw new Error("no workbench page appeared");
        }
        await sleep(500);
    }
}

async function resize(page) {
    // Electron implements no Browser-domain window API and the virtual display
    // has no window manager, so the renderer resizes its own window.
    await page.evaluate(([width, height]) => window.resizeTo(width, height), [width, height]);
    await sleep(1000);
    console.log(`  window ${JSON.stringify(await page.evaluate(() => [window.innerWidth, window.innerHeight]))}`);
}

/// Wait until the window title names the demo workspace.
async function wait_for_title(page, needle) {
    const deadline = Date.now() + 60_000;
    for (;;) {
        const title = await page.title();
        if (title.includes(needle)) {
            return;
        }
        if (Date.now() > deadline) {
            throw new Error(`workbench page is not the demo window: ${JSON.stringify(title)}`);
        }
        await sleep(500);
    }
}

/// Wait until the workspace-scan progress notification has come and gone.
///
/// The scan can finish before the notification is even drawn, so wait a bounded
/// time for it to appear first; a missing notification is not a failure.
async function wait_idle(page) {
    const started = Date.now();
    while (Date.now() - started < 15_000) {
        if ((await notification_count(page)) > 0) {
            break;
        }
        await sleep(200);
    }
    while ((await notification_count(page)) > 0 && Date.now() - started < 180_000) {
        await sleep(300);
    }
    await sleep(500);
}

async function notification_count(page) {
    return page
        .locator(".notifications-toasts .notification-list-item")
        .count()
        .catch(() => 0);
}

/// Open every demo file once, leaving the requested one active: the Problems
/// view then lists the whole workspace, the way the README shows it.
async function open_workspace_files(page, active) {
    for (const name of Object.keys(fixtures)) {
        if (name === active) {
            continue;
        }
        await open_file(page, name);
    }
    await open_file(page, active);
    // Every story shows the workspace's diagnostics. The toggle has to happen
    // once per session, and only once an editor exists to focus afterwards.
    if (!problems_shown) {
        problems_shown = true;
        await show_problems(page);
    }
}



async function open_file(page, name) {
    await page.keyboard.press("Control+P");
    await page.locator(".quick-input-widget").waitFor({ state: "visible" });
    await page.keyboard.insertText(name);
    await sleep(400);
    await page.keyboard.press("Enter");
    await page.waitForSelector(".monaco-editor .view-lines", { timeout: 30_000 });
    await sleep(1200);
}

/// Replace the buffer with the copy from `examples/`, so frames never depend on
/// what an earlier run typed.
async function reset_buffer(page, name) {
    await page.keyboard.press("Control+A");
    await page.keyboard.insertText(fixtures[name]);
    await sleep(2000);
    await page.keyboard.press("Control+Home");
    await sleep(300);
}

async function goto(page, line, column) {
    await page.keyboard.press("Control+G");
    await page.locator(".quick-input-widget").waitFor({ state: "visible" });
    await page.keyboard.insertText(`${line + 1}:${column + 1}`);
    await page.keyboard.press("Enter");
    await sleep(400);
}

/// Put the mouse on the character the caret sits on, which is what makes the
/// hover open.
async function hover_at_caret(page) {
    const caret = await page.locator(".cursors-layer .cursor").first().boundingBox();
    if (!caret) {
        throw new Error("no caret to hover");
    }
    await page.mouse.move(caret.x + 1, caret.y + caret.height / 2);
    await page.locator(".monaco-hover").first().waitFor({ state: "visible" });
    // The tooltip fades in; a frame caught mid-fade shows the code through it.
    await sleep(1200);
}

async function shot(page, story, label) {
    // The pointer goes to the title bar so no tooltip sits in the frame; hover
    // frames are captured before this runs.
    await page.mouse.move(4, 4);
    await sleep(500);
    await page.screenshot({ path: join(frames_dir, story, `${label}.png`), animations: "disabled" });
    console.log(`  ${story}/${label}.png`);
}

/// Open the Problems view so the diagnostics the README talks about are in the
/// frame, then hand focus back to the editor.
async function show_problems(page) {
    await page.keyboard.press("Control+Shift+M");
    await sleep(1200);
    const editor = page.locator(".editor-instance .monaco-editor").first();
    const box = await editor.boundingBox();
    await page.mouse.click(box.x + box.width - 200, box.y + 200);
    await sleep(600);
}

/// 0-based line and UTF-16 column of `needle` in `text`.
function position_of(text, needle) {
    const byte = text.indexOf(needle);
    if (byte < 0) {
        throw new Error(`no ${JSON.stringify(needle)} in the fixture`);
    }
    const before = text.slice(0, byte);
    const line = before.split("\n").length - 1;
    const column = before.slice(before.lastIndexOf("\n") + 1).length;
    return { line, column };
}

function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

// The stories. Each one opens a file from `examples/`, rewrites its buffer,
// then walks the feature the README paragraph describes; the order of the
// frames is the order of the story.

async function record_yang(page, shot) {
    await open_workspace_files(page, "example-demo.yang");
    await reset_buffer(page, "example-demo.yang");
    await shot("01-file");

    const path = position_of(fixtures["example-demo.yang"], "if:interfaces/if:interface");
    await goto(page, path.line, path.column + 3);
    await hover_at_caret(page);
    await page.screenshot({ path: join(frames_dir, "yang", "02-hover.png") });
    console.log("  yang/02-hover.png");
}

async function record_xml(page, shot) {
    await open_workspace_files(page, "example-netconf-config.xml");
    await reset_buffer(page, "example-netconf-config.xml");
    await shot("01-file");

    const element = position_of(fixtures["example-netconf-config.xml"], "admin-down xmlns");
    await goto(page, element.line, element.column + 2);
    await hover_at_caret(page);
    await page.screenshot({ path: join(frames_dir, "xml", "02-hover.png") });
    console.log("  xml/02-hover.png");
}

async function record_json(page, shot) {
    await open_workspace_files(page, "example-netconf-data.json");
    await reset_buffer(page, "example-netconf-data.json");
    await shot("01-file");

    const member = position_of(fixtures["example-netconf-data.json"], "\"speed\": 9999999");
    await goto(page, member.line, member.column + 2);
    await hover_at_caret(page);
    await page.screenshot({ path: join(frames_dir, "json", "02-hover.png") });
    console.log("  json/02-hover.png");
}

async function record_xcomp(page, shot) {
    await open_workspace_files(page, "example-netconf-config.xml");
    await reset_buffer(page, "example-netconf-config.xml");

    // A new line inside the first `<interface>`: typing `<` asks the server
    // which children the schema allows there.
    const closing = position_of(fixtures["example-netconf-config.xml"], "</interface>");
    await goto(page, closing.line, 0);
    await page.keyboard.press("Home");
    // A fresh child line, indented like its siblings, so the frame shows an
    // element position inside `<interface>` rather than the closing tag.
    await page.keyboard.press("Enter");
    await page.keyboard.insertText("      <");
    await page.locator(".suggest-widget.visible").waitFor({ state: "visible" });
    await sleep(900);
    await shot("01-suggest");

    await page.keyboard.insertText("ad");
    await sleep(900);
    await shot("02-filter");

    await page.keyboard.press("Enter");
    await sleep(1200);
    // The accepted element lands where the caret was; one more line break puts
    // the parent's closing tag under it, where the schema's own indent expects
    // it.
    await page.keyboard.press("Enter");
    await page.keyboard.insertText("    ");
    await sleep(600);
    await shot("03-accepted");
    await reset_buffer(page, "example-netconf-config.xml");
}

async function record_jcomp(page, shot) {
    await open_workspace_files(page, "example-netconf-data.json");
    await reset_buffer(page, "example-netconf-data.json");

    // After a member, a comma opens the next member slot (RFC 7951): the server
    // offers the sibling names.
    const member = position_of(fixtures["example-netconf-data.json"], "\"example-demo:admin-down\": false");
    const end = member.column + "\"example-demo:admin-down\": false".length;
    await goto(page, member.line, end);
    await page.keyboard.press("End");
    await page.keyboard.insertText(",");
    await page.locator(".suggest-widget.visible").waitFor({ state: "visible" });
    await sleep(900);
    await shot("01-suggest");

    // A filter would have to be typed inside the member name, where the server
    // stops offering siblings; walk the list instead.
    await page.keyboard.press("ArrowDown");
    await sleep(900);
    await shot("02-select");

    await page.keyboard.press("Enter");
    await sleep(1400);
    await shot("03-accepted");
    await reset_buffer(page, "example-netconf-data.json");
}
