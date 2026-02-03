//! Image cache for storing loaded images (templates/masks) with metadata.
//!
//! This module provides an LRU cache for images, enabling the `find_image`
//! tool to reference previously loaded images by ID rather than requiring
//! base64-encoded image data for every call.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Default maximum number of images to cache.
const DEFAULT_CACHE_SIZE: usize = 50;

/// Maximum allowed image file size (10 MB).
pub const MAX_IMAGE_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum allowed image dimensions (width or height).
pub const MAX_IMAGE_DIMENSION: u32 = 8192;

/// Metadata associated with a cached image.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ImageMetadata {
    /// Original file path (if loaded from file).
    pub source_path: Option<String>,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Number of channels (1 = grayscale, 3 = RGB, 4 = RGBA).
    pub channels: u8,
    /// MIME type (e.g., "image/png", "image/jpeg").
    pub mime: String,
    /// SHA-256 hash of the original file bytes (for dedup/debug).
    pub sha256: Option<String>,
    /// Whether this image is intended to be used as a mask.
    pub is_mask: bool,
}

/// A cached image entry.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CachedImage {
    /// Raw image data (PNG format for consistency).
    pub png_data: Vec<u8>,
    /// Metadata for the image.
    pub metadata: ImageMetadata,
    /// Access order for LRU eviction (higher = more recently used).
    access_order: u64,
}

/// LRU cache for images.
///
/// Stores the most recent N images, evicting the least recently used
/// when the cache is full.
pub struct ImageCache {
    /// Map from image ID to cached data.
    entries: HashMap<String, CachedImage>,
    /// Maximum number of entries to store.
    max_size: usize,
    /// Counter for generating unique IDs and tracking access order.
    counter: AtomicU64,
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_SIZE)
    }
}

#[allow(dead_code)]
impl ImageCache {
    /// Create a new cache with the specified maximum size.
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_size),
            max_size,
            counter: AtomicU64::new(0),
        }
    }

    /// Store an image in the cache and return its ID.
    ///
    /// If a prefix is provided, the ID will be formatted as "{prefix}-{n}".
    /// If the cache is full, the least recently used entry is evicted.
    pub fn store(
        &mut self,
        png_data: Vec<u8>,
        metadata: ImageMetadata,
        id_prefix: Option<&str>,
    ) -> String {
        let id = self.next_id(id_prefix);

        // Evict LRU entry if at capacity
        if self.entries.len() >= self.max_size {
            self.evict_lru();
        }

        let access_order = self.counter.fetch_add(1, Ordering::Relaxed);
        self.entries.insert(
            id.clone(),
            CachedImage {
                png_data,
                metadata,
                access_order,
            },
        );

        id
    }

    /// Retrieve an image by ID, updating its access order.
    ///
    /// Returns `None` if the ID is not found.
    pub fn get(&mut self, id: &str) -> Option<&CachedImage> {
        // Update access order on retrieval
        if let Some(entry) = self.entries.get_mut(id) {
            entry.access_order = self.counter.fetch_add(1, Ordering::Relaxed);
        }
        self.entries.get(id)
    }

    /// Get an image without updating access order (for read-only access).
    pub fn peek(&self, id: &str) -> Option<&CachedImage> {
        self.entries.get(id)
    }

    /// Check if an image ID exists in the cache.
    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    /// Clear all entries from the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get the current number of cached images.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Generate the next unique image ID.
    fn next_id(&self, prefix: Option<&str>) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        match prefix {
            Some(p) => format!("{}-{}", p, n),
            None => format!("image-{}", n),
        }
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

    fn make_metadata() -> ImageMetadata {
        ImageMetadata {
            source_path: None,
            width: 100,
            height: 100,
            channels: 4,
            mime: "image/png".to_string(),
            sha256: None,
            is_mask: false,
        }
    }

    #[test]
    fn test_store_and_get() {
        let mut cache = ImageCache::new(5);
        let data = vec![1, 2, 3, 4];
        let id = cache.store(data.clone(), make_metadata(), None);

        let retrieved = cache.get(&id).unwrap();
        assert_eq!(retrieved.png_data, data);
    }

    #[test]
    fn test_store_with_prefix() {
        let mut cache = ImageCache::new(5);
        let id = cache.store(vec![1, 2, 3], make_metadata(), Some("template"));

        assert!(id.starts_with("template-"));
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = ImageCache::new(3);

        let id1 = cache.store(vec![1], make_metadata(), None);
        let id2 = cache.store(vec![2], make_metadata(), None);
        let id3 = cache.store(vec![3], make_metadata(), None);

        assert_eq!(cache.len(), 3);

        // Access id1 to make it more recently used
        cache.get(&id1);

        // Add a fourth entry, should evict id2 (least recently used)
        let _id4 = cache.store(vec![4], make_metadata(), None);

        assert_eq!(cache.len(), 3);
        assert!(cache.contains(&id1));
        assert!(!cache.contains(&id2)); // evicted
        assert!(cache.contains(&id3));
    }

    #[test]
    fn test_clear() {
        let mut cache = ImageCache::new(5);
        cache.store(vec![1], make_metadata(), None);
        cache.store(vec![2], make_metadata(), None);

        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_peek_does_not_update_lru() {
        let mut cache = ImageCache::new(3);

        let id1 = cache.store(vec![1], make_metadata(), None);
        let id2 = cache.store(vec![2], make_metadata(), None);
        let id3 = cache.store(vec![3], make_metadata(), None);

        // Peek at id1 (should NOT update LRU order)
        let _ = cache.peek(&id1);

        // Add fourth entry - should still evict id1 since peek doesn't update order
        let _id4 = cache.store(vec![4], make_metadata(), None);

        assert!(!cache.contains(&id1)); // id1 should be evicted
        assert!(cache.contains(&id2));
        assert!(cache.contains(&id3));
    }
}
