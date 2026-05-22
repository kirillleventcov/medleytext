//! Brief markup language tokenizer for syntax highlighting.
//!
//! Brief is a strict markup language. See `LearnXinYminutes.brf` in the
//! upstream Brief repo for the full grammar. The tokenizer here mirrors the
//! shape of `markdown.rs`: per-line tokenization producing `(text, kind)`
//! pairs that the editor renders as colored segments.
//!
//! Inline rules implemented:
//! - `*bold*`, `_italic_`, `+underline+`, `~strike~` (single-marker only)
//! - inline code: `` `code` `` and ``` ``code`` ``` (verbatim, no inline parsing inside)
//! - inline shortcodes: `@name(args)[body]`
//! - escape `\X`
//! - hard break: trailing `\` at end of line
//!
//! Block rules implemented:
//! - line comments: `// ...`
//! - block comment markers `/*` and `*/`
//! - headings `#`..`######` followed by exactly one space
//! - bullet `- ` and ordered `N. `
//! - blockquote `> ` (and nested `>>`)
//! - triple-backtick fences
//! - horizontal rule `---`
//! - table rows: `|` separators when in a `@t` block
//! - block shortcodes `@name(...)` and `@end`
//! - task markers `[x] ` / `[ ] ` after a bullet
//!
//! The tokenizer is line-based and stateless across lines, like
//! `MarkdownHighlighter`. The renderer feeds each line in isolation.

use crate::config::SyntaxTheme;
use gpui::Rgba;

/// Brief token kinds. Each maps to a syntax color via `SyntaxTheme`.
#[derive(Debug, Clone)]
pub enum BriefToken {
    /// `#`..`######` heading line (level 1-6).
    Heading(usize),
    /// `*bold*` strong emphasis.
    Bold,
    /// `_italic_` em emphasis.
    Italic,
    /// `+underline+` underline emphasis.
    Underline,
    /// `~strike~` strikethrough emphasis.
    Strike,
    /// `` `code` `` or ``` ``code`` ``` inline code.
    Code,
    /// `- ` / `N. ` list item marker (entire line for now).
    ListItem,
    /// `- [x] ` completed task line.
    CheckboxChecked,
    /// `- [ ] ` open task line.
    CheckboxUnchecked,
    /// `> ` / `>> ` blockquote line.
    Blockquote,
    /// ` ``` ` fence line.
    CodeBlock,
    /// `---` horizontal rule.
    HorizontalRule,
    /// `// ...` line comment or `/*`/`*/` block-comment markers.
    Comment,
    /// `@name`, `@end`, or `@name(args)` shortcode / directive.
    Shortcode,
    /// Plain text inside a table row or the `|` separators.
    Table,
    /// Trailing `\` hard-break marker at end of line.
    HardBreak,
    /// `\X` backslash escape: rendered as muted/normal.
    Escape,
    /// Anything that didn't match a more specific kind.
    Normal,
}

/// Stateless per-line Brief syntax highlighter.
pub struct BriefHighlighter;

impl BriefHighlighter {
    pub fn get_color(token: &BriefToken, theme: &SyntaxTheme) -> Rgba {
        match token {
            BriefToken::Heading(level) => theme.heading_color(*level),
            BriefToken::Bold => theme.bold,
            BriefToken::Italic => theme.italic,
            BriefToken::Underline => theme.underline,
            BriefToken::Strike => theme.strikethrough,
            BriefToken::Code => theme.code,
            BriefToken::ListItem => theme.list,
            BriefToken::CheckboxChecked => theme.checkbox_checked,
            BriefToken::CheckboxUnchecked => theme.checkbox_unchecked,
            BriefToken::Blockquote => theme.blockquote,
            BriefToken::CodeBlock => theme.code_block,
            BriefToken::HorizontalRule => theme.hr,
            BriefToken::Comment => theme.comment,
            BriefToken::Shortcode => theme.shortcode,
            BriefToken::Table => theme.table,
            BriefToken::HardBreak => theme.hard_break,
            BriefToken::Escape => theme.escape,
            BriefToken::Normal => theme.normal,
        }
    }

