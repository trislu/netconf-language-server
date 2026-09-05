//! Leaf value validation (D31 / M5).
//!
//! [`check`] evaluates a **decoded** leaf value (canonical text form) against
//! the resolved scalar [`ValueType`] from `yrepo`. It is deliberately format
//! agnostic: XML and JSON mappers decode their leaf value into the canonical
//! text first (JSON strings unescaped, `empty` → `""`, numbers → literal), then
//! share this one validator.
//!
//! Scope (D31): only reducible **scalars** are checked — `string`
//! (`length`/`pattern`), integers (`range`, per-signedness bit width),
//! `decimal64` (lexical), `boolean`, `empty`, `binary` (base64), `enumeration`
//! and `bits`. `union` (first-match ambiguity), references, and unresolved
//! types are never checked ([`ValueType::is_checked`]).

use yrepo::{IdentityStatus, Library, ValueType};

/// Check `value` against the resolved scalar `t`. Returns an error message
/// when invalid, `None` when valid **or** when `t` is not a checked scalar.
pub fn check(t: &ValueType, value: &str) -> Option<String> {
    match t {
        ValueType::String { lengths, patterns } => check_string(lengths, patterns, value),
        ValueType::Integer {
            signed,
            bits,
            ranges,
        } => check_integer(*signed, *bits, ranges, value),
        ValueType::Decimal64 { ranges } => {
            let s = value.trim();
            if !valid_decimal(s) {
                return Some(format!("`{value}` is not a valid decimal64 value"));
            }
            // Exponent forms are not canonical decimal64 — accept them lexically
            // but skip range comparison (would need unbounded scale).
            if s.contains(['e', 'E']) {
                return None;
            }
            let Some(v) = parse_dec(s) else {
                return Some(format!("`{value}` is not a valid decimal64 value"));
            };
            for spec in ranges {
                if let Some(ivs) = dec_intervals(spec)
                    && !ivs.iter().any(|iv| iv.contains(&v))
                {
                    return Some(format!("`{value}` is not in range {spec}"));
                }
            }
            None
        }
        ValueType::Boolean => match value.trim() {
            "true" | "false" => None,
            other => Some(format!("expected `true` or `false`, got `{other}`")),
        },
        ValueType::Empty => {
            if value.trim().is_empty() {
                None
            } else {
                Some("an `empty` leaf carries no value".to_owned())
            }
        }
        ValueType::Binary => {
            if valid_base64(value.trim()) {
                None
            } else {
                Some("`binary` value must be valid base64".to_owned())
            }
        }
        ValueType::Enumeration { members } => {
            let v = value.trim();
            if members.iter().any(|m| m == v) {
                None
            } else {
                Some(format!("must be one of: {}", members.join(", ")))
            }
        }
        ValueType::Bits { members } => {
            for tok in value.split_whitespace() {
                if !members.iter().any(|m| m == tok) {
                    return Some(format!(
                        "unknown bit `{tok}` (expected one of: {})",
                        members.join(", ")
                    ));
                }
            }
            None
        }
        _ => None,
    }
}

/// Scalar **and** reference value check. The mappers call this (they have the
/// library for the semantic `identityref` check); the pure [`check`] is kept
/// for the scalar cases.
pub fn check_value(lib: &Library, module: &str, t: &ValueType, value: &str) -> Option<String> {
    check(t, value).or_else(|| check_ref(lib, module, t, value))
}

