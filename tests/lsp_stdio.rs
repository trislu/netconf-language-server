//! Drive the server over stdio, the way an editor does.
//!
//! The unit tests call handlers directly; they cannot cover `Content-Length`
//! framing, the initialize handshake, the pull-diagnostic request pair, the
//! `workspace/configuration` request the server sends back, or the bytes that
//! a client actually sees. This does.
//!
//! Every test builds its own workspace in a temporary directory, so the
//! assertions never depend on the demo files under `examples/`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// How long one message may take before the test fails instead of hanging.
const TIMEOUT: Duration = Duration::from_secs(60);

/// A temporary workspace: fixtures on disk, one directory per test.
struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("netconf-ls-stdio-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create workspace");
        let workspace = Self { root };
        workspace.write("lsp-test.yang", MODULE);
        workspace.write("lsp-broken.yang", BROKEN_MODULE);
        workspace.write("config.xml", CONFIG_XML);
        workspace.write("data.json", DATA_JSON);
        workspace.write("project.xml", NOT_NETCONF_XML);
        workspace
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, text).expect("write fixture");
        path
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The module every instance fixture is written against. Multi-line
/// statements matter here: a hover snippet is dedented against its own
/// continuation lines.
const MODULE: &str = r#"module lsp-test {
  yang-version 1.1;
  namespace "urn:lsp:test";
  prefix t;
  typedef bandwidth {
    type uint32 {
      range "1..100000";
    }
  }
  container top {
    leaf name {
      type string;
      description "A name.";
    }
    leaf enabled {
      type boolean;
    }
    leaf speed {
      type bandwidth;
    }
    list user {
      key "name";
      leaf name {
        type string;
      }
      leaf speed {
        type bandwidth;
      }
    }
    leaf probe {
      type ba;
    }
  }
}
"#;

/// A module with both resolutions broken at compile time.
const BROKEN_MODULE: &str = r#"module lsp-broken {
  namespace "urn:lsp:broken";
  prefix b;
  import missing {
    prefix m;
  }
  leaf x {
    type nosuchtype;
  }
}
"#;

/// A NETCONF `<config>` payload with two leaf values that fail validation.
const CONFIG_XML: &str = r#"<?xml version="1.0"?>
<config xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
  <top xmlns="urn:lsp:test">
    <name>eth0</name>
    <enabled>ffff</enabled>
    <speed>9999999</speed>
  </top>
</config>
"#;

/// The RFC 7951 rendering of the same data, with the same two bad values.
const DATA_JSON: &str = r#"{
  "lsp-test:top": {
    "name": "eth0",
    "enabled": "zsadf",
    "user": [
      {
        "name": "admin",
        "speed": 9999999
      }
    ]
  }
}
"#;

/// Well-formed XML belonging to no YANG module: not a NETCONF instance
/// document, so the server must stay dormant.
const NOT_NETCONF_XML: &str = r#"<?xml version="1.0"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>example</groupId>
</project>
"#;

/// One server session with a reader thread, so a missing reply fails the test
/// with a message instead of blocking it forever.
struct Session {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Value>,
    next_id: i64,
    /// Notifications and server requests seen while waiting for something
    /// else — what a message the test does not expect has to be judged from.
    seen: Vec<Value>,
    init: Value,
}

