//! Temporary probe: print yrepo diagnostics for a single .yang file with
//! context snippets around each error range.

fn main() {
    let file = std::env::args().nth(1).expect("file");
    let text = std::fs::read_to_string(&file).expect("read");
    let mut repo = yrepo::Repository::new();
    let url = file.clone();
    repo.upsert(url.clone(), text.clone());

    let out = repo.compile();
    println!("file: {file}  len={}", text.len());
    let mut diags: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.url.as_deref() == Some(url.as_str()))
        .collect();
    diags.sort_by_key(|d| d.range.as_ref().map(|r| r.start).unwrap_or(0));
    for d in &diags {
        let Some(r) = &d.range else { continue };
        let s = text.get(r.start.min(text.len())..(r.start + 160).min(text.len()));
        let ctx = s.unwrap_or_default().replace('\n', "\\n");
        println!(
            "\n[{}] {:?} range {}..{} :: {}\n  …{}…",
            d.code.as_str(),
            d.severity,
            r.start,
            r.end,
            d.message,
            ctx
        );
    }
    println!("\nTotal diagnostics for file: {}", diags.len());
}
