//! The data folder: where pages, uploads, the cache and config live, how URLs map onto them, and the page tree.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::page;

/// The name the home page's Markdown file has at the top of `pages/`.
const HOME_NAME: &str = "index";

/// The location of a page or group in the tree, as a list of validated path segments.
///
/// An empty path is the home page. Every segment matches `[a-z0-9][a-z0-9-]*`, so a `PagePath` can never point
/// outside `pages/` and never collides with a dotfile.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PagePath(Vec<String>);

impl PagePath {
    /// Returns the home page's path.
    pub fn home() -> Self {
        PagePath(Vec::new())
    }

    /// Parses the part of a URL after the leading `/`.
    ///
    /// Returns `None` when any segment is not a valid page name, when the path has empty segments (a trailing or
    /// doubled `/`), or when it names the reserved top-level `index`.
    pub fn parse(url_path: &str) -> Option<Self> {
        if url_path.is_empty() {
            return Some(Self::home());
        }
        let segments: Vec<String> = url_path.split('/').map(str::to_owned).collect();
        if !segments.iter().all(|s| is_valid_name(s)) || segments[0] == HOME_NAME {
            return None;
        }
        Some(PagePath(segments))
    }

    /// Returns `true` for the home page.
    pub fn is_home(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the path's segments, outermost first.
    pub fn segments(&self) -> &[String] {
        &self.0
    }

    /// Returns the last segment, or `None` for the home page.
    pub fn name(&self) -> Option<&str> {
        self.0.last().map(String::as_str)
    }

    /// Returns a new path with `name` appended.
    pub fn child(&self, name: &str) -> Self {
        let mut segments = self.0.clone();
        segments.push(name.to_owned());
        PagePath(segments)
    }

    /// Returns the URL the page is served at, such as `/mushroom/chanterelle`, or `/` for the home page.
    pub fn url(&self) -> String {
        format!("/{}", self.0.join("/"))
    }

    /// Returns `true` when `self` is `other` or one of its subpages.
    pub fn starts_with(&self, other: &PagePath) -> bool {
        self.0.starts_with(&other.0)
    }

    /// Returns the path of every page above this one, nearest first, ending with the top-level ancestor.
    ///
    /// The home page is not included: top-level pages sit next to it, not under it.
    pub fn ancestors(&self) -> Vec<PagePath> {
        (1..self.0.len())
            .rev()
            .map(|n| PagePath(self.0[..n].to_vec()))
            .collect()
    }

    fn relative(&self) -> PathBuf {
        self.0.iter().collect()
    }
}

impl fmt::Display for PagePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.url())
    }
}

/// Returns `true` when `name` can be a page or group name: `[a-z0-9][a-z0-9-]*`.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The data folder (C02) and the folders inside it.
#[derive(Clone, Debug)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    /// Wraps `root` without touching the disk; call [`DataDir::init`] to create the folders.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        DataDir { root: root.into() }
    }

    /// Creates any of `pages/`, `uploads/`, `cache/` and `config/` that are missing.
    pub fn init(&self) -> io::Result<()> {
        for dir in [self.pages(), self.uploads(), self.cache(), self.config()] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// Returns the data folder itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns `pages/`, the source of truth and the tree (C03, C05).
    pub fn pages(&self) -> PathBuf {
        self.root.join("pages")
    }

    /// Returns `uploads/`, the flat folder of uploaded assets.
    pub fn uploads(&self) -> PathBuf {
        self.root.join("uploads")
    }

    /// Returns `cache/`, the cached assets that are safe to delete at any time (C06).
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Returns `config/`, which holds the secret key and the custom CSS.
    pub fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    /// Returns the Markdown file of the page at `path`, whether or not it exists.
    pub fn page_file(&self, path: &PagePath) -> PathBuf {
        match path.name() {
            None => self.pages().join(format!("{HOME_NAME}.md")),
            Some(name) => self.page_folder(path).with_file_name(format!("{name}.md")),
        }
    }

    /// Returns the folder that holds the subpages of `path`, whether or not it exists.
    ///
    /// For the home page that is `pages/` itself.
    pub fn page_folder(&self, path: &PagePath) -> PathBuf {
        self.pages().join(path.relative())
    }

    /// Returns the cached rendered body of the page at `path`, whether or not it exists.
    pub fn cached_body(&self, path: &PagePath) -> PathBuf {
        let relative = match path.name() {
            None => PathBuf::from(HOME_NAME),
            Some(_) => path.relative(),
        };
        self.cache().join("pages").join(relative).with_extension("html")
    }

    /// Returns what is at `path` on disk: a page, a group, or nothing.
    pub fn kind(&self, path: &PagePath) -> Option<Kind> {
        if self.page_file(path).is_file() {
            Some(Kind::Page)
        } else if !path.is_home() && self.page_folder(path).is_dir() {
            Some(Kind::Group)
        } else {
            None
        }
    }
}

