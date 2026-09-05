//! Diagnostics (`textDocument/diagnostic`, pull model) + LS-side checks.
//!
//! yrepo reports most problems via `Outcome.diagnostics`; the only spec
//! scenario still implemented LS-side is **conflict prefix** (D17).

use ropey::Rope;
use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticSeverity, DocumentDiagnosticReport, DocumentDiagnosticReportResult,
    FullDocumentDiagnosticReport, NumberOrString, RelatedFullDocumentDiagnosticReport,
};
use yrepo::{DiagnosticCode, Statement, StatementKind};

use crate::convert;

pub(crate) fn severity(s: yrepo::Severity) -> DiagnosticSeverity {
    match s {
        yrepo::Severity::Error => DiagnosticSeverity::ERROR,
        yrepo::Severity::Warning => DiagnosticSeverity::WARNING,
        yrepo::Severity::Info => DiagnosticSeverity::INFORMATION,
        yrepo::Severity::Hint => DiagnosticSeverity::HINT,
    }
}

fn code_name(code: DiagnosticCode) -> &'static str {
    code.as_str()
}

fn to_lsp(rope: &Rope, d: &yrepo::Diagnostic) -> Option<Diagnostic> {
    let range = d.range.clone()?;
    Some(Diagnostic {
        range: convert::range_to_lsp(rope, range),
        severity: Some(severity(d.severity)),
        code: Some(NumberOrString::String(code_name(d.code).to_owned())),
        source: Some("yang".to_owned()),
        message: d.message.clone(),
        ..Default::default()
    })
}

/// Convert yrepo diagnostics for one document (matching `url`) into LSP ones.
pub(crate) fn convert(rope: &Rope, diags: &[yrepo::Diagnostic], url: &str) -> Vec<Diagnostic> {
    diags
        .iter()
        .filter(|d| d.url.as_deref() == Some(url))
        .filter_map(|d| to_lsp(rope, d))
        .collect()
}

/// LS-side check: two `import`s (or an `import` and the module's own `prefix`)
/// declaring the same prefix (RFC 7950 §7.1.4).
pub(crate) fn conflict_prefix(rope: &Rope, root: Option<&Statement>) -> Vec<Diagnostic> {
    let Some(root) = root else {
        return vec![];
    };
    let mut out = Vec::new();

    // First candidate: the module's own prefix (module/submodule header).
    let mut seen: Vec<(String, tower_lsp_server::ls_types::Range)> = Vec::new();
    if let Some(own) = root.find_one(StatementKind::Prefix)
        && let Some(arg) = &own.arg
    {
        seen.push((
            arg.name().to_owned(),
            convert::range_to_lsp(rope, arg.range.clone()),
        ));
    }

    for import in root.find(&[StatementKind::Import]) {
        let Some(prefix_stmt) = import.find_one(StatementKind::Prefix) else {
            continue;
        };
        let Some(arg) = &prefix_stmt.arg else {
            continue;
        };
        let name = arg.name().to_owned();
        let range = convert::range_to_lsp(rope, arg.range.clone());
        if let Some((_, first)) = seen.iter().find(|(n, _)| *n == name) {
            out.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                code: Some(NumberOrString::String("conflict_prefix".to_owned())),
                source: Some("yang".to_owned()),
                message: format!("conflicting prefix `{name}` (already declared at {first:?})"),
                ..Default::default()
            });
        } else {
            seen.push((name, range));
        }
    }
    out
}

/// Full report for the pull-diagnostics response.
pub(crate) fn report(result_id: String, items: Vec<Diagnostic>) -> DocumentDiagnosticReportResult {
    DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(
        RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                result_id: Some(result_id),
                items,
            },
        },
    ))
}
