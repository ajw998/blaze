use std::sync::{Arc, RwLock};

use blaze_engine::Index;
use blaze_indexer::open_or_build_index;
use log::warn;

use crate::config::DaemonConfig;

pub struct DaemonState {
    pub config: DaemonConfig,
    index: RwLock<Arc<Index>>,
}

impl DaemonState {
    pub fn new(config: DaemonConfig) -> anyhow::Result<Self> {
        let (index, warning) = open_or_build_index(&config.root, &config.index_path, true)?;

        if let Some(msg) = warning {
            warn!("{msg}")
        }

        Ok(Self {
            config,
            index: RwLock::new(Arc::new(index)),
        })
    }

    pub fn current_index(&self) -> Arc<Index> {
        self.index.read().unwrap().clone()
    }

    pub fn swap_index(&self, new_index: Index) {
        *self.index.write().unwrap() = Arc::new(new_index);
    }
}