/// What a tree entry is on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A `.md` file, possibly with a folder of subpages next to it.
    Page,
    /// A folder with no `.md` of its own.
    Group,
}

/// One page or group in the navigation tree.
#[derive(Clone, Debug)]
pub struct TreeNode {
    /// Where the entry is.
    pub path: PagePath,
    /// The title shown in the navigation.
    pub title: String,
    /// Whether this is a page or a group.
    pub kind: Kind,
    /// Whether the page, or a page above it, is protected. Drives the lock icon only, never access.
    pub protected: bool,
    /// Subpages and subgroups, sorted by title.
    pub children: Vec<TreeNode>,
}

/// The navigation tree, built by walking `pages/` (C05).
#[derive(Clone, Debug, Default)]
pub struct Tree {
    /// The home page's title, or `None` when `index.md` is missing. The home page is not part of [`Tree::roots`].
    pub home_title: Option<String>,
    /// The top-level pages and groups.
    pub roots: Vec<TreeNode>,
}

impl Tree {
    /// Walks `pages/` and builds the tree. Unreadable pages are listed with their name as their title.
    pub fn scan(data: &DataDir) -> io::Result<Self> {
        let home_title = fs::read_to_string(data.page_file(&PagePath::home()))
            .ok()
            .map(|source| page::title(page::split_front_matter(&source).1, "home"));
        Ok(Tree {
            home_title,
            roots: scan_folder(data, &PagePath::home(), false)?,
        })
    }

    /// Returns the title of the entry at `path`, if the tree has it.
    pub fn title(&self, path: &PagePath) -> Option<&str> {
        if path.is_home() {
            return self.home_title.as_deref();
        }
        let mut nodes = &self.roots;
        let mut found = None;
        for depth in 1..=path.segments().len() {
            let prefix = PagePath(path.segments()[..depth].to_vec());
            let node = nodes.iter().find(|n| n.path == prefix)?;
            found = Some(node.title.as_str());
            nodes = &node.children;
        }
        found
    }
}

fn scan_folder(data: &DataDir, parent: &PagePath, parent_protected: bool) -> io::Result<Vec<TreeNode>> {
    let folder = data.page_folder(parent);
    let mut names = std::collections::BTreeMap::<String, (bool, bool)>::new();
    for entry in fs::read_dir(&folder)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else { continue };
        let file_type = entry.file_type()?;
        if file_type.is_dir() && is_valid_name(file_name) {
            names.entry(file_name.to_owned()).or_default().1 = true;
        } else if let Some(stem) = file_name.strip_suffix(".md")
            && file_type.is_file()
            && is_valid_name(stem)
        {
            names.entry(stem.to_owned()).or_default().0 = true;
        }
    }
    if parent.is_home() {
        names.remove(HOME_NAME);
    }

    let mut nodes = Vec::with_capacity(names.len());
    for (name, (has_page, has_folder)) in names {
        let path = parent.child(&name);
        let (kind, title, own_protected) = if has_page {
            match fs::read_to_string(data.page_file(&path)) {
                Ok(source) => {
                    let (front_matter, body) = page::split_front_matter(&source);
                    (Kind::Page, page::title(body, &name), front_matter.protected())
                }
                Err(_) => (Kind::Page, page::title_from_name(&name), false),
            }
        } else {
            (Kind::Group, page::title_from_name(&name), false)
        };
        let protected = parent_protected || own_protected;
        let children = if has_folder {
            scan_folder(data, &path, protected)?
        } else {
            Vec::new()
        };
        nodes.push(TreeNode {
            path,
            title,
            kind,
            protected,
            children,
        });
    }
    nodes.sort_by_cached_key(|n| n.title.to_lowercase());
    Ok(nodes)
}

