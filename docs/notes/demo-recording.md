# Demo captures

The READMEs show five short loops, one per feature, in the order the paragraphs
talk about them. The loops are committed as GIFs; the PNG frames they are built
from are not.

| file | what the loop shows | frames |
| --- | --- | --- |
| `assets/demos/yang.gif` | the module with its diagnostics, then the hover on an `augment` target | 2 |
| `assets/demos/xml.gif` | the `<config>` payload with its value errors, then the hover on an element | 2 |
| `assets/demos/json.gif` | the RFC 7951 payload, then the hover on a member | 2 |
| `assets/demos/xcomp.gif` | typing `<`, the child list, the filter, the accepted element | 3 |
| `assets/demos/jcomp.gif` | typing `,`, the sibling list, the accepted member | 3 |

The hover loops are the reason the `xml.gif`/`yang.gif` snippets look flush with
the code fence: the hover dedents a multi-line statement, and the recording is
what proves it.

## Recording

```bash
cargo build                                  # the server the dev host attaches to
npm --dir clients/vscode install
npm --dir clients/vscode run package          # dist/extension.js
npm --dir scripts install                     # playwright-core, for the DevTools protocol

node scripts/record-vscode.mjs                # every story
node scripts/record-vscode.mjs xml jcomp      # only these

python3 scripts/assemble-gif.py assets/frames/yang assets/demos/yang.gif --fps 1
python3 scripts/assemble-gif.py assets/frames/xcomp assets/demos/xcomp.gif --fps 1.5
```

`record-vscode.mjs` starts Xvfb, launches an Extension Development Host with a
private user-data dir and a private `XDG_RUNTIME_DIR`, drives it over the
DevTools protocol, writes `assets/frames/<story>/`, and stops everything again.
It needs `Xvfb`, the `code` install's Electron binary (default
`/usr/share/code/code`, `DEMO_CODE_LAUNCHER` to override), and Pillow for the
assembly step. `DEMO_DISPLAY` and `DEMO_PORT` move the virtual display and the
DevTools port when the defaults (`:126`, `9355`) are taken. Nothing in the
scripts is a build dependency of the extension.

## Choices that keep a recording reproducible

- **The workspace is a copy.** `examples/` is copied to
  `scratch/demo/examples/` for every run, so the window title and Explorer both
  read `examples` and no demo can dirty the real files.
- **The buffer is rewritten from disk.** Each story starts with `Ctrl+A` and an
  insert of the file's own text, so a previous run leaves nothing behind. No
  file is ever saved.
- **Auto-indent and auto-close are off.** `keyboard.insertText` is typing as far
  as the editor is concerned: with auto-indent on, every newline stacks another
  level of indentation, and with auto-close on, a trailing `}` gains a twin —
  which shows up as a bogus diagnostic in the frame.
- **The Problems view is opened once**, by the first story, so all five loops
  share the same furniture; pressing the toggle twice would close it again.
- **Positions come from the text.** Every hover/caret target is looked up in the
  fixture (`position_of`) instead of being a hard-coded line, and the caret goes
  there through *Go to Line/Column*, so a fixture edit cannot silently move the
  demo somewhere else.
- **Hover frames wait out the fade.** A frame captured during the tooltip's
  fade-in shows the code through it.
- **Non-hover frames park the pointer** in the title bar, so no stray tooltip
  sits in a still frame.
- **The window is checked.** The recorder refuses to drive a page whose title
  does not name the demo workspace, which is what stops it from recording the
  user's own window if the launch went wrong.
