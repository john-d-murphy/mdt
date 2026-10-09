//! Recognising quine's link lines.
//!
//! `quine read --markdown` ends a page with `## Links out` and `## Links in` lists whose
//! items have one shape: an arrowed relationship, the other node's name, and a bracketed
//! tail — the node's short id (an inline code span) and kind, or `[dangling]`:
//!
//! ```text
//! —links-to→ Measuring the room [`meas0` note]
//! ←informed-by— The piano [`pian0` note]
//! —links-to→ notes:inbox/never-written.md [dangling]
//! ```
//!
//! That is the pattern
//! `^([—←])([a-z][a-z-]*)([→—])\s+(.+?)\s+\[(`?[a-z0-9]+`?)(?:\s+([a-z]+))?\]$`
//! (or `\[dangling\]$`), parsed here by hand so no regex crate is needed. The renderer
//! paints a matching item the way quine's triage does; every other item is left alone.

/// The parts of one link line, ready to be styled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LinkLine {
    /// The relationship with its arrows, e.g. `—links-to→` or `←informed-by—`.
    pub edge: String,
    /// The other node's name (or the unresolved path, for a dangling link).
    pub name: String,
    pub tail: Tail,
}

/// What a link line ends in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Tail {
    /// `[id kind]` — the node's short id and, usually, its kind.
    Node { id: String, kind: Option<String> },
    /// `[dangling]` — a link to a node that does not exist.
    Dangling,
}

/// Parse the text of a list item (marker removed, inline code re-wrapped in backticks).
/// `None` when the text is not a link line.
pub(super) fn parse_link_line(text: &str) -> Option<LinkLine> {
    let text = text.trim_end();
    let (edge, rest) = split_edge(text)?;

    // `\s+` between the edge and the name.
    let rest = rest.strip_prefix(char::is_whitespace)?.trim_start();

    // The tail is the last bracket group, which must close the line.
    let inner = rest.strip_suffix(']')?;
    let open = inner.rfind('[')?;
    let (head, tail_text) = (&inner[..open], &inner[open + 1..]);

    // `\s+` between the name and the tail; the name itself is non-empty.
    if !head.ends_with(char::is_whitespace) {
        return None;
    }
    let name = head.trim_end();
    if name.is_empty() {
        return None;
    }

    let tail = parse_tail(tail_text)?;
    Some(LinkLine { edge: edge.to_string(), name: name.to_string(), tail })
}

/// Split off `—rel→` (link out) or `←rel—` (link in) from the start of the text.
fn split_edge(text: &str) -> Option<(&str, &str)> {
    let (open, close) = if text.starts_with('—') {
        ('—', '→')
    } else if text.starts_with('←') {
        ('←', '—')
    } else {
        return None;
    };
    let after_open = &text[open.len_utf8()..];
    let rel_len = after_open.find(close)?;
    let rel = &after_open[..rel_len];
    if !is_relationship(rel) {
        return None;
    }
    let edge_len = open.len_utf8() + rel_len + close.len_utf8();
    Some(text.split_at(edge_len))
}

/// `[a-z][a-z-]*`
fn is_relationship(rel: &str) -> bool {
    let mut chars = rel.chars();
    matches!(chars.next(), Some('a'..='z')) && chars.all(|c| c.is_ascii_lowercase() || c == '-')
}

fn parse_tail(tail: &str) -> Option<Tail> {
    if tail == "dangling" {
        return Some(Tail::Dangling);
    }
    let mut words = tail.split_whitespace();
    let id = words.next()?.trim_matches('`');
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
        return None;
    }
    let kind = match words.next() {
        Some(k) if k.chars().all(|c| c.is_ascii_lowercase()) && !k.is_empty() => Some(k),
        Some(_) => return None,
        None => None,
    };
    if words.next().is_some() {
        return None;
    }
    Some(Tail::Node { id: id.to_string(), kind: kind.map(str::to_string) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, kind: Option<&str>) -> Tail {
        Tail::Node { id: id.to_string(), kind: kind.map(str::to_string) }
    }

    #[test]
    fn link_out_with_id_and_kind() {
        let got = parse_link_line("—links-to→ Measuring the room [`meas0` note]").unwrap();
        assert_eq!(got.edge, "—links-to→");
        assert_eq!(got.name, "Measuring the room");
        assert_eq!(got.tail, node("meas0", Some("note")));
    }

    #[test]
    fn link_in_arrows_reversed() {
        let got = parse_link_line("←informed-by— The piano [`pian0` note]").unwrap();
        assert_eq!(got.edge, "←informed-by—");
        assert_eq!(got.name, "The piano");
        assert_eq!(got.tail, node("pian0", Some("note")));
    }

    #[test]
    fn dangling_link_keeps_the_path_as_name() {
        let got = parse_link_line("—links-to→ notes:inbox/never-written.md [dangling]").unwrap();
        assert_eq!(got.name, "notes:inbox/never-written.md");
        assert_eq!(got.tail, Tail::Dangling);
    }

    #[test]
    fn id_without_backticks_or_kind() {
        let got = parse_link_line("—extends→ The rug [rug00]").unwrap();
        assert_eq!(got.tail, node("rug00", None));
    }

    #[test]
    fn name_may_contain_brackets() {
        let got = parse_link_line("—links-to→ Notes [draft] on tuning [`tune0` note]").unwrap();
        assert_eq!(got.name, "Notes [draft] on tuning");
        assert_eq!(got.tail, node("tune0", Some("note")));
    }

    #[test]
    fn only_the_last_bracket_group_is_the_tail() {
        let got = parse_link_line("—links-to→ The rug [dangling] [`meas0` note]").unwrap();
        assert_eq!(got.name, "The rug [dangling]");
        assert_eq!(got.tail, node("meas0", Some("note")));
    }

    #[test]
    fn trailing_whitespace_is_ignored() {
        assert!(parse_link_line("—links-to→ The rug [`rug00` note]  ").is_some());
    }

    #[test]
    fn rejects_ordinary_items_and_near_misses() {
        for text in [
            "Just a list item",
            "—links-to→ [`meas0` note]",           // no name
            "—links-to→ The rug",                  // no tail
            "—links-to→ The rug [`meas0` Note]",   // kind not lowercase
            "—links-to→ The rug [`meas0` note x]", // too many words
            "—links-to→ The rug [`MEAS0` note]",   // id not lowercase
            "—Links-To→ The rug [`meas0` note]",   // relationship not lowercase
            "—links-to→The rug [`meas0` note]",    // no space after edge
            "—links-to→ The rug[`meas0` note]",    // no space before tail
            "→links-to— The rug [`meas0` note]",   // arrows the wrong way round
            "←links-to→ The rug [`meas0` note]",   // mismatched arrow pair
        ] {
            assert!(parse_link_line(text).is_none(), "should not match: {text:?}");
        }
    }
}
