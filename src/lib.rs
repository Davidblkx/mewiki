//! me-wiki: a personal, self-hosted wiki that keeps its pages as Markdown files on disk.

pub mod config;
pub mod page;
pub mod render;
pub mod routes;
pub mod store;

use std::io;
use std::sync::{Arc, RwLock};

use render::Renderer;
use store::{DataDir, Tree};

/// Everything the request handlers share.
pub struct App {
    /// The data folder.
    pub data: DataDir,
    /// The Markdown renderer.
    pub renderer: Renderer,
    /// The navigation tree, rebuilt after every change made through the app.
    pub tree: RwLock<Tree>,
}

impl App {
    /// Creates the data folder's missing parts and scans the tree.
    pub fn new(data: DataDir) -> io::Result<Arc<Self>> {
        data.init()?;
        let tree = Tree::scan(&data)?;
        Ok(Arc::new(App {
            data,
            renderer: Renderer::new(),
            tree: RwLock::new(tree),
        }))
    }

    /// Returns a copy of the current tree.
    pub fn tree(&self) -> Tree {
        self.tree.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
