use std::thread::{self, JoinHandle};

pub struct ScopedWorker {
    handle: Option<JoinHandle<()>>,
}

impl ScopedWorker {
    pub fn new<F>(f: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        let handle = thread::spawn(f);
        Self {
            handle: Some(handle),
        }
    }

    pub fn empty() -> Self {
        Self { handle: None }
    }

    pub fn join(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    pub fn is_joinable(&self) -> bool {
        self.handle.is_some()
    }
}

impl Default for ScopedWorker {
    fn default() -> Self {
        Self::empty()
    }
}

impl Drop for ScopedWorker {
    fn drop(&mut self) {
        self.join();
    }
}