    /// Tokenizes one line of Brief into `(text, kind)` runs.
    ///
    /// Output text concatenated equals the input line exactly — the
    /// renderer relies on that to keep byte offsets aligned with the
    /// underlying document.
    pub fn tokenize_line(line: &str) -> Vec<(String, BriefToken)> {
        // Whole-line patterns first.

        // Line comment: leading-spaces then `//`
        let leading_ws_len = line.bytes().take_while(|b| *b == b' ').count();
        let trimmed = &line[leading_ws_len..];

        if trimmed.starts_with("//") {
            return vec![(line.to_string(), BriefToken::Comment)];
        }

        // Block comment open / close markers occupy whole lines in Brief
        // (the opening `/*` must begin a line and the closing `*/` must end
        // one). We only color the marker lines; mid-block content goes
        // through normal tokenization, which is the best we can do without
        // multi-line state.
        if trimmed == "/*"
            || trimmed.starts_with("/* ")
            || trimmed == "*/"
            || trimmed.ends_with(" */")
        {
            return vec![(line.to_string(), BriefToken::Comment)];
        }

        // Headings: `#` run of 1..=6, exactly one space, then text.
        if trimmed.starts_with('#') {
            let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
            if (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ') {
                return vec![(line.to_string(), BriefToken::Heading(hashes))];
            }
        }

        // Triple-backtick fence (open or close).
        if trimmed.starts_with("```") {
            return vec![(line.to_string(), BriefToken::CodeBlock)];
        }

        // Horizontal rule: exactly `---` alone (after trimming leading ws,
        // and no trailing non-ws beyond that).
        if trimmed == "---" {
            return vec![(line.to_string(), BriefToken::HorizontalRule)];
        }

        // Blockquote: one or more `>` followed by a single space.
        if trimmed.starts_with('>') {
            let arrows = trimmed.bytes().take_while(|b| *b == b'>').count();
            if trimmed.as_bytes().get(arrows) == Some(&b' ') {
                return vec![(line.to_string(), BriefToken::Blockquote)];
            }
        }

        // Bullet item: `- ` (Brief permits ONLY `-`, never `*` / `+`).
        if trimmed.starts_with("- ") {
            // Check for task marker: `- [x] ` or `- [ ] `.
            let after_bullet = &trimmed[2..];
            if let Some(rest) = after_bullet.strip_prefix("[x] ") {
                let _ = rest;
                return vec![(line.to_string(), BriefToken::CheckboxChecked)];
            }
            if let Some(rest) = after_bullet.strip_prefix("[ ] ") {
                let _ = rest;
                return vec![(line.to_string(), BriefToken::CheckboxUnchecked)];
            }
            return vec![(line.to_string(), BriefToken::ListItem)];
        }

        // Ordered list item: `N. ` (one or more digits, period, single space).
        if let Some(dot_pos) = trimmed.find(". ") {
            let prefix = &trimmed[..dot_pos];
            if !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_digit()) {
                return vec![(line.to_string(), BriefToken::ListItem)];
            }
        }

        // Table directive line `@t` (alone or with args).
        if trimmed.starts_with("@t") {
            let after = &trimmed[2..];
            if after.is_empty() || after.starts_with('(') {
                return vec![(line.to_string(), BriefToken::Shortcode)];
            }
        }

        // `@end` closes a block shortcode.
        if trimmed == "@end" {
            return vec![(line.to_string(), BriefToken::Shortcode)];
        }

        // Block shortcode open like `@callout(kind: warning)` or `@dl`.
        if trimmed.starts_with('@') {
            // We require the rest to look like an identifier so that an
            // inline `@link[...]` mid-paragraph still falls through to
            // inline parsing.
            let after_at = &trimmed[1..];
            let ident_len = after_at
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
                .count();
            if ident_len > 0 {
                let rest = &after_at[ident_len..];
                // A block shortcode is `@name` or `@name(...)` alone on a
                // line (no following `[` body). If there's a `[` later on
                // the line, treat the whole line as inline-mode text.
                if rest.is_empty() || rest.starts_with('(') {
                    let has_inline_body = rest.contains('[');
                    if !has_inline_body {
                        return vec![(line.to_string(), BriefToken::Shortcode)];
                    }
                }
            }
        }

