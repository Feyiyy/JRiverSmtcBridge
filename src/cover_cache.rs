use crate::config::get_app_dir;
use std::collections::VecDeque;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

const MAX_CACHE_BYTES: usize = 10 * 1024 * 1024; // 10 MB

const DEFAULT_COVER_FALLBACK: &[u8] = include_bytes!("../Assets/default_cover.jpg");

struct CacheEntry {
    key: String,
    data: Vec<u8>,
}

pub struct CoverCache {
    entries: VecDeque<CacheEntry>,
    total_bytes: usize,
}

impl CoverCache {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            total_bytes: 0,
        }
    }

    pub fn get(&mut self, key: &str) -> Option<Vec<u8>> {
        if let Some(pos) = self.entries.iter().position(|e| e.key == key) {
            let entry = self.entries.remove(pos).unwrap();
            let data = entry.data.clone();
            self.entries.push_back(entry);
            Some(data)
        } else {
            None
        }
    }

    pub fn put(&mut self, key: String, data: Vec<u8>) {
        let entry_size = data.len();
        if entry_size > MAX_CACHE_BYTES {
            return;
        }

        if let Some(pos) = self.entries.iter().position(|e| e.key == key) {
            let old = self.entries.remove(pos).unwrap();
            self.total_bytes = self.total_bytes.saturating_sub(old.data.len());
        }

        while self.total_bytes + entry_size > MAX_CACHE_BYTES && !self.entries.is_empty() {
            if let Some(oldest) = self.entries.pop_front() {
                self.total_bytes = self.total_bytes.saturating_sub(oldest.data.len());
            }
        }

        self.total_bytes += entry_size;
        self.entries.push_back(CacheEntry { key, data });
    }
}

static GLOBAL_COVER_CACHE: Mutex<Option<CoverCache>> = Mutex::new(None);

pub fn get_cached_cover(key: &str) -> Option<Vec<u8>> {
    let mut lock = GLOBAL_COVER_CACHE.lock().unwrap();
    if lock.is_none() {
        *lock = Some(CoverCache::new());
    }
    lock.as_mut().unwrap().get(key)
}

pub fn put_cached_cover(key: String, data: Vec<u8>) {
    let mut lock = GLOBAL_COVER_CACHE.lock().unwrap();
    if lock.is_none() {
        *lock = Some(CoverCache::new());
    }
    lock.as_mut().unwrap().put(key, data);
}

pub fn get_default_cover() -> Vec<u8> {
    let app_dir = get_app_dir();
    let external_path = app_dir.join("Assets").join("default_cover.jpg");
    if external_path.exists() {
        if let Ok(bytes) = fs::read(&external_path) {
            if !bytes.is_empty() {
                return bytes;
            }
        }
    }
    DEFAULT_COVER_FALLBACK.to_vec()
}

pub fn export_current_cover_atomic(bytes: &[u8]) {
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        let dir = Path::new(&local_app_data).join("JRiverSmtcBridge");
        if fs::create_dir_all(&dir).is_err() {
            return;
        }

        let target_file = dir.join("current_cover.jpg");
        let tmp_file = dir.join("current_cover.jpg.tmp");

        if fs::write(&tmp_file, bytes).is_ok() {
            let _ = fs::rename(&tmp_file, &target_file);
        }
    }
}
