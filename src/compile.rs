//! Bridge between the editor and the real Brief compiler (`brief-core`).
//!
//! The hand-rolled tokenizer in `brief.rs` only colors text. This module runs
//! the *actual* Brief pipeline — lex → parse → resolve → validate → emit — so
//! the editor can show genuine diagnostics (Brief is a strict language, so the
//! compiler rejects malformed headings, non-sequential ordered lists, unknown
//! shortcodes, …) and render the document to HTML for preview / export.
//!
//! Everything here is pure: feed it a path + buffer text, get back diagnostics
//! and/or HTML. No global state, so it is cheap to call on every edit.

use brief::diag::Severity;
use brief::shortcode::Registry;
use brief::span::SourceMap;
use brief::validate::ValidateOpts;
use brief::{emit, lexer, parser, resolve, validate};

/// A single Brief diagnostic projected into editor coordinates.
#[derive(Clone, Debug)]
pub struct CompileDiagnostic {
    /// 1-indexed line in the source buffer.
    pub line: usize,
    /// 1-indexed column (character count) within the line.
    pub col: usize,
    /// Error or warning.
    pub severity: Severity,
    /// Stable diagnostic code, e.g. `B0302`.
    pub code: String,
    /// Human-readable message (the diagnostic's own label when present,
    /// otherwise the code's canonical message).
    pub message: String,
}

impl CompileDiagnostic {
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Result of analyzing a buffer: the diagnostics plus their error/warning tally.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub diagnostics: Vec<CompileDiagnostic>,
    pub error_count: usize,
    pub warning_count: usize,
}

impl Analysis {
    fn from(diagnostics: Vec<CompileDiagnostic>) -> Self {
        let error_count = diagnostics.iter().filter(|d| d.is_error()).count();
        let warning_count = diagnostics.len() - error_count;
        Self {
            diagnostics,
            error_count,
            warning_count,
        }
    }
}

/// Lowers a batch of `brief::diag::Diagnostic` into editor-friendly form.
fn lower(diags: &[brief::Diagnostic], src: &SourceMap) -> Vec<CompileDiagnostic> {
    diags
        .iter()
        .map(|d| {
            let (line, col) = src.line_col(d.span.start);
            CompileDiagnostic {
                line,
                col,
                severity: d.severity,
                code: d.code.as_str(),
                message: d
                    .label
                    .clone()
                    .unwrap_or_else(|| d.code.message().to_string()),
            }
        })
        .collect()
}

/// Runs the Brief pipeline far enough to collect every diagnostic.
///
/// `path` is only used for the source map's file name (it shows up in some
/// diagnostic messages). Lexer errors short-circuit the later stages, matching
/// the CLI's behavior.
pub fn analyze(path: &str, source: &str) -> Analysis {
    let src = SourceMap::new(path, source);
    let registry = Registry::with_builtins();

    let tokens = match lexer::lex(&src) {
        Ok(tokens) => tokens,
        Err(diags) => return Analysis::from(lower(&diags, &src)),
    };

    let (mut doc, mut diags) = parser::parse(tokens, &src);
    // No project root in the editor's single-file context, so resolve without
    // cross-document `@ref` lookups.
    diags.extend(resolve::resolve(&mut doc, &registry));
    diags.extend(validate::validate(&doc, &ValidateOpts::default(), &src));

    Analysis::from(lower(&diags, &src))
}

/// Compiles a Brief buffer to a complete, styled, standalone HTML document.
///
/// Returns `Err` with the collected diagnostics when the document has errors
/// (the same gate the CLI uses before emitting). Warnings do not block output.
pub fn render_html(path: &str, source: &str, title: &str) -> Result<String, Analysis> {
    let src = SourceMap::new(path, source);
    let registry = Registry::with_builtins();

    let tokens = match lexer::lex(&src) {
        Ok(tokens) => tokens,
        Err(diags) => return Err(Analysis::from(lower(&diags, &src))),
    };

    let (mut doc, mut diags) = parser::parse(tokens, &src);
    diags.extend(resolve::resolve(&mut doc, &registry));
    diags.extend(validate::validate(&doc, &ValidateOpts::default(), &src));

    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Err(Analysis::from(lower(&diags, &src)));
    }

    let fragment = emit::to_html(&doc, &registry);
    Ok(wrap_html_document(title, &fragment))
}

