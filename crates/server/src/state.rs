use blaise::{raptor::Allocator, realtime::Realtime, repository::Repository};
use crossbeam_queue::ArrayQueue;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{trace, warn};

pub struct AppState {
    pub gtfs_data_path: PathBuf,
    pub gtfs_rt_data_path: PathBuf,
    pub allocator_count: usize,
    pub repository: RwLock<Option<Repository>>,
    pub realtime: RwLock<Option<Realtime>>,
    pub allocator_pool: RwLock<Option<AllocatorPool>>,
}

pub struct AllocatorPool {
    inner: Arc<ArrayQueue<Allocator>>,
}

impl AllocatorPool {
    pub fn new(capacity: usize, repository: &Repository) -> Self {
        let queue = ArrayQueue::new(capacity);
        for _ in 0..capacity {
            let _ = queue.push(Allocator::new(repository));
        }
        Self {
            inner: Arc::new(queue),
        }
    }

    pub fn get_safe(&self, repository: &Repository) -> AllocatorGuard {
        self.inner
            .pop()
            .map(|alloc| {
                trace!("Reused allocator");
                AllocatorGuard {
                    allocator: Some(alloc),
                    owned: true,
                    pool: self.inner.clone(),
                }
            })
            .unwrap_or_else(|| {
                warn!("Created new (temp) allocator");
                AllocatorGuard {
                    allocator: Some(Allocator::new(repository)),
                    owned: false,
                    pool: self.inner.clone(),
                }
            })
    }
}

pub struct AllocatorGuard {
    pub allocator: Option<Allocator>,
    owned: bool,
    pool: Arc<ArrayQueue<Allocator>>,
}

impl Drop for AllocatorGuard {
    fn drop(&mut self) {
        if let Some(mut alloc) = self.allocator.take()
            && self.owned
        {
            trace!("Returned reused allocator");
            alloc.reset();
            let _ = self.pool.push(alloc);
        }
    }
}
