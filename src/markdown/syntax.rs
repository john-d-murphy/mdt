//! Syntax highlighting infrastructure for code blocks.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use std::sync::OnceLock;
use syntect::easy::ScopeRegionIterator;
use syntect::parsing::SyntaxSet;
use syntect::parsing::{ParseState, Scope, ScopeStack};
use syntect::util::LinesWithEndings;

/// Semantic token categories for ANSI code highlighting.
pub(super) enum CodeToken {
    Comment,
    String,
    Number,
    Operator,
    Keyword,
    Function,
    Type,
    Tag,
    Punctuation,
    Variable,
    Constant,
    Normal,
}

impl CodeToken {
    pub(super) fn to_style(&self) -> Style {
        match self {
            CodeToken::Comment => Style::new().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            CodeToken::String => Style::new().fg(Color::Green),
            CodeToken::Number => Style::new().fg(Color::Cyan),
            CodeToken::Operator => Style::new().fg(Color::Cyan),
            CodeToken::Keyword => Style::new().fg(Color::Magenta),
            CodeToken::Function => Style::new().fg(Color::Blue),
            CodeToken::Type => Style::new().fg(Color::Yellow),
            CodeToken::Tag => Style::new().fg(Color::Red),
            CodeToken::Punctuation => Style::default(),
            CodeToken::Variable => Style::new().fg(Color::Red),
            CodeToken::Constant => Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            CodeToken::Normal => Style::default(),
        }
    }
}

/// Pre-built `Scope` objects for prefix matching.
pub(crate) struct ScopeMatchers {
    comment: Scope,
    string: Scope,
    constant_character: Scope,
    constant_numeric: Scope,
    keyword_operator: Scope,
    keyword: Scope,
    storage: Scope,
    entity_name_function: Scope,
    support_function: Scope,
    entity_name_type: Scope,
    support_type: Scope,
    entity_name_tag: Scope,
    punctuation: Scope,
    variable: Scope,
    entity_name: Scope,
    constant: Scope,
}

impl ScopeMatchers {
    pub(super) fn new() -> Self {
        Self {
            comment: Scope::new("comment").expect("valid scope literal"),
            string: Scope::new("string").expect("valid scope literal"),
            constant_character: Scope::new("constant.character").expect("valid scope literal"),
            constant_numeric: Scope::new("constant.numeric").expect("valid scope literal"),
            keyword_operator: Scope::new("keyword.operator").expect("valid scope literal"),
            keyword: Scope::new("keyword").expect("valid scope literal"),
            storage: Scope::new("storage").expect("valid scope literal"),
            entity_name_function: Scope::new("entity.name.function").expect("valid scope literal"),
            support_function: Scope::new("support.function").expect("valid scope literal"),
            entity_name_type: Scope::new("entity.name.type").expect("valid scope literal"),
            support_type: Scope::new("support.type").expect("valid scope literal"),
            entity_name_tag: Scope::new("entity.name.tag").expect("valid scope literal"),
            punctuation: Scope::new("punctuation").expect("valid scope literal"),
            variable: Scope::new("variable").expect("valid scope literal"),
            entity_name: Scope::new("entity.name").expect("valid scope literal"),
            constant: Scope::new("constant").expect("valid scope literal"),
        }
    }
}

/// SuperCollider's grammar, embedded: syntect's defaults have none (a fence tagged
/// `supercollider`, `sc`, `scd` or `sclang`).
const SUPERCOLLIDER: &str = include_str!("../../syntaxes/SuperCollider.sublime-syntax");

/// The syntaxes: syntect's defaults, SuperCollider, and whatever `.sublime-syntax` files are
/// in `$XDG_CONFIG_HOME/mdt/syntaxes` (default `~/.config/mdt/syntaxes`) — a grammar dropped
/// there is highlighted on the next run, no rebuild.
fn build_syntax_set() -> SyntaxSet {
    use syntect::parsing::SyntaxDefinition;
    let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
    if let Ok(def) = SyntaxDefinition::load_from_str(SUPERCOLLIDER, true, None) {
        builder.add(def);
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")));
    if let Some(dir) = config.map(|c| c.join("mdt").join("syntaxes")) {
        if dir.is_dir() {
            // A broken grammar in the folder is skipped with the rest kept: never a crash on open.
            let _ = builder.add_from_folder(&dir, true);
        }
    }
    builder.build()
}

pub(crate) fn syntax_set() -> &'static SyntaxSet {
    static SS: OnceLock<SyntaxSet> = OnceLock::new();
    SS.get_or_init(build_syntax_set)
}

