//! NETCONF skeleton templates (M2, decisions D21: envelope hard-coded).
//!
//! Pure text generators for the small, fixed RFC 6241 message layer: `hello`,
//! a `<get-config>` RPC, an `<edit-config>` RPC, and a bare `<config>` payload
//! root. A `workspace/executeCommand` handler inserts them at the cursor.

/// `<hello>` with base capability (RFC 6241 §8.1).
pub const HELLO: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<hello xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
  <capabilities>
    <capability>urn:ietf:params:netconf:base:1.1</capability>
  </capabilities>
</hello>
"#;

/// `<rpc>` wrapping a `<get-config>` against `<running>` with a subtree filter.
pub const RPC_GET_CONFIG: &str = r#"<rpc message-id="101" xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
  <get-config>
    <source>
      <running/>
    </source>
    <filter type="subtree">
      <!-- data subtree here -->
    </filter>
  </get-config>
</rpc>
"#;

/// `<rpc>` wrapping an `<edit-config>` merge into `<running>`.
pub const RPC_EDIT_CONFIG: &str = r#"<rpc message-id="102" xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
  <edit-config>
    <target>
      <running/>
    </target>
    <default-operation>merge</default-operation>
    <config>
      <!-- module data here -->
    </config>
  </edit-config>
</rpc>
"#;

/// A bare `<config>` payload root (multiple modules' data nodes may be added).
pub const CONFIG_PAYLOAD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<config xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
  <!-- module data here -->
</config>
"#;

/// The named templates known to the insert command (exercised by tests).
#[cfg(test)]
pub const KINDS: &[&str] = &["hello", "get-config", "edit-config", "config"];

/// The skeleton text for `kind`, if known.
pub fn skeleton(kind: &str) -> Option<&'static str> {
    match kind {
        "hello" => Some(HELLO),
        "get-config" => Some(RPC_GET_CONFIG),
        "edit-config" => Some(RPC_EDIT_CONFIG),
        "config" => Some(CONFIG_PAYLOAD),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_skeleton_has_base_ns_and_capabilities() {
        let text = skeleton("hello").unwrap();
        assert!(text.contains("<hello"));
        assert!(text.contains("urn:ietf:params:xml:ns:netconf:base:1.0"));
        assert!(text.contains("<capability>"));
    }

    #[test]
    fn get_config_rpc_skeleton() {
        let text = skeleton("get-config").unwrap();
        assert!(text.contains("<rpc message-id="));
        assert!(text.contains("<get-config>"));
        assert!(text.contains("<source>"));
        assert!(text.contains("<running/>"));
        assert!(text.contains("<filter"));
        assert!(text.contains("</rpc>"));
    }

    #[test]
    fn edit_config_rpc_skeleton() {
        let text = skeleton("edit-config").unwrap();
        assert!(text.contains("<edit-config>"));
        assert!(text.contains("<default-operation>merge</default-operation>"));
        assert!(text.contains("<config>"));
    }

    #[test]
    fn config_payload_skeleton() {
        let text = skeleton("config").unwrap();
        assert!(text.contains("<config"));
        assert!(text.contains("urn:ietf:params:xml:ns:netconf:base:1.0"));
    }

    #[test]
    fn unknown_kind_is_none() {
        assert!(skeleton("nope").is_none());
        for k in KINDS {
            assert!(skeleton(k).is_some());
        }
    }
}
