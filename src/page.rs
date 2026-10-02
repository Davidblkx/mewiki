//! What a page's Markdown source holds besides its body: the front-matter block and the title.

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, Options, parse_document};

/// The `---` block at the top of a page, kept line by line so keys the app doesn't know survive a save.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrontMatter {
    lines: Vec<String>,
}

impl FrontMatter {
    /// Returns the trimmed value of `key`, or `None` when the block has no such key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            (k.trim() == key).then(|| v.trim())
        })
    }

    /// Returns whether the page itself is marked protected.
    ///
    /// Fails closed: once the key is present, any value other than exactly `false` counts as protected.
    pub fn protected(&self) -> bool {
        self.get("protected").is_some_and(|v| v != "false")
    }
}

/// Splits `source` into its front matter and its Markdown body.
///
/// A source that does not open with a `---` line, or whose block is never closed, has no front matter and is all
/// body.
pub fn split_front_matter(source: &str) -> (FrontMatter, &str) {
    let Some(rest) = source.strip_prefix("---\n").or_else(|| source.strip_prefix("---\r\n")) else {
        return (FrontMatter::default(), source);
    };
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            return (FrontMatter { lines }, &rest[offset..]);
        }
        lines.push(trimmed.to_owned());
    }
    (FrontMatter::default(), source)
}

/// Returns the page's title: the text of its first level-1 heading, or [`title_from_name`] when it has none.
pub fn title(body: &str, name: &str) -> String {
    let arena = Arena::new();
    let root = parse_document(&arena, body, &Options::default());
    for node in root.descendants() {
        if let NodeValue::Heading(heading) = &node.data.borrow().value
            && heading.level == 1
        {
            let text = collect_text(node);
            if !text.trim().is_empty() {
                return text.trim().to_owned();
            }
        }
    }
    title_from_name(name)
}

/// Turns a page name into a title: `wild-garlic` becomes "Wild garlic".
pub fn title_from_name(name: &str) -> String {
    let spaced = name.replace('-', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn collect_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut text = String::new();
    for child in node.descendants() {
        match &child.data.borrow().value {
            NodeValue::Text(t) => text.push_str(t),
            NodeValue::Code(code) => text.push_str(&code.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => text.push(' '),
            _ => {}
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_front_matter_from_the_body() {
        let (front_matter, body) = split_front_matter("---\nprotected: true\n---\n# Chanterelle\n");
        assert_eq!(front_matter.get("protected"), Some("true"));
        assert_eq!(body, "# Chanterelle\n");
    }

    #[test]
    fn accepts_windows_line_endings() {
        let (front_matter, body) = split_front_matter("---\r\nprotected: true\r\n---\r\nBody");
        assert!(front_matter.protected());
        assert_eq!(body, "Body");
    }

    #[test]
    fn treats_a_source_without_a_block_as_all_body() {
        let (front_matter, body) = split_front_matter("# Title\n---\n");
        assert_eq!(front_matter, FrontMatter::default());
        assert_eq!(body, "# Title\n---\n");
    }

    #[test]
    fn treats_an_unclosed_block_as_all_body() {
        let source = "---\nprotected: true\n# Title\n";
        let (front_matter, body) = split_front_matter(source);
        assert!(!front_matter.protected());
        assert_eq!(body, source);
    }

    #[test]
    fn protects_any_value_except_false() {
        for value in ["true", "yes", "True", "", "no", "FALSE"] {
            let source = format!("---\nprotected: {value}\n---\n");
            assert!(split_front_matter(&source).0.protected(), "{value:?}");
        }
    }

    #[test]
    fn leaves_pages_without_the_key_or_set_to_false_public() {
        assert!(!split_front_matter("---\nother: 1\n---\n").0.protected());
        assert!(!split_front_matter("---\nprotected: false\n---\n").0.protected());
        assert!(!split_front_matter("no front matter").0.protected());
    }

    #[test]
    fn takes_the_title_from_the_first_level_one_heading() {
        assert_eq!(
            title("Intro\n\n## Sub\n\n# The *real* `title`\n\n# Second", "x"),
            "The real title"
        );
    }

    #[test]
    fn ignores_headings_inside_code_blocks() {
        assert_eq!(title("```\n# not a title\n```\n", "wild-garlic"), "Wild garlic");
    }

    #[test]
    fn falls_back_to_the_name() {
        assert_eq!(title("No heading here", "wild-garlic"), "Wild garlic");
        assert_eq!(title_from_name("2024-notes"), "2024 notes");
    }
}