pub(crate) fn scope_matchers() -> &'static ScopeMatchers {
    static MATCHERS: OnceLock<ScopeMatchers> = OnceLock::new();
    MATCHERS.get_or_init(ScopeMatchers::new)
}

/// Pre-compile regex patterns for common languages by parsing a dummy snippet.
///
/// Syntect lazily compiles regexes on first `parse_line` call per syntax definition.
/// Running this on a background thread at startup eliminates the ~30ms-per-language
/// stall on the first real file open.
pub(crate) fn prewarm_syntaxes() {
    let ss = syntax_set();
    let matchers = scope_matchers();
    let langs = [
        "rust",
        "python",
        "bash",
        "javascript",
        "typescript",
        "go",
        "c",
        "cpp",
        "java",
        "json",
        "yaml",
        "toml",
        "html",
        "css",
        "sql",
        "ruby",
        "markdown",
    ];
    for lang in &langs {
        if let Some(syntax) = ss.find_syntax_by_token(lang) {
            let mut state = ParseState::new(syntax);
            let mut stack = ScopeStack::new();
            // Parse a trivial line to force regex compilation for this syntax.
            if let Ok(ops) = state.parse_line("x\n", ss) {
                for (_s, op) in ScopeRegionIterator::new(&ops, "x\n") {
                    let _ = stack.apply(op);
                    // Touch scope_to_style to warm scope matcher paths.
                    let _ = scope_to_style(&stack, matchers);
                }
            }
        }
    }
}

pub(super) fn no_color() -> bool {
    static NO_COLOR: OnceLock<bool> = OnceLock::new();
    *NO_COLOR.get_or_init(|| std::env::var("NO_COLOR").is_ok_and(|v| !v.is_empty()))
}

/// Map the most-specific scope in the stack to an ANSI style.
/// Priority order: most-specific prefixes checked first.
pub(super) fn scope_to_style(stack: &ScopeStack, m: &ScopeMatchers) -> Style {
    let Some(&scope) = stack.as_slice().last() else {
        return CodeToken::Normal.to_style();
    };

    // Priority order: most-specific first
    if m.comment.is_prefix_of(scope) {
        return CodeToken::Comment.to_style();
    }
    if m.string.is_prefix_of(scope) || m.constant_character.is_prefix_of(scope) {
        return CodeToken::String.to_style();
    }
    if m.constant_numeric.is_prefix_of(scope) {
        return CodeToken::Number.to_style();
    }
    // keyword.operator MUST be before keyword
    if m.keyword_operator.is_prefix_of(scope) {
        return CodeToken::Operator.to_style();
    }
    if m.keyword.is_prefix_of(scope) || m.storage.is_prefix_of(scope) {
        return CodeToken::Keyword.to_style();
    }
    if m.entity_name_function.is_prefix_of(scope) || m.support_function.is_prefix_of(scope) {
        return CodeToken::Function.to_style();
    }
    if m.entity_name_type.is_prefix_of(scope) || m.support_type.is_prefix_of(scope) {
        return CodeToken::Type.to_style();
    }
    if m.entity_name_tag.is_prefix_of(scope) {
        return CodeToken::Tag.to_style();
    }
    if m.punctuation.is_prefix_of(scope) {
        return CodeToken::Punctuation.to_style();
    }
    if m.variable.is_prefix_of(scope) || m.entity_name.is_prefix_of(scope) {
        return CodeToken::Variable.to_style();
    }
    // constant MUST be after specific constant prefixes
    if m.constant.is_prefix_of(scope) {
        return CodeToken::Constant.to_style();
    }

    CodeToken::Normal.to_style()
}