/// Writes `contents` to `path` so a crash can never leave a half-written file.
///
/// The data goes to `.<name>.tmp` in the same folder first and is then renamed over `path`. The parent folder is
/// created when missing.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::other("path has no parent"))?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::other("path has no file name"))?;
    let temp = parent.join(format!(".{name}.tmp"));
    fs::write(&temp, contents)?;
    fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_page_paths() {
        let path = PagePath::parse("mushroom/chanterelle").unwrap();
        assert_eq!(path.segments(), ["mushroom", "chanterelle"]);
        assert_eq!(path.url(), "/mushroom/chanterelle");
    }

    #[test]
    fn parses_the_empty_path_as_home() {
        assert!(PagePath::parse("").unwrap().is_home());
        assert_eq!(PagePath::home().url(), "/");
    }

    #[test]
    fn rejects_traversal_and_separators() {
        for bad in ["..", "a/../b", "a/./b", "a\\b", "a%2Fb", ".hidden", "a//b", "a/", "/a"] {
            assert_eq!(PagePath::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn rejects_uppercase_underscores_and_leading_dashes() {
        for bad in ["Mushroom", "_", "_static", "-a", "a_b", "a b", "é"] {
            assert_eq!(PagePath::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn reserves_index_at_the_top_level_only() {
        assert_eq!(PagePath::parse("index"), None);
        assert!(PagePath::parse("mushroom/index").is_some());
    }

    #[test]
    fn lists_ancestors_nearest_first_without_home() {
        let path = PagePath::parse("a/b/c").unwrap();
        let urls: Vec<String> = path.ancestors().iter().map(PagePath::url).collect();
        assert_eq!(urls, ["/a/b", "/a"]);
        assert!(PagePath::parse("a").unwrap().ancestors().is_empty());
    }

    #[test]
    fn maps_paths_onto_files_and_folders() {
        let data = DataDir::new("/data");
        let path = PagePath::parse("mushroom/chanterelle").unwrap();
        assert_eq!(data.page_file(&path), Path::new("/data/pages/mushroom/chanterelle.md"));
        assert_eq!(data.page_folder(&path), Path::new("/data/pages/mushroom/chanterelle"));
        assert_eq!(
            data.cached_body(&path),
            Path::new("/data/cache/pages/mushroom/chanterelle.html")
        );
        assert_eq!(data.page_file(&PagePath::home()), Path::new("/data/pages/index.md"));
        assert_eq!(
            data.cached_body(&PagePath::home()),
            Path::new("/data/cache/pages/index.html")
        );
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
    fn builds_the_tree_from_files_and_folders() {
        let (_dir, data) = data_with(&[
            ("index.md", "# Home"),
            ("mushroom.md", "# Mushrooms"),
            ("mushroom/chanterelle.md", "# Chanterelle"),
            ("recipes/risotto.md", "# Risotto"),
            (".mushroom.md.tmp", "# Half written"),
            ("Bad Name.md", "# Ignored"),
        ]);
        let tree = Tree::scan(&data).unwrap();

        let titles: Vec<&str> = tree.roots.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(tree.home_title.as_deref(), Some("Home"));
        assert_eq!(titles, ["Mushrooms", "Recipes"]);
        assert_eq!(tree.roots[0].kind, Kind::Page);
        assert_eq!(tree.roots[0].children[0].title, "Chanterelle");
        assert_eq!(tree.roots[1].kind, Kind::Group);
        assert_eq!(tree.roots[1].children[0].path.url(), "/recipes/risotto");
    }

    #[test]
    fn marks_subpages_of_protected_pages_as_protected() {
        let (_dir, data) = data_with(&[
            ("secret.md", "---\nprotected: true\n---\n# Secret"),
            ("secret/inner.md", "# Inner"),
            ("open.md", "# Open"),
        ]);
        let tree = Tree::scan(&data).unwrap();

        let open = &tree.roots[0];
        let secret = &tree.roots[1];
        assert!(!open.protected);
        assert!(secret.protected);
        assert!(secret.children[0].protected);
    }

    #[test]
    fn finds_titles_by_path() {
        let (_dir, data) = data_with(&[
            ("mushroom.md", "# Mushrooms"),
            ("mushroom/chanterelle.md", "# Chanterelle"),
        ]);
        let tree = Tree::scan(&data).unwrap();

        assert_eq!(
            tree.title(&PagePath::parse("mushroom/chanterelle").unwrap()),
            Some("Chanterelle")
        );
        assert_eq!(tree.title(&PagePath::parse("missing").unwrap()), None);
    }

    #[test]
    fn writes_atomically_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("sub/page.md");

        write_atomic(&target, b"first").unwrap();
        write_atomic(&target, b"second").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "second");
        assert!(!dir.path().join("sub/.page.md.tmp").exists());
    }
}