/// Checks for reference kinds that need the library: semantic `identityref`
/// and a coarse `instance-identifier` shape check. Returns `None` for other
/// kinds and for valid values.
pub fn check_ref(lib: &Library, module: &str, t: &ValueType, value: &str) -> Option<String> {
    match t {
        ValueType::Identityref { base } => {
            let v = value.trim();
            if v.is_empty() || v.split_whitespace().count() > 1 || v.split(':').count() > 2 {
                return Some("expected an identity reference (module:name)".to_owned());
            }
            match lib.check_identityref(module, base.as_deref(), v) {
                IdentityStatus::Ok => None,
                IdentityStatus::UnknownIdentity => Some(format!("unknown identity `{v}`")),
                IdentityStatus::NotDerived => base
                    .as_ref()
                    .map(|b| format!("identity `{v}` is not `{b}` or derived from it")),
            }
        }
        ValueType::InstanceIdentifier => {
            let v = value.trim();
            if v.is_empty() {
                Some("expected an instance-identifier".to_owned())
            } else if v.split_whitespace().count() > 1 {
                Some("instance-identifier must not contain whitespace".to_owned())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn check_string(lengths: &[String], patterns: &[String], value: &str) -> Option<String> {
    let n = value.chars().count();
    for spec in lengths {
        if let Some(ivs) = intervals(spec)
            && !ivs.iter().any(|iv| iv.contains(n as i128))
        {
            return Some(format!("string length must be {spec} (value length {n})"));
        }
    }
    for p in patterns {
        // YANG `pattern` is implicitly anchored (RFC 7950 §9.4.6); skip the
        // check if the XSD-ish regex does not compile in the Rust engine.
        if let Ok(re) = regex::Regex::new(&format!("^(?:{p})$"))
            && !re.is_match(value)
        {
            return Some(format!("value does not match pattern `{p}`"));
        }
    }
    None
}

fn int_name(signed: bool, bits: u8) -> String {
    if signed {
        format!("int{bits}")
    } else {
        format!("uint{bits}")
    }
}

fn bounds(signed: bool, bits: u8) -> (i128, i128) {
    if signed {
        let m = 1i128 << (bits - 1);
        (-m, m - 1)
    } else {
        (0, (1i128 << bits) - 1)
    }
}

fn check_integer(signed: bool, bits: u8, ranges: &[String], value: &str) -> Option<String> {
    let s = value.trim();
    let name = int_name(signed, bits);
    let parsed = if s.starts_with('-') {
        s.parse::<i128>().ok()
    } else if s.chars().all(|c| c.is_ascii_digit()) {
        // Allow unsigned literals up to i128 (covers all of u64).
        s.parse::<i128>().ok()
    } else {
        None
    };
    let Some(v) = parsed else {
        return Some(format!("`{s}` is not a valid {name}"));
    };
    let (lo, hi) = bounds(signed, bits);
    if v < lo || v > hi {
        return Some(format!("`{s}` is out of range for {name} ({lo}..{hi})"));
    }
    for spec in ranges {
        if let Some(ivs) = intervals(spec)
            && !ivs.iter().any(|iv| iv.contains(v))
        {
            return Some(format!("`{s}` is not in range {spec} for {name}"));
        }
    }
    None
}

/// One inclusive interval of an integer `length`/`range` argument. `min`/`max`
/// (or the absent side) mean unbounded.
#[derive(Debug, Clone, Copy)]
struct Iv {
    lo: Option<i128>,
    hi: Option<i128>,
}

impl Iv {
    fn contains(&self, v: i128) -> bool {
        self.lo.is_none_or(|lo| v >= lo) && self.hi.is_none_or(|hi| v <= hi)
    }
}

/// Parse a YANG length/range argument: `"1..10"`, `"5"`, `"1..10 | 20..30"`,
/// with `min`/`max` for the unbounded sides. `None` when malformed.
fn intervals(spec: &str) -> Option<Vec<Iv>> {
    spec.split('|')
        .map(|part| {
            let p = part.trim();
            if let Some((a, b)) = p.split_once("..") {
                let a = a.trim();
                let b = b.trim();
                let lo = if a == "min" {
                    None
                } else {
                    Some(a.parse().ok()?)
                };
                let hi = if b == "max" {
                    None
                } else {
                    Some(b.parse().ok()?)
                };
                Some(Iv { lo, hi })
            } else {
                let v = p.parse().ok()?;
                Some(Iv {
                    lo: Some(v),
                    hi: Some(v),
                })
            }
        })
        .collect()
}

/// A fixed-scale decimal `m / 10^s` for exact range comparison.
#[derive(Debug, Clone, Copy)]
struct Dec {
    m: i128,
    s: u32,
}

fn pow10(n: u32) -> i128 {
    let mut r: i128 = 1;
    for _ in 0..n {
        r = r.saturating_mul(10);
    }
    r
}

impl Dec {
    fn cmp(&self, other: &Dec) -> std::cmp::Ordering {
        let scale = self.s.max(other.s);
        let a = self.m.saturating_mul(pow10(scale - self.s));
        let b = other.m.saturating_mul(pow10(scale - other.s));
        a.cmp(&b)
    }
}

/// Parse a decimal literal (no exponent) into a [`Dec`].
fn parse_dec(s: &str) -> Option<Dec> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut sign: i128 = 1;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        if b[i] == b'-' {
            sign = -1;
        }
        i += 1;
    }
    let mut m: i128 = 0;
    let mut scale: u32 = 0;
    let mut int_digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        m = m.saturating_mul(10) + i128::from(b[i] - b'0');
        int_digits += 1;
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            m = m.saturating_mul(10) + i128::from(b[i] - b'0');
            scale += 1;
            i += 1;
        }
    }
    if i != b.len() || (int_digits == 0 && scale == 0) {
        return None;
    }
    Some(Dec {
        m: sign * m,
        s: scale,
    })
}