impl Session {
    /// Spawn the server with `root` as the workspace and complete the
    /// handshake, answering the `workspace/configuration` request the server
    /// sends from `initialized` (the session stalls without an answer).
    fn start(root: &Path) -> Self {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_netconf-language-server"))
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the language server");

        let stdin = child.stdin.take().expect("server stdin");
        let stdout = child.stdout.take().expect("server stdout");
        let (tx, messages) = channel();
        std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            while let Some(message) = read_message(&mut stdout) {
                if tx.send(message).is_err() {
                    return;
                }
            }
        });

        let mut session = Self {
            child,
            stdin,
            messages,
            next_id: 1,
            seen: Vec::new(),
            init: Value::Null,
        };
        let uri = file_uri(root);
        let init = session.request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": uri,
                "capabilities": {},
                "workspaceFolders": [{ "uri": uri, "name": "workspace" }],
            }),
        );
        session.init = init;
        session.notify("initialized", json!({}));
        session
    }

    fn notify(&mut self, method: &str, params: Value) {
        write_message(
            &mut self.stdin,
            &json!({ "jsonrpc": "2.0", "method": method, "params": params }),
        );
    }

    /// Send a request and return its `result`, answering anything the server
    /// asks on the way.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        write_message(
            &mut self.stdin,
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        );
        loop {
            let message = self.recv(method);
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                assert!(message.get("error").is_none(), "{method} failed: {message}");
                return message.get("result").cloned().unwrap_or(Value::Null);
            }
            self.handle_on_the_side(message);
        }
    }

    /// Wait for the next message, failing the test after [`TIMEOUT`].
    fn recv(&mut self, waiting_for: &str) -> Value {
        match self.messages.recv_timeout(TIMEOUT) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => {
                panic!("no message within {TIMEOUT:?} while waiting for {waiting_for}")
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!("server closed its stdout while waiting for {waiting_for}")
            }
        }
    }

    /// A server-to-client request needs an answer; a notification (or an
    /// unrelated response) is kept for later inspection.
    fn handle_on_the_side(&mut self, message: Value) {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            self.seen.push(message);
            return;
        };
        let Some(id) = message.get("id") else {
            self.seen.push(message);
            return;
        };
        // The only request this server makes. `{}` decodes to the defaults, so
        // a session that answers it behaves like a client with no settings.
        let result = if method == "workspace/configuration" {
            json!([{}])
        } else {
            Value::Null
        };
        write_message(
            &mut self.stdin,
            &json!({ "jsonrpc": "2.0", "id": id.clone(), "result": result }),
        );
    }

    fn did_open(&mut self, path: &Path) {
        let text = std::fs::read_to_string(path).expect("read fixture");
        let language = match path.extension().and_then(|e| e.to_str()) {
            Some("yang") => "yang",
            Some("xml") => "xml",
            Some("json") => "json",
            other => panic!("no language for extension {other:?}"),
        };
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": file_uri(path),
                    "languageId": language,
                    "version": 1,
                    "text": text,
                },
            }),
        );
    }

    /// Pull the diagnostics of an open document.
    fn diagnostics(&mut self, path: &Path) -> Vec<Value> {
        let result = self.request(
            "textDocument/diagnostic",
            json!({ "textDocument": { "uri": file_uri(path) } }),
        );
        result
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    fn hover(&mut self, path: &Path, (line, character): (u32, u32)) -> Value {
        self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": file_uri(path) },
                "position": { "line": line, "character": character },
            }),
        )
    }

    /// The markdown a hover carries.
    fn hover_markdown(&mut self, path: &Path, position: (u32, u32)) -> String {
        let hover = self.hover(path, position);
        hover["contents"]["value"]
            .as_str()
            .unwrap_or_else(|| panic!("hover has no markdown: {hover}"))
            .to_owned()
    }

    /// Everything that arrived while no request was pending, up to
    /// `grace` after the last message.
    fn drain(&mut self, grace: Duration) -> Vec<Value> {
        while let Ok(message) = self.messages.recv_timeout(grace) {
            self.handle_on_the_side(message);
        }
        std::mem::take(&mut self.seen)
    }

    /// Ask the server to stop, then wait for the process to go.
    fn shutdown(mut self) -> std::process::ExitStatus {
        let result = self.request("shutdown", Value::Null);
        assert!(result.is_null(), "shutdown returned {result}");
        self.notify("exit", Value::Null);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().expect("wait for the server") {
                std::mem::forget(self);
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "server did not exit after `exit`"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A test that failed half-way still has a live child.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A `file:` URI for `path`.
///
/// The harness cannot borrow the server's converter (this is a binary crate),
/// so it does the work itself: separators become `/`, a Windows drive letter
/// gains the leading slash a `file:` URI needs, and everything outside the
/// URI-safe set is percent-encoded — a temporary directory may live under a
/// user name with a space in it.
fn file_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from("file://");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                uri.push(byte as char);
            }
            other => uri.push_str(&format!("%{other:02X}")),
        }
    }
    uri
}