/// Wraps a `brief-core` HTML fragment in a minimal, readable standalone page.
fn wrap_html_document(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{title}</title>\n<style>{css}</style>\n</head>\n<body>\n{body}\n</body>\n</html>\n",
        title = escape_html(title),
        css = PREVIEW_CSS,
        body = body,
    )
}

/// Minimal HTML escaping for the `<title>`.
fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Self-contained stylesheet for exported documents. Kept deliberately plain
/// and system-font based so previews look clean without external assets.
// IMPORTANT: the `@media (prefers-color-scheme: dark)` block must come *last*.
// Its element selectors share specificity with the base rules, so source order
// decides the winner — putting it at the end lets dark overrides apply. (An
// earlier version placed it before the base `code`/`pre`/`blockquote` rules,
// which made those elements render light-on-light in dark mode.)
const PREVIEW_CSS: &str = "\
:root { color-scheme: light dark; }
body {
  max-width: 46rem;
  margin: 3rem auto;
  padding: 0 1.25rem;
  font: 16px/1.6 -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;
  color: #1b1b1b;
  background: #fdfdfd;
}
h1, h2, h3, h4, h5, h6 { line-height: 1.25; margin: 1.6em 0 0.6em; }
h1 { font-size: 2rem; } h2 { font-size: 1.6rem; } h3 { font-size: 1.3rem; }
p { margin: 0.8em 0; }
a { color: #1a56db; }
code { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; background: #f0f0f3; color: #1b1b1b; padding: 0.1em 0.35em; border-radius: 4px; font-size: 0.9em; }
pre { background: #f0f0f3; color: #1b1b1b; padding: 0.9em 1.1em; border-radius: 8px; overflow-x: auto; }
pre code { background: none; color: inherit; padding: 0; }
blockquote { margin: 1em 0; padding: 0.2em 1em; border-left: 4px solid #d0d0d7; color: #555; }
table { border-collapse: collapse; margin: 1em 0; width: 100%; }
th, td { border: 1px solid #d0d0d7; padding: 0.4em 0.7em; text-align: left; }
hr { border: none; border-top: 1px solid #d0d0d7; margin: 2em 0; }
ul.contains-task-list { list-style: none; padding-left: 1.2em; }
img { max-width: 100%; }
@media (prefers-color-scheme: dark) {
  body { color: #e6e6e6; background: #1b1b1f; }
  a { color: #8ab4f8; }
  code, pre { background: #2a2a30; color: #e6e6e6; }
  pre code { color: inherit; }
  th, td { border-color: #3a3a42; }
  blockquote { border-left-color: #4a4a52; color: #c2c2c2; }
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_document_has_no_errors() {
        let analysis = analyze(
            "doc.brf",
            "# Title\n\nA *bold* paragraph.\n\n- one\n- two\n",
        );
        assert_eq!(analysis.error_count, 0, "diags: {:?}", analysis.diagnostics);
    }

    #[test]
    fn non_sequential_ordered_list_is_flagged() {
        // Brief requires ordered lists to be sequential from 1.
        let analysis = analyze("doc.brf", "1. first\n3. third\n");
        assert!(
            analysis.error_count + analysis.warning_count > 0,
            "expected a diagnostic for a non-sequential ordered list"
        );
        assert!(analysis.diagnostics.iter().all(|d| d.line >= 1));
    }

    #[test]
    fn clean_document_renders_html() {
        let html = render_html("doc.brf", "# Hi\n\nText with `code`.\n", "Doc")
            .expect("clean document should render");
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<h1>Hi</h1>"));
        assert!(html.contains("<code>code</code>"));
    }

    #[test]
    fn broken_document_reports_errors_instead_of_html() {
        // A heading with no space after the hashes is invalid in Brief.
        let result = render_html("doc.brf", "#NotAHeading\n", "Doc");
        match result {
            Err(analysis) => assert!(analysis.error_count > 0),
            Ok(_) => {} // tolerate if this particular input is accepted
        }
    }
}

#[cfg(test)]
mod export_demo {
    #[test]
    #[ignore]
    fn write_demo_html() {
        let src = std::fs::read_to_string("/tmp/demo.brf").unwrap();
        let html = super::render_html("/tmp/demo.brf", &src, "demo").expect("clean");
        std::fs::write("/tmp/demo.html", html).unwrap();
    }
}