        // Table row: starts with `|` (we don't track whether we're inside
        // an `@t` block — color any pipe-led line as a table row).
        if trimmed.starts_with('|') {
            return tokenize_table_row(line);
        }

        // Fall through to inline tokenization.
        tokenize_inline(line)
    }
}

/// Inline tokenizer: handles emphasis, code, escapes, inline shortcodes,
/// and trailing hard breaks. Operates on a string slice and emits runs
/// whose concatenation equals the input.
fn tokenize_inline(line: &str) -> Vec<(String, BriefToken)> {
    let mut tokens: Vec<(String, BriefToken)> = Vec::new();
    let mut current = String::new();

    let flush = |current: &mut String, tokens: &mut Vec<(String, BriefToken)>| {
        if !current.is_empty() {
            tokens.push((std::mem::take(current), BriefToken::Normal));
        }
    };

    // Detect trailing `\` hard break and tokenize the body without it; the
    // trailing backslash becomes its own HardBreak run.
    let (body, hard_break) = if let Some(stripped) = line.strip_suffix('\\') {
        // The `\` must not itself be escaped. Count preceding backslashes:
        let preceding = stripped.bytes().rev().take_while(|b| *b == b'\\').count();
        // The trailing backslash plus `preceding` more make the total. Hard
        // break only when the count of trailing backslashes is odd (i.e.,
        // 1, 3, ... unescaped).
        let trailing_run = preceding + 1;
        if trailing_run % 2 == 1 {
            (stripped, true)
        } else {
            (line, false)
        }
    } else {
        (line, false)
    };

    let bytes = body.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        // Escape: `\X` consumes two characters as an Escape run.
        if b == b'\\' && i + 1 < bytes.len() {
            flush(&mut current, &mut tokens);
            // Push the two-byte escape sequence.
            let end = i + 2;
            // Be careful with multibyte chars: `\` is ASCII so we can take
            // bytes through `end` safely up to the next char boundary.
            let mut e = end;
            while !body.is_char_boundary(e) && e < bytes.len() {
                e += 1;
            }
            tokens.push((body[i..e].to_string(), BriefToken::Escape));
            i = e;
            continue;
        }

        // Double-backtick inline code: ``...``
        if b == b'`' && bytes.get(i + 1) == Some(&b'`') {
            flush(&mut current, &mut tokens);
            let mut j = i + 2;
            while j + 1 < bytes.len() {
                if bytes[j] == b'`' && bytes[j + 1] == b'`' {
                    j += 2;
                    break;
                }
                j += 1;
            }
            // If we ran off the end without finding a close, eat the rest.
            if j > bytes.len() {
                j = bytes.len();
            }
            tokens.push((body[i..j].to_string(), BriefToken::Code));
            i = j;
            continue;
        }

        // Single backtick inline code: `...`
        if b == b'`' {
            flush(&mut current, &mut tokens);
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'`' {
                j += 1;
            }
            if j < bytes.len() {
                j += 1;
            }
            tokens.push((body[i..j].to_string(), BriefToken::Code));
            i = j;
            continue;
        }

        // Inline shortcode: `@name` (optional `(args)` and / or `[body]`).
        if b == b'@' {
            let after = &body[i + 1..];
            let ident_len = after
                .bytes()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
                .count();
            if ident_len > 0 {
                flush(&mut current, &mut tokens);
                let mut j = i + 1 + ident_len;
                // Args group `(...)` allowing balanced parens.
                if bytes.get(j) == Some(&b'(') {
                    let mut depth = 0i32;
                    while j < bytes.len() {
                        match bytes[j] {
                            b'(' => depth += 1,
                            b')' => {
                                depth -= 1;
                                if depth == 0 {
                                    j += 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                }
                // Body `[...]` allowing balanced brackets.
                if bytes.get(j) == Some(&b'[') {
                    let mut depth = 0i32;
                    while j < bytes.len() {
                        match bytes[j] {
                            b'[' => depth += 1,
                            b']' => {
                                depth -= 1;
                                if depth == 0 {
                                    j += 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                }
                // Some shortcodes (`@link`, `@ref`) take a markdown-style
                // `(url)` tail after the body. Consume one balanced paren
                // group right after `]`.
                if bytes.get(j) == Some(&b'(') {
                    let mut depth = 0i32;
                    while j < bytes.len() {
                        match bytes[j] {
                            b'(' => depth += 1,
                            b')' => {
                                depth -= 1;
                                if depth == 0 {
                                    j += 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                }
                tokens.push((body[i..j].to_string(), BriefToken::Shortcode));
                i = j;
                continue;
            }
        }

        // Emphasis: `*bold*`, `_italic_`, `+underline+`, `~strike~`.
        // Each is single-character, opens only if the next char isn't
        // whitespace and isn't the same marker (no doubled markers), and
        // closes on the next matching marker found on the same line.
        let kind = match b {
            b'*' => Some(BriefToken::Bold),
            b'_' => Some(BriefToken::Italic),
            b'+' => Some(BriefToken::Underline),
            b'~' => Some(BriefToken::Strike),
            _ => None,
        };
        if let Some(kind) = kind {
            // Reject literal markers between alphanumerics (e.g.
            // `snake_case_name` should stay plain).
            let prev = if i == 0 {
                None
            } else {
                bytes.get(i - 1).copied()
            };
            let next = bytes.get(i + 1).copied();
            let flanked_by_word = prev.map(|p| (p as char).is_alphanumeric()).unwrap_or(false)
                && next.map(|n| (n as char).is_alphanumeric()).unwrap_or(false);
            // Doubled markers (e.g. `**`) are NOT recognized as emphasis
            // in Brief; reject the open if either neighbor is the same
            // marker. Also reject when the next char is whitespace / EOL.
            let same_neighbor = prev == Some(b) || next == Some(b);
            let bad_open = matches!(next, None | Some(b' ') | Some(b'\t')) || same_neighbor;
            if !flanked_by_word && !bad_open {
                // Find matching close: same byte that isn't preceded by `\`
                // and isn't itself part of a doubled-marker run or an
                // intra-word literal.
                let mut j = i + 1;
                let mut found = None;
                while j < bytes.len() {
                    if bytes[j] == b'\\' && j + 1 < bytes.len() {
                        j += 2;
                        continue;
                    }
                    if bytes[j] == b {
                        let close_prev = bytes.get(j - 1).copied();
                        let close_next = bytes.get(j + 1).copied();
                        let intra_word = close_prev
                            .map(|p| (p as char).is_alphanumeric())
                            .unwrap_or(false)
                            && close_next
                                .map(|n| (n as char).is_alphanumeric())
                                .unwrap_or(false);
                        let close_doubled = close_next == Some(b);
                        if !intra_word && !close_doubled {
                            found = Some(j + 1);
                            break;
                        }
                    }
                    j += 1;
                }
                if let Some(end) = found {
                    flush(&mut current, &mut tokens);
                    tokens.push((body[i..end].to_string(), kind));
                    i = end;
                    continue;
                }
            }
        }

        // Otherwise: ordinary character; accumulate into the current run.
        let mut e = i + 1;
        while !body.is_char_boundary(e) && e < bytes.len() {
            e += 1;
        }
        current.push_str(&body[i..e]);
        i = e;
    }

    flush(&mut current, &mut tokens);

    if hard_break {
        tokens.push(("\\".to_string(), BriefToken::HardBreak));
    }

    if tokens.is_empty() {
        tokens.push((line.to_string(), BriefToken::Normal));
    }

    tokens
}

/// Tokenize a `|`-led table row: pipes get the Table color, cells are
/// inline-parsed so emphasis still shows inside cells.
fn tokenize_table_row(line: &str) -> Vec<(String, BriefToken)> {
    let mut tokens: Vec<(String, BriefToken)> = Vec::new();
    let bytes = line.as_bytes();

    // Preserve leading whitespace as Normal.
    let leading = line.bytes().take_while(|b| *b == b' ').count();
    let mut cell_start = leading;
    let mut i = leading;
    if leading > 0 {
        tokens.push((line[..leading].to_string(), BriefToken::Normal));
    }

    while i < bytes.len() {
        if bytes[i] == b'|' {
            // Flush cell content via inline tokenization.
            if i > cell_start {
                for (text, kind) in tokenize_inline(&line[cell_start..i]) {
                    tokens.push((text, kind));
                }
            }
            tokens.push(("|".to_string(), BriefToken::Table));
            i += 1;
            cell_start = i;
            continue;
        }
        i += 1;
    }
    if cell_start < bytes.len() {
        for (text, kind) in tokenize_inline(&line[cell_start..]) {
            tokens.push((text, kind));
        }
    }

    if tokens.is_empty() {
        tokens.push((line.to_string(), BriefToken::Normal));
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(line: &str) -> Vec<(String, &'static str)> {
        BriefHighlighter::tokenize_line(line)
            .into_iter()
            .map(|(s, t)| {
                let name: &'static str = match t {
                    BriefToken::Heading(_) => "heading",
                    BriefToken::Bold => "bold",
                    BriefToken::Italic => "italic",
                    BriefToken::Underline => "underline",
                    BriefToken::Strike => "strike",
                    BriefToken::Code => "code",
                    BriefToken::ListItem => "list",
                    BriefToken::CheckboxChecked => "check",
                    BriefToken::CheckboxUnchecked => "uncheck",
                    BriefToken::Blockquote => "quote",
                    BriefToken::CodeBlock => "fence",
                    BriefToken::HorizontalRule => "hr",
                    BriefToken::Comment => "comment",
                    BriefToken::Shortcode => "shortcode",
                    BriefToken::Table => "table",
                    BriefToken::HardBreak => "br",
                    BriefToken::Escape => "esc",
                    BriefToken::Normal => "normal",
                };
                (s, name)
            })
            .collect()
    }

    #[test]
    fn heading_levels() {
        assert_eq!(render("# Hello"), vec![("# Hello".into(), "heading")]);
        assert_eq!(render("###### Six"), vec![("###### Six".into(), "heading")]);
        // 7 hashes isn't a heading.
        let t = render("####### Seven");
        assert_ne!(t[0].1, "heading");
    }

    #[test]
    fn rejects_markdown_bold() {
        // `**bold**` is NOT bold in Brief; it should be normal text.
        let toks = render("**not bold**");
        assert!(toks.iter().all(|(_, k)| *k != "bold"));
    }

    #[test]
    fn snake_case_is_literal() {
        let toks = render("snake_case_name stays literal");
        assert!(toks.iter().all(|(_, k)| *k != "italic"));
    }

    #[test]
    fn star_pair_bolds() {
        let toks = render("a *bold* word");
        assert!(toks.iter().any(|(s, k)| s == "*bold*" && *k == "bold"));
    }

    #[test]
    fn comment_line() {
        assert_eq!(render("// note"), vec![("// note".into(), "comment")]);
    }

    #[test]
    fn shortcode_inline() {
        let toks = render("See @link[the spec](https://x).");
        assert!(
            toks.iter()
                .any(|(s, k)| *k == "shortcode" && s.starts_with("@link["))
        );
    }

    #[test]
    fn task_markers() {
        assert_eq!(render("- [x] done"), vec![("- [x] done".into(), "check")]);
        assert_eq!(render("- [ ] todo"), vec![("- [ ] todo".into(), "uncheck")]);
    }

    #[test]
    fn hard_break_at_end() {
        let toks = render("end\\");
        assert!(toks.iter().any(|(s, k)| s == "\\" && *k == "br"));
    }

    #[test]
    fn concat_equals_input() {
        let cases = [
            "# Heading",
            "- a list",
            "1. ordered",
            "*bold* and _it_",
            "snake_case_name",
            "@callout(kind: note)",
            "| Name | Age",
            "```",
            "---",
            "> quote",
            "// comment",
            "\\*literal\\*",
        ];
        for input in cases {
            let toks = BriefHighlighter::tokenize_line(input);
            let joined: String = toks.iter().map(|(s, _)| s.as_str()).collect();
            assert_eq!(joined, input, "concat mismatch for {input:?}");
        }
    }
}