fn write_message(stdin: &mut ChildStdin, message: &Value) {
    let body = serde_json::to_vec(message).expect("encode message");
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write header");
    stdin.write_all(&body).expect("write body");
    stdin.flush().expect("flush");
}

fn read_message(stdout: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if stdout.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0_u8; length?];
    stdout.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

/// The position of `needle` in `text` as a zero-based line and UTF-16 column.
fn position_of(text: &str, needle: &str) -> (u32, u32) {
    let byte = text.find(needle).unwrap_or_else(|| panic!("no {needle:?}"));
    let before = &text[..byte];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let column = before[line_start..]
        .chars()
        .map(char::len_utf16)
        .sum::<usize>() as u32;
    (line, column)
}

fn codes(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item.get("code").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

/// The harness runs on Windows and macOS too, where the naive
/// `file://` + `path.display()` spelling is not a URI.
#[test]
fn file_uri_encodes_a_platform_path() {
    assert_eq!(
        file_uri(Path::new("/tmp/netconf demo/#1")),
        "file:///tmp/netconf%20demo/%231"
    );
    // A Windows path, passed as text so the assertion holds on every host.
    assert_eq!(
        file_uri(Path::new(r"C:\Users\John Doe\AppData\Local\Temp")),
        "file:///C:/Users/John%20Doe/AppData/Local/Temp"
    );
}

#[test]
fn initialize_announces_the_full_feature_set() {
    let workspace = Workspace::new("initialize");
    let session = Session::start(&workspace.root);
    let capabilities = &session.init["capabilities"];
    for feature in [
        "textDocumentSync",
        "semanticTokensProvider",
        "foldingRangeProvider",
        "documentFormattingProvider",
        "definitionProvider",
        "referencesProvider",
        "renameProvider",
        "hoverProvider",
        "completionProvider",
        "diagnosticProvider",
        "executeCommandProvider",
    ] {
        assert!(
            !capabilities[feature].is_null(),
            "initialize result has no {feature}: {capabilities}"
        );
    }
    let commands = &session.init["capabilities"]["executeCommandProvider"]["commands"];
    assert!(
        commands
            .as_array()
            .is_some_and(|list| list.iter().any(|c| c == "netconf/insertTemplate")),
        "template command missing: {commands}"
    );
    let status = session.shutdown();
    assert!(status.success(), "server exited with {status}");
}

#[test]
fn yang_diagnostics_name_both_unresolved_constructs() {
    let workspace = Workspace::new("yang-diagnostics");
    let path = workspace.path("lsp-broken.yang");
    let mut session = Session::start(&workspace.root);
    session.did_open(&path);
    let items = session.diagnostics(&path);
    let codes = codes(&items);
    assert!(
        codes.iter().any(|c| c == "unresolved-import"),
        "missing unresolved-import in {items:?}"
    );
    assert!(
        codes.iter().any(|c| c == "unresolved-typedef"),
        "missing unresolved-typedef in {items:?}"
    );
}

#[test]
fn yang_hover_resolves_a_typedef_chain() {
    let workspace = Workspace::new("yang-hover");
    let path = workspace.path("lsp-test.yang");
    let mut session = Session::start(&workspace.root);
    session.did_open(&path);
    let (line, column) = position_of(MODULE, "type bandwidth;");
    let markdown = session.hover_markdown(&path, (line, column + "type ".len() as u32));
    assert!(markdown.contains("**type** `bandwidth`"), "{markdown}");
    assert!(
        markdown.contains("typedef `bandwidth` (in module `lsp-test`)"),
        "{markdown}"
    );
    assert!(markdown.contains("→ built-in **`uint32`**"), "{markdown}");
}

#[test]
fn yang_completion_offers_typedefs_and_builtins() {
    let workspace = Workspace::new("yang-completion");
    let path = workspace.path("lsp-test.yang");
    let mut session = Session::start(&workspace.root);
    session.did_open(&path);
    let (line, column) = position_of(MODULE, "ba;");
    let result = session.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": file_uri(&path) },
            "position": { "line": line, "character": column },
        }),
    );
    let items = result
        .get("items")
        .or_else(|| result.as_array().map(|_| &result))
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("completion returned {result}"));
    let labels: Vec<&str> = items
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"bandwidth"), "labels: {labels:?}");
    assert!(labels.contains(&"uint32"), "labels: {labels:?}");
}

