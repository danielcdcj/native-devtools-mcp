//! Screenshot cache for storing recent screenshots with metadata.
//!
//! This module provides an LRU cache for screenshots, enabling the `find_image`
//! tool to reference previously captured screenshots by ID rather than requiring
//! base64-encoded image data for every call.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Default maximum number of screenshots to cache.
const DEFAULT_CACHE_SIZE: usize = 10;

/// Metadata associated with a cached screenshot.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ScreenshotMetadata {
    /// Screen-space X origin of the screenshot (top-left), in points.
    pub origin_x: f64,
    /// Screen-space Y origin of the screenshot (top-left), in points.
    pub origin_y: f64,
    /// The backing scale factor (e.g., 2.0 for Retina displays).
    pub scale: f64,
    /// The window ID this screenshot was taken from, if applicable.
    pub window_id: Option<u32>,
    /// Pixel width of the screenshot image.
    pub pixel_width: u32,
    /// Pixel height of the screenshot image.
    pub pixel_height: u32,
}

/// A cached screenshot entry.
#[derive(Debug, Clone)]
pub struct CachedScreenshot {
    /// Raw PNG image data (kept as PNG for accuracy, not re-encoded JPEG).
    pub png_data: Vec<u8>,
    /// Metadata for coordinate conversion.
    pub metadata: ScreenshotMetadata,
    /// Access order for LRU eviction (higher = more recently used).
    access_order: u64,
}

/// LRU cache for screenshots.
///
/// Stores the most recent N screenshots, evicting the least recently used
/// when the cache is full.
pub struct ScreenshotCache {
    /// Map from screenshot ID to cached data.
    entries: HashMap<String, CachedScreenshot>,
    /// Maximum number of entries to store.
    max_size: usize,
    /// Counter for generating unique IDs and tracking access order.
    counter: AtomicU64,
}

impl Default for ScreenshotCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_SIZE)
    }
}

#[allow(dead_code)]
impl ScreenshotCache {
    /// Create a new cache with the specified maximum size.
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_size),
            max_size,
            counter: AtomicU64::new(0),
        }
    }

    /// Store a screenshot in the cache and return its ID.
    ///
    /// If the cache is full, the least recently used entry is evicted.
    pub fn store(&mut self, png_data: Vec<u8>, metadata: ScreenshotMetadata) -> String {
        let id = self.next_id();

        // Evict LRU entry if at capacity
        if self.entries.len() >= self.max_size {
            self.evict_lru();
        }

        let access_order = self.counter.fetch_add(1, Ordering::Relaxed);
        self.entries.insert(
            id.clone(),
            CachedScreenshot {
                png_data,
                metadata,
                access_order,
            },
        );

        id
    }

    /// Retrieve a screenshot by ID, updating its access order.
    ///
    /// Returns `None` if the ID is not found.
    pub fn get(&mut self, id: &str) -> Option<&CachedScreenshot> {
        // Update access order on retrieval
        if let Some(entry) = self.entries.get_mut(id) {
            entry.access_order = self.counter.fetch_add(1, Ordering::Relaxed);
        }
        self.entries.get(id)
    }

    /// Get a screenshot without updating access order (for read-only access).
    pub fn peek(&self, id: &str) -> Option<&CachedScreenshot> {
        self.entries.get(id)
    }

    /// Check if a screenshot ID exists in the cache.
    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    /// Clear all entries from the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get the current number of cached screenshots.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Generate the next unique screenshot ID.
    fn next_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("screenshot-{}", n)
    }

    /// Evict the least recently used entry.
    fn evict_lru(&mut self) {
        if let Some((lru_id, _)) = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.access_order)
        {
            let lru_id = lru_id.clone();
            self.entries.remove(&lru_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_metadata() -> ScreenshotMetadata {
        ScreenshotMetadata {
            origin_x: 0.0,
            origin_y: 0.0,
            scale: 2.0,
            window_id: None,
            pixel_width: 100,
            pixel_height: 100,
        }
    }

    #[test]
    fn test_store_and_get() {
        let mut cache = ScreenshotCache::new(5);
        let data = vec![1, 2, 3, 4];
        let id = cache.store(data.clone(), make_metadata());

        let retrieved = cache.get(&id).unwrap();
        assert_eq!(retrieved.png_data, data);
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = ScreenshotCache::new(3);

        let id1 = cache.store(vec![1], make_metadata());
        let id2 = cache.store(vec![2], make_metadata());
        let id3 = cache.store(vec![3], make_metadata());

        assert_eq!(cache.len(), 3);

        // Access id1 to make it more recently used
        cache.get(&id1);

        // Add a fourth entry, should evict id2 (least recently used)
        let _id4 = cache.store(vec![4], make_metadata());

        assert_eq!(cache.len(), 3);
        assert!(cache.contains(&id1));
        assert!(!cache.contains(&id2)); // evicted
        assert!(cache.contains(&id3));
    }

    #[test]
    fn test_clear() {
        let mut cache = ScreenshotCache::new(5);
        cache.store(vec![1], make_metadata());
        cache.store(vec![2], make_metadata());

        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_unique_ids() {
        let mut cache = ScreenshotCache::new(10);
        let id1 = cache.store(vec![1], make_metadata());
        let id2 = cache.store(vec![2], make_metadata());
        let id3 = cache.store(vec![3], make_metadata());

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_get_nonexistent_id() {
        let mut cache = ScreenshotCache::new(5);
        cache.store(vec![1], make_metadata());

        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_peek_does_not_update_lru() {
        let mut cache = ScreenshotCache::new(3);

        let id1 = cache.store(vec![1], make_metadata());
        let id2 = cache.store(vec![2], make_metadata());
        let id3 = cache.store(vec![3], make_metadata());

        // Peek at id1 (should NOT update LRU order)
        let _ = cache.peek(&id1);

        // Add fourth entry - should still evict id1 since peek doesn't update order
        let _id4 = cache.store(vec![4], make_metadata());

        assert!(!cache.contains(&id1)); // id1 should be evicted
        assert!(cache.contains(&id2));
        assert!(cache.contains(&id3));
    }

    #[test]
    fn test_cache_size_one() {
        let mut cache = ScreenshotCache::new(1);

        let id1 = cache.store(vec![1], make_metadata());
        assert!(cache.contains(&id1));

        let id2 = cache.store(vec![2], make_metadata());
        assert!(!cache.contains(&id1)); // evicted
        assert!(cache.contains(&id2));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_metadata_preserved() {
        let mut cache = ScreenshotCache::new(5);
        let metadata = ScreenshotMetadata {
            origin_x: 100.5,
            origin_y: 200.5,
            scale: 2.5,
            window_id: Some(42),
            pixel_width: 1920,
            pixel_height: 1080,
        };

        let id = cache.store(vec![1, 2, 3], metadata);
        let retrieved = cache.get(&id).unwrap();

        assert!((retrieved.metadata.origin_x - 100.5).abs() < f64::EPSILON);
        assert!((retrieved.metadata.origin_y - 200.5).abs() < f64::EPSILON);
        assert!((retrieved.metadata.scale - 2.5).abs() < f64::EPSILON);
        assert_eq!(retrieved.metadata.window_id, Some(42));
        assert_eq!(retrieved.metadata.pixel_width, 1920);
        assert_eq!(retrieved.metadata.pixel_height, 1080);
    }
}
