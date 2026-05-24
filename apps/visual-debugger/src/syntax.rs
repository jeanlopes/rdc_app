//! Hand-rolled Rust tokenizer for basic syntax highlighting.

/// Syntax highlighting categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Syntax highlighting categories.
pub enum TokenClass {
    /// fn, let, pub, impl, struct, enum, match, …
    Keyword,
    /// "…"
    StringLiteral,
    /// '…'
    CharLiteral,
    /// // …
    LineComment,
    /// /* … */
    BlockComment,
    /// CamelCase identifiers
    TypeIdent,
    /// everything else
    Other,
}

/// Tokenize a single line of Rust source.
/// Tokenize a single line of Rust source.
pub fn tokenize_line(src: &str) -> Vec<(TokenClass, &str)> {
    let mut tokens = Vec::new();
    let mut i = 0;
    let bytes = src.as_bytes();

    while i < bytes.len() {
        // Line comment
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            let start = i;
            i = bytes.len();
            tokens.push((TokenClass::LineComment, &src[start..]));
            continue;
        }
        // Block comment (simple single-line only for perf)
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < bytes.len() {
                i += 2;
            }
            tokens.push((TokenClass::BlockComment, &src[start..i]));
            continue;
        }
        // String literal
        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            tokens.push((TokenClass::StringLiteral, &src[start..i]));
            continue;
        }
        // Char literal
        if bytes[i] == b'\'' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                } else if bytes[i] == b'\'' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            tokens.push((TokenClass::CharLiteral, &src[start..i]));
            continue;
        }
        // Identifier / keyword
        if is_ident_start(bytes[i]) {
            let start = i;
            i += 1;
            while i < bytes.len() && is_ident_cont(bytes[i]) {
                i += 1;
            }
            let word = &src[start..i];
            let class = if KEYWORDS.contains(&word) {
                TokenClass::Keyword
            } else if is_type_ident(word) {
                TokenClass::TypeIdent
            } else {
                TokenClass::Other
            };
            tokens.push((class, word));
            continue;
        }
        // Number literal (treat as Other)
        if bytes[i].is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_' || bytes[i] == b'.') {
                i += 1;
            }
            tokens.push((TokenClass::Other, &src[start..i]));
            continue;
        }
        // Whitespace / punctuation
        let start = i;
        i += 1;
        tokens.push((TokenClass::Other, &src[start..i]));
    }

    tokens
}

fn is_ident_start(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphabetic()
}

fn is_ident_cont(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

fn is_type_ident(word: &str) -> bool {
    let mut chars = word.chars();
    if let Some(first) = chars.next() {
        first.is_ascii_uppercase() && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    } else {
        false
    }
}

static KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
    "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
    "trait", "true", "type", "unsafe", "use", "where", "while", "abstract", "become", "box",
    "do", "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield",
    "try",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_keyword() {
        let tokens = tokenize_line("fn foo");
        assert_eq!(tokens[0], (TokenClass::Keyword, "fn"));
    }

    #[test]
    fn tokenize_string_literal() {
        let tokens = tokenize_line(r#""hello""#);
        assert!(tokens.iter().any(|(c, s)| *c == TokenClass::StringLiteral && s.contains("hello")));
    }

    #[test]
    fn tokenize_line_comment() {
        let tokens = tokenize_line("// comment");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], (TokenClass::LineComment, "// comment"));
    }

    #[test]
    fn tokenize_block_comment() {
        let tokens = tokenize_line("/* a */ x");
        assert_eq!(tokens[0], (TokenClass::BlockComment, "/* a */"));
        assert_eq!(tokens[1], (TokenClass::Other, " "));
        assert_eq!(tokens[2], (TokenClass::Other, "x"));
    }

    #[test]
    fn tokenize_type_ident() {
        let tokens = tokenize_line("MyStruct");
        assert_eq!(tokens[0], (TokenClass::TypeIdent, "MyStruct"));
    }

    #[test]
    fn tokenize_empty_line() {
        let tokens = tokenize_line("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_10k_lines_no_panic() {
        let line = r#"let x = "hello" + 42; // comment"#;
        let start = std::time::Instant::now();
        for _ in 0..10_000 {
            let _ = tokenize_line(line);
        }
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 100, "tokenize 10k lines took {}ms", elapsed.as_millis());
    }
}