#[test]
fn xml_instance_document_validates_leaf_values() {
    let workspace = Workspace::new("xml-diagnostics");
    let path = workspace.path("config.xml");
    let mut session = Session::start(&workspace.root);
    session.did_open(&path);
    let items = session.diagnostics(&path);
    let messages: Vec<&str> = items
        .iter()
        .filter_map(|item| item["message"].as_str())
        .collect();
    assert!(
        codes(&items).iter().all(|c| c == "netconf_bad_value"),
        "unexpected diagnostic kinds in {items:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("ffff")),
        "no boolean value error in {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("9999999")),
        "no range error in {messages:?}"
    );
}

#[test]
fn json_instance_hover_and_validation_use_the_schema() {
    let workspace = Workspace::new("json-instance");
    let path = workspace.path("data.json");
    let mut session = Session::start(&workspace.root);
    session.did_open(&path);

    let markdown = session.hover_markdown(&path, position_of(DATA_JSON, "\"name\": \"eth0\""));
    assert!(markdown.contains("`leaf` **`name`**"), "{markdown}");
    assert!(markdown.contains("- type: `string`"), "{markdown}");

    let items = session.diagnostics(&path);
    let messages: Vec<&str> = items
        .iter()
        .filter_map(|item| item["message"].as_str())
        .collect();
    assert!(
        codes(&items).iter().all(|c| c == "json_bad_value"),
        "unexpected diagnostic kinds in {items:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("zsadf")),
        "no boolean value error in {messages:?}"
    );
}

/// The snippet a hover shows starts at the fence's left edge, which is what
/// makes a multi-line statement read as one block.
#[test]
fn xml_hover_snippet_is_dedented() {
    let workspace = Workspace::new("xml-hover");
    let path = workspace.path("config.xml");
    let mut session = Session::start(&workspace.root);
    session.did_open(&path);
    let markdown = session.hover_markdown(&path, position_of(CONFIG_XML, "name>eth0"));
    let expected = "```yang\nleaf name {\n  type string;\n  description \"A name.\";\n}\n```";
    assert!(
        markdown.contains(expected),
        "hover snippet is not dedented: {markdown}"
    );
}

/// A workspace whose XML belongs to no YANG module: the server must not report
/// anything, because there is nothing of its own to report.
#[test]
fn non_netconf_xml_is_dormant() {
    let workspace = Workspace::new("dormant");
    let path = workspace.path("project.xml");
    let mut session = Session::start(&workspace.root);
    session.did_open(&path);
    let items = session.diagnostics(&path);
    assert!(items.is_empty(), "dormant document got {items:?}");
    let seen = session.drain(Duration::from_millis(300));
    let errors: Vec<&Value> = seen
        .iter()
        .filter(|message| {
            // MessageType::ERROR is 1 in the protocol.
            message.get("method").and_then(Value::as_str) == Some("window/showMessage")
                && message["params"]["type"].as_i64() == Some(1)
        })
        .collect();
    assert!(errors.is_empty(), "the server reported errors: {errors:?}");
}
