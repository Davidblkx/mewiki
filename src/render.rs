//! Markdown to HTML, and the on-disk cache of rendered page bodies (C03, C06).

use std::fmt;
use std::fs;
use std::io;
use std::time::SystemTime;

use comrak::adapters::CodefenceRendererAdapter;
use comrak::nodes::Sourcepos;
use comrak::options::Plugins;
use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};
use comrak::{Options, markdown_to_html_with_plugins};

use crate::page;
use crate::store::{self, DataDir, PagePath};

/// Turns page bodies into HTML. Building one loads syntect's syntax definitions, so build it once and share it.
pub struct Renderer {
    highlighter: SyntectAdapter,
    options: Options<'static>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    /// Builds a renderer with tables, task lists, strikethrough and autolinks on, raw HTML escaped, and code
    /// highlighted with CSS classes so the theme lives in the stylesheet (R01, R02).
    pub fn new() -> Self {
        let mut options = markdown_options();
        options.render.escape = true;
        Renderer {
            highlighter: SyntectAdapterBuilder::new().css().build(),
            options,
        }
    }

    /// Renders a page body, without its front matter, to an HTML fragment.
    pub fn render(&self, body: &str) -> String {
        let mut plugins = Plugins::default();
        plugins.render.codefence_syntax_highlighter = Some(&self.highlighter);
        plugins
            .render
            .codefence_renderers
            .insert("mermaid".to_owned(), &MermaidBlock);
        markdown_to_html_with_plugins(body, &self.options, &plugins)
    }

    /// Returns the rendered body of the page at `path`, from the cache when it is fresh.
    ///
    /// A cached body is stale when the page's `.md` was modified after it, so pages edited outside the app are
    /// re-rendered on the next request. Returns `Ok(None)` when the page has no `.md` file.
    pub fn cached_body(&self, data: &DataDir, path: &PagePath) -> io::Result<Option<String>> {
        let source_file = data.page_file(path);
        let source_modified = match fs::metadata(&source_file) {
            Ok(meta) => meta.modified()?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let cache_file = data.cached_body(path);
        if modified(&cache_file).is_some_and(|cached| cached >= source_modified) {
            return fs::read_to_string(&cache_file).map(Some);
        }
        let source = fs::read_to_string(&source_file)?;
        let html = self.render(page::split_front_matter(&source).1);
        store::write_atomic(&cache_file, html.as_bytes())?;
        Ok(Some(html))
    }
}

/// Returns the parsing options every part of the app uses, so links are found wherever the renderer finds them.
pub fn markdown_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options
}

fn modified(path: &std::path::Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Writes ```` ```mermaid ```` blocks as `<pre class="mermaid">` for Mermaid to draw in the browser.
struct MermaidBlock;

impl CodefenceRendererAdapter for MermaidBlock {
    fn write(
        &self,
        output: &mut dyn fmt::Write,
        _lang: &str,
        _meta: &str,
        code: &str,
        _sourcepos: Option<Sourcepos>,
    ) -> fmt::Result {
        output.write_str("<pre class=\"mermaid\">")?;
        comrak::html::escape(output, code)?;
        output.write_str("</pre>\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn renders_tables_and_task_lists() {
        let html = Renderer::new().render("| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n");
        assert!(html.contains("<table>"), "{html}");
        assert!(html.contains("<td>1</td>"), "{html}");
        assert!(html.contains("type=\"checkbox\""), "{html}");
        assert!(html.contains("checked"), "{html}");
    }

    #[test]
    fn highlights_code_with_css_classes() {
        let html = Renderer::new().render("```rust\nfn main() {}\n```\n");
        assert!(html.contains("<pre class=\"syntax-highlighting\">"), "{html}");
        assert!(html.contains("class=\"source rust\""), "{html}");
        assert!(!html.contains("style="), "{html}");
    }

    #[test]
    fn writes_mermaid_blocks_for_the_browser() {
        let html = Renderer::new().render("```mermaid\ngraph TD; A-->B<script>\n```\n");
        assert_eq!(
            html,
            "<pre class=\"mermaid\">graph TD; A--&gt;B&lt;script&gt;\n</pre>\n"
        );
    }

    #[test]
    fn escapes_raw_html() {
        let html = Renderer::new().render("<script>alert(1)</script>\n\nhi <b>there</b>");
        assert!(!html.contains("<script>"), "{html}");
        assert!(!html.contains("<b>"), "{html}");
        assert!(html.contains("&lt;b&gt;"), "{html}");
    }

    fn data_with_page(source: &str) -> (tempfile::TempDir, DataDir, PagePath) {
        let dir = tempfile::tempdir().unwrap();
        let data = DataDir::new(dir.path());
        data.init().unwrap();
        let path = PagePath::parse("note").unwrap();
        fs::write(data.page_file(&path), source).unwrap();
        (dir, data, path)
    }

    #[test]
    fn renders_without_front_matter_and_writes_the_cache() {
        let (_dir, data, path) = data_with_page("---\nprotected: true\n---\n# Note\n");

        let html = Renderer::new().cached_body(&data, &path).unwrap().unwrap();

        assert_eq!(html, "<h1>Note</h1>\n");
        assert_eq!(fs::read_to_string(data.cached_body(&path)).unwrap(), html);
    }

    #[test]
    fn serves_a_fresh_cache_without_rendering() {
        let (_dir, data, path) = data_with_page("# Source");
        let renderer = Renderer::new();
        renderer.cached_body(&data, &path).unwrap();
        fs::write(data.cached_body(&path), "<p>from cache</p>").unwrap();

        assert_eq!(
            renderer.cached_body(&data, &path).unwrap().unwrap(),
            "<p>from cache</p>"
        );
    }

    #[test]
    fn re_renders_when_the_source_is_newer() {
        let (_dir, data, path) = data_with_page("# Old");
        let renderer = Renderer::new();
        renderer.cached_body(&data, &path).unwrap();
        let later = SystemTime::now() + Duration::from_secs(5);
        fs::write(data.page_file(&path), "# New").unwrap();
        fs::File::options()
            .write(true)
            .open(data.page_file(&path))
            .unwrap()
            .set_modified(later)
            .unwrap();

        assert_eq!(renderer.cached_body(&data, &path).unwrap().unwrap(), "<h1>New</h1>\n");
    }

    #[test]
    fn returns_none_for_a_page_without_a_file() {
        let (_dir, data, _) = data_with_page("");
        let missing = PagePath::parse("missing").unwrap();

        assert_eq!(Renderer::new().cached_body(&data, &missing).unwrap(), None);
    }
}
