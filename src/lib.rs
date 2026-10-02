//! me-wiki: a personal, self-hosted wiki that keeps its pages as Markdown files on disk.

pub mod auth;
pub mod config;
pub mod health;
pub mod page;
pub mod render;
pub mod routes;
pub mod store;

use std::io;
use std::sync::{Arc, RwLock};

use auth::Auth;
use render::Renderer;
use store::{DataDir, Tree};

/// The home page written at startup when `pages/index.md` is missing, so a new wiki has a page to edit and to add
/// pages under.
pub const HOME_PAGE: &str = "# MEWIKI\n";

/// Everything the request handlers share.
pub struct App {
    /// The data folder.
    pub data: DataDir,
    /// The Markdown renderer.
    pub renderer: Renderer,
    /// The navigation tree, rebuilt after every change made through the app.
    pub tree: RwLock<Tree>,
    /// The password and session checks.
    pub auth: Auth,
    /// Held by every change to the data folder, so a move never runs at the same time as a save.
    pub writes: tokio::sync::Mutex<()>,
}

impl App {
    /// Creates the data folder's missing parts, removes temporary files a crash left behind, creates the home page
    /// when there is none, loads or creates the secret key, and scans the tree.
    ///
    /// `cookie_secure` adds the `Secure` attribute to session cookies.
    pub fn new(data: DataDir, password: &str, cookie_secure: bool) -> io::Result<Arc<Self>> {
        data.init()?;
        let removed = store::remove_temp_files(&data)?;
        if removed > 0 {
            tracing::warn!("removed {removed} temporary files left by an interrupted write");
        }
        let home = data.page_file(&store::PagePath::home());
        if !home.exists() {
            store::write_atomic(&home, HOME_PAGE.as_bytes())?;
            tracing::info!("created the home page at {}", home.display());
        }
        let secret = Auth::load_or_create_secret(&data.config())?;
        let tree = Tree::scan(&data)?;
        Ok(Arc::new(App {
            data,
            renderer: Renderer::new(),
            tree: RwLock::new(tree),
            auth: Auth::new(&secret, password, cookie_secure),
            writes: tokio::sync::Mutex::new(()),
        }))
    }

    /// Rescans `pages/` and replaces the tree.
    pub fn rebuild_tree(&self) -> io::Result<()> {
        let tree = Tree::scan(&self.data)?;
        *self.tree.write().unwrap_or_else(|e| e.into_inner()) = tree;
        Ok(())
    }

    /// Returns a copy of the current tree.
    pub fn tree(&self) -> Tree {
        self.tree.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