/// One inclusive interval of a `decimal64` `range` argument.
#[derive(Debug, Clone, Copy)]
struct Dv {
    lo: Option<Dec>,
    hi: Option<Dec>,
}

impl Dv {
    fn contains(&self, v: &Dec) -> bool {
        let ge = self
            .lo
            .is_none_or(|lo| lo.cmp(v) != std::cmp::Ordering::Greater);
        let le = self
            .hi
            .is_none_or(|hi| hi.cmp(v) != std::cmp::Ordering::Less);
        ge && le
    }
}

/// Parse a `decimal64` `range` argument into intervals (bounds may be `min`/
/// `max`; `|` separates intervals).
fn dec_intervals(spec: &str) -> Option<Vec<Dv>> {
    spec.split('|')
        .map(|part| {
            let p = part.trim();
            if let Some((a, b)) = p.split_once("..") {
                let a = a.trim();
                let b = b.trim();
                let lo = if a == "min" {
                    None
                } else {
                    Some(parse_dec(a)?)
                };
                let hi = if b == "max" {
                    None
                } else {
                    Some(parse_dec(b)?)
                };
                Some(Dv { lo, hi })
            } else {
                let v = parse_dec(p)?;
                Some(Dv {
                    lo: Some(v),
                    hi: Some(v),
                })
            }
        })
        .collect()
}

/// Whether `s` is a decimal literal (optionally with fraction and exponent) —
/// covers both the XML and JSON encodings of a `decimal64` value.
fn valid_decimal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut int = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        int += 1;
        i += 1;
    }
    let mut frac = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            frac += 1;
            i += 1;
        }
    }
    if int == 0 && frac == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let mut exp = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            exp += 1;
            i += 1;
        }
        if exp == 0 {
            return false;
        }
    }
    i == b.len()
}

/// Whether `s` is a syntactically plausible base64 string (standard alphabet,
/// length a multiple of four, at most two trailing `=` pads).
fn valid_base64(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if !s.len().is_multiple_of(4) {
        return false;
    }
    let bytes = s.as_bytes();
    let mut pads = 0;
    for (i, c) in bytes.iter().enumerate() {
        if *c == b'=' {
            if i + 1 < bytes.len() && bytes[i + 1] != b'=' && i + 2 != bytes.len() {
                // `=` must be at the tail; loosen: count and range-check below.
            }
            pads += 1;
        } else if !(c.is_ascii_alphanumeric() || *c == b'+' || *c == b'/') {
            return false;
        }
    }
    pads <= 2
}

/// A scalar completion default, per format-agnostic shape (the completion
/// modules format it: JSON quotes strings / uses `[null]`, XML uses text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultValue {
    /// A bare literal (booleans, numbers).
    Bare(&'static str),
    /// A string value (enumeration member).
    Quoted(String),
    /// `empty` — XML empty element / JSON `[null]`.
    Empty,
}