pub(super) fn highlight_code(code: &str, lang: &str) -> Vec<Vec<Span<'static>>> {
    let ss = syntax_set();

    // Try to find syntax for the language.
    let syntax = if lang.is_empty() { None } else { ss.find_syntax_by_token(lang) };

    match syntax {
        Some(syntax) => {
            let mut state = ParseState::new(syntax);
            let mut stack = ScopeStack::new();
            let matchers = scope_matchers();
            let mut result = Vec::new();

            for line in LinesWithEndings::from(code) {
                let ops = state.parse_line(line, ss).unwrap_or_default();
                let mut spans = Vec::new();

                for (s, op) in ScopeRegionIterator::new(&ops, line) {
                    let _ = stack.apply(op);
                    if s.is_empty() {
                        continue;
                    }
                    let style = scope_to_style(&stack, matchers);
                    spans.push(Span::styled(s.trim_end_matches('\n').to_string(), style));
                }

                result.push(spans);
            }
            result
        }
        None => {
            // No syntax found — render with uniform code style.
            code.lines()
                .map(|line| vec![Span::styled(line.to_string(), super::CODE_DEFAULT_STYLE)])
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntect::parsing::{Scope, ScopeStack};

    #[test]
    fn supercollider_is_a_syntax_and_its_symbols_strings_and_classes_are_scoped() {
        use syntect::parsing::ParseState;
        let ss = syntax_set();
        for token in ["supercollider", "sc", "scd", "sclang"] {
            assert!(ss.find_syntax_by_token(token).is_some(), "{token}");
        }
        let syntax = ss.find_syntax_by_token("supercollider").unwrap();
        let mut state = ParseState::new(syntax);
        let line = "SynthDef(\\sine, { |freq=440| Out.ar(0, SinOsc.ar(freq) * 0.1) }).add; // hi\n";
        let ops = state.parse_line(line, ss).unwrap();
        let mut stack = ScopeStack::new();
        let mut seen = std::collections::HashSet::new();
        for (_, op) in ScopeRegionIterator::new(&ops, line) {
            stack.apply(op).unwrap();
            for s in stack.as_slice() {
                seen.insert(s.build_string());
            }
        }
        for want in ["entity.name.type.class.supercollider", "constant.character.symbol.supercollider", "entity.name.function.method.supercollider", "constant.numeric.supercollider", "comment.line.double-slash.supercollider"] {
            assert!(seen.iter().any(|s| s == want), "missing {want} in {seen:?}");
        }
    }

    #[test]
    fn scope_keyword_operator_gets_operator_not_keyword() {
        let mut stack = ScopeStack::new();
        stack.push(Scope::new("keyword.operator").unwrap());
        let style = scope_to_style(&stack, scope_matchers());
        // keyword.operator → Operator (Cyan), NOT Keyword (Magenta)
        assert_eq!(style.fg, Some(Color::Cyan));
        assert_ne!(style.fg, Some(Color::Magenta));
    }

    #[test]
    fn scope_comment_gets_darkgray_italic() {
        let mut stack = ScopeStack::new();
        stack.push(Scope::new("comment.line.double-slash").unwrap());
        let style = scope_to_style(&stack, scope_matchers());
        assert_eq!(style.fg, Some(Color::DarkGray));
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn scope_string_gets_green() {
        let mut stack = ScopeStack::new();
        stack.push(Scope::new("string.quoted.double").unwrap());
        let style = scope_to_style(&stack, scope_matchers());
        assert_eq!(style.fg, Some(Color::Green));
    }

    #[test]
    fn scope_function_gets_blue() {
        let mut stack = ScopeStack::new();
        stack.push(Scope::new("entity.name.function").unwrap());
        let style = scope_to_style(&stack, scope_matchers());
        assert_eq!(style.fg, Some(Color::Blue));
    }

    #[test]
    fn scope_type_gets_yellow() {
        let mut stack = ScopeStack::new();
        stack.push(Scope::new("entity.name.type").unwrap());
        let style = scope_to_style(&stack, scope_matchers());
        assert_eq!(style.fg, Some(Color::Yellow));
    }

    #[test]
    fn scope_unknown_gets_default() {
        let mut stack = ScopeStack::new();
        stack.push(Scope::new("unknown.scope.xyz").unwrap());
        let style = scope_to_style(&stack, scope_matchers());
        assert_eq!(style, Style::default());
    }

    #[test]
    fn scope_entity_name_function_before_entity_name() {
        // entity.name.function should match Function (Blue),
        // NOT fall through to Variable (Red) via entity.name
        let mut stack = ScopeStack::new();
        stack.push(Scope::new("entity.name.function").unwrap());
        let style = scope_to_style(&stack, scope_matchers());
        assert_eq!(style.fg, Some(Color::Blue));
        assert_ne!(style.fg, Some(Color::Red));
    }

    #[test]
    fn scope_constant_numeric_before_constant() {
        // constant.numeric → Number (Cyan, no Bold),
        // NOT Constant (Cyan + Bold)
        let mut stack = ScopeStack::new();
        stack.push(Scope::new("constant.numeric").unwrap());
        let style = scope_to_style(&stack, scope_matchers());
        assert_eq!(style.fg, Some(Color::Cyan));
        assert!(!style.add_modifier.contains(Modifier::BOLD));
    }
}
