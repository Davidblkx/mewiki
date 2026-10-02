//! What a page's Markdown source holds besides its body: the front-matter block and the title.

use std::fs;
use std::io;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, Options, parse_document};

use crate::store::{DataDir, PagePath};

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

    /// Marks the page protected, or removes the key so the page is public. Other keys stay as they are.
    pub fn set_protected(&mut self, protected: bool) {
        let position = self
            .lines
            .iter()
            .position(|line| line.split_once(':').is_some_and(|(k, _)| k.trim() == "protected"));
        match (protected, position) {
            (true, Some(i)) => self.lines[i] = "protected: true".to_owned(),
            (true, None) => self.lines.push("protected: true".to_owned()),
            (false, Some(i)) => {
                self.lines.remove(i);
            }
            (false, None) => {}
        }
    }

    /// Joins the block and `body` back into a page source. A block with no lines is left out.
    pub fn to_source(&self, body: &str) -> String {
        if self.lines.is_empty() {
            return body.to_owned();
        }
        format!(
            "---
{}
---
{body}",
            self.lines.join(
                "
"
            )
        )
    }
}

/// Returns whether the page at `path` is protected: its own front matter, or that of any page above it, says so.
///
/// Reads every front-matter block from disk on each call, never from the tree, so a page protected by editing its
/// file outside the app is hidden right away. Missing files along the way, such as groups, don't protect anything.
pub fn is_protected(data: &DataDir, path: &PagePath) -> io::Result<bool> {
    for candidate in std::iter::once(path.clone()).chain(path.ancestors()) {
        match fs::read_to_string(data.page_file(&candidate)) {
            Ok(source) if split_front_matter(&source).0.protected() => return Ok(true),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(false)
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
    fn sets_and_clears_protection_keeping_other_keys() {
        let (mut front_matter, body) = split_front_matter(
            "---
tags: fungi
---
# Body
",
        );

        front_matter.set_protected(true);
        assert_eq!(
            front_matter.to_source(body),
            "---
tags: fungi
protected: true
---
# Body
"
        );

        front_matter.set_protected(false);
        assert_eq!(
            front_matter.to_source(body),
            "---
tags: fungi
---
# Body
"
        );
    }

    #[test]
    fn replaces_an_odd_protected_value() {
        let (mut front_matter, body) = split_front_matter(
            "---
protected: yes
---
x",
        );
        front_matter.set_protected(true);
        assert_eq!(
            front_matter.to_source(body),
            "---
protected: true
---
x"
        );
    }

    #[test]
    fn drops_an_empty_block() {
        let (mut front_matter, body) = split_front_matter(
            "---
protected: true
---
x",
        );
        front_matter.set_protected(false);
        assert_eq!(front_matter.to_source(body), "x");
    }

    fn data_with(files: &[(&str, &str)]) -> (tempfile::TempDir, DataDir) {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir::new(dir.path());
        data.init().unwrap();
        for (file, contents) in files {
            let path = data.pages().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        (dir, data)
    }

    #[test]
    fn inherits_protection_from_pages_above() {
        let (_dir, data) = data_with(&[
            (
                "secret.md",
                "---
protected: true
---
",
            ),
            ("secret/group/deep.md", "# Deep"),
            ("open.md", "# Open"),
        ]);
        let protected = |p: &str| is_protected(&data, &PagePath::parse(p).unwrap()).unwrap();

        assert!(protected("secret"));
        assert!(protected("secret/group"));
        assert!(protected("secret/group/deep"));
        assert!(!protected("open"));
        assert!(!protected("missing"));
    }

    #[test]
    fn protecting_the_home_page_protects_only_home() {
        let (_dir, data) = data_with(&[
            (
                "index.md",
                "---
protected: true
---
",
            ),
            ("top.md", "# Top"),
        ]);

        assert!(is_protected(&data, &PagePath::home()).unwrap());
        assert!(!is_protected(&data, &PagePath::parse("top").unwrap()).unwrap());
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