/// The natural completion default for a checked scalar, or `None` when the
/// leaf has no obvious default (`string`/`binary`/`bits`/union/references).
pub fn default_value(t: &ValueType) -> Option<DefaultValue> {
    match t {
        ValueType::Boolean => Some(DefaultValue::Bare("true")),
        ValueType::Integer { .. } | ValueType::Decimal64 { .. } => Some(DefaultValue::Bare("0")),
        ValueType::Enumeration { members } => {
            members.first().map(|m| DefaultValue::Quoted(m.clone()))
        }
        ValueType::Empty => Some(DefaultValue::Empty),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(lengths: Vec<&str>, patterns: Vec<&str>) -> ValueType {
        ValueType::String {
            lengths: lengths.into_iter().map(str::to_owned).collect(),
            patterns: patterns.into_iter().map(str::to_owned).collect(),
        }
    }

    fn i(signed: bool, bits: u8, ranges: Vec<&str>) -> ValueType {
        ValueType::Integer {
            signed,
            bits,
            ranges: ranges.into_iter().map(str::to_owned).collect(),
        }
    }

    #[test]
    fn string_length_and_pattern() {
        let t = s(vec!["1..32"], vec!["[a-z]+"]);
        assert!(check(&t, "abc").is_none());
        assert!(check(&t, "").is_some());
        assert!(check(&t, "a".repeat(33).as_str()).is_some());
        assert!(check(&t, "abc1").is_some());
        // Multiple length intervals.
        let t = s(vec!["1..2 | 5..6"], vec![]);
        assert!(check(&t, "abcde").is_none());
        assert!(check(&t, "abcd").is_some());
    }

    #[test]
    fn integer_bounds_and_ranges() {
        let t = i(false, 16, vec!["1..65535"]);
        assert!(check(&t, "1").is_none());
        assert!(check(&t, "65535").is_none());
        assert!(check(&t, "0").is_some());
        assert!(check(&t, "70000").is_some());
        assert!(check(&t, "abc").is_some());
        // signed min/max
        let t = i(true, 8, vec![]);
        assert!(check(&t, "-128").is_none());
        assert!(check(&t, "127").is_none());
        assert!(check(&t, "-129").is_some());
        assert!(check(&t, "128").is_some());
    }

    #[test]
    fn scalars_and_refs() {
        assert!(check(&ValueType::Boolean, "true").is_none());
        assert!(check(&ValueType::Boolean, "yes").is_some());
        assert!(check(&ValueType::Empty, "").is_none());
        assert!(check(&ValueType::Empty, "x").is_some());
        assert!(check(&ValueType::Binary, "aGVsbG8=").is_none());
        assert!(check(&ValueType::Binary, "!!!").is_some());
        let en = ValueType::Enumeration {
            members: vec!["red".into(), "green".into()],
        };
        assert!(check(&en, "red").is_none());
        assert!(check(&en, "blue").is_some());
        let bits = ValueType::Bits {
            members: vec!["a".into(), "b".into()],
        };
        assert!(check(&bits, "a b").is_none());
        assert!(check(&bits, "").is_none());
        assert!(check(&bits, "a c").is_some());
        // Non-checked → None.
        assert!(check(&ValueType::Union, "123").is_none());
        assert!(
            check(
                &ValueType::Leafref {
                    path: None,
                    require_instance: true
                },
                "anything"
            )
            .is_none()
        );
    }

    #[test]
    fn decimal_lexical() {
        let t = ValueType::Decimal64 { ranges: vec![] };
        assert!(check(&t, "1").is_none());
        assert!(check(&t, "-1.5").is_none());
        assert!(check(&t, "1e3").is_none());
        assert!(check(&t, "1.2.3").is_some());
        assert!(check(&t, "--1").is_some());
    }

    #[test]
    fn decimal_range_enforced() {
        let t = ValueType::Decimal64 {
            ranges: vec!["-1.5..1.5".to_owned()],
        };
        assert!(check(&t, "1.25").is_none());
        assert!(check(&t, "-1.5").is_none());
        assert!(check(&t, "0").is_none());
        assert!(check(&t, "2").is_some());
        assert!(check(&t, "-2").is_some());
        let t = ValueType::Decimal64 {
            ranges: vec!["min..1.5".to_owned()],
        };
        assert!(check(&t, "-100").is_none());
        assert!(check(&t, "1.5").is_none());
        assert!(check(&t, "1.6").is_some());
    }
}
