//! Image storage with LRU memory management.
//!
//! Implements a memory-bounded store for decoded images with LRU eviction.
//!
//! # Memory Quota
//!
//! Default quota is 320MB. When quota is exceeded, oldest accessed images
//! are evicted until sufficient space is available.
//!
//! # Example
//!
//! ```
//! use term_images::image_proc::store::ImageStore;
//! use term_images::image_proc::DecodedImage;
//!
//! let mut store = ImageStore::new(1024 * 1024); // 1MB quota
//!
//! let image = DecodedImage {
//!     id: 1,
//!     width: 10,
//!     height: 10,
//!     rgba_data: vec![0; 400],
//!     rgba_base64: String::new(),
//! };
//!
//! store.insert(image).unwrap();
//! assert!(store.get(1).is_some());
//! ```

use std::collections::{HashMap, VecDeque};

use super::DecodedImage;

/// Default memory quota: 320MB.
pub const DEFAULT_QUOTA: usize = 320 * 1024 * 1024;

/// Maximum single image size: 100MB.
pub const MAX_IMAGE_SIZE: usize = 100 * 1024 * 1024;

/// Image storage error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Image exceeds maximum allowed size.
    ImageTooLarge { size: usize, max: usize },
    /// Quota exceeded even after eviction.
    QuotaExceeded { current: usize, max: usize },
    /// Image not found.
    NotFound(u32),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::ImageTooLarge { size, max } => {
                write!(f, "Image size {} exceeds maximum {}", size, max)
            }
            StoreError::QuotaExceeded { current, max } => {
                write!(f, "Quota exceeded: {} / {}", current, max)
            }
            StoreError::NotFound(id) => {
                write!(f, "Image not found: {}", id)
            }
        }
    }
}

impl std::error::Error for StoreError {}

/// Stored image with metadata.
#[derive(Debug, Clone)]
pub struct StoredImage {
    /// Decoded image data.
    pub image: DecodedImage,
    /// Size in bytes (RGBA data length).
    pub size: usize,
}

impl StoredImage {
    /// Create from decoded image.
    pub fn new(image: DecodedImage) -> Self {
        let size = image.rgba_data.len();
        Self { image, size }
    }
}

/// Image storage with LRU eviction.
pub struct ImageStore {
    /// Stored images by ID.
    images: HashMap<u32, StoredImage>,

    /// LRU order (front = oldest, back = newest).
    lru_order: VecDeque<u32>,

    /// Current total size in bytes.
    total_size: usize,

    /// Maximum allowed size in bytes.
    max_size: usize,
}

impl Default for ImageStore {
    fn default() -> Self {
        Self::new(DEFAULT_QUOTA)
    }
}

impl ImageStore {
    /// Create a new store with the specified quota.
    pub fn new(max_size: usize) -> Self {
        Self {
            images: HashMap::new(),
            lru_order: VecDeque::new(),
            total_size: 0,
            max_size,
        }
    }

    /// Insert an image into the store.
    ///
    /// If the image already exists, it will be replaced.
    /// If quota is exceeded, oldest images will be evicted.
    pub fn insert(&mut self, image: DecodedImage) -> Result<(), StoreError> {
        let size = image.rgba_data.len();

        // Check single image size limit
        if size > MAX_IMAGE_SIZE {
            return Err(StoreError::ImageTooLarge {
                size,
                max: MAX_IMAGE_SIZE,
            });
        }

        // If image already exists, remove it first
        if let Some(existing) = self.images.remove(&image.id) {
            self.total_size -= existing.size;
            self.lru_order.retain(|&id| id != image.id);
        }

        // Evict old images until we have space
        while self.total_size + size > self.max_size && !self.lru_order.is_empty() {
            if let Some(oldest_id) = self.lru_order.pop_front() {
                if let Some(removed) = self.images.remove(&oldest_id) {
                    self.total_size -= removed.size;
                    log::debug!("Evicted image {} ({} bytes)", oldest_id, removed.size);
                }
            }
        }

        // Check if we have enough space
        if self.total_size + size > self.max_size {
            return Err(StoreError::QuotaExceeded {
                current: self.total_size + size,
                max: self.max_size,
            });
        }

        // Insert the new image
        let id = image.id;
        let stored = StoredImage::new(image);
        self.total_size += stored.size;
        self.images.insert(id, stored);
        self.lru_order.push_back(id);

        Ok(())
    }

    /// Get an image by ID, updating LRU order.
    pub fn get(&mut self, id: u32) -> Option<&DecodedImage> {
        if self.images.contains_key(&id) {
            // Update LRU order
            self.lru_order.retain(|&i| i != id);
            self.lru_order.push_back(id);

            self.images.get(&id).map(|s| &s.image)
        } else {
            None
        }
    }

    /// Get an image by ID without updating LRU order.
    pub fn peek(&self, id: u32) -> Option<&DecodedImage> {
        self.images.get(&id).map(|s| &s.image)
    }

    /// Check if an image exists.
    pub fn contains(&self, id: u32) -> bool {
        self.images.contains_key(&id)
    }

    /// Remove an image by ID.
    pub fn remove(&mut self, id: u32) -> Option<DecodedImage> {
        if let Some(stored) = self.images.remove(&id) {
            self.total_size -= stored.size;
            self.lru_order.retain(|&i| i != id);
            Some(stored.image)
        } else {
            None
        }
    }

    /// Remove all images.
    pub fn clear(&mut self) {
        self.images.clear();
        self.lru_order.clear();
        self.total_size = 0;
    }

    /// Get current total size in bytes.
    pub fn total_size(&self) -> usize {
        self.total_size
    }

    /// Get maximum allowed size in bytes.
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Get number of stored images.
    pub fn len(&self) -> usize {
        self.images.len()
    }

    /// Check if store is empty.
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Get all image IDs.
    pub fn ids(&self) -> Vec<u32> {
        self.images.keys().copied().collect()
    }

    /// Get usage statistics.
    pub fn stats(&self) -> StoreStats {
        StoreStats {
            image_count: self.images.len(),
            total_size: self.total_size,
            max_size: self.max_size,
            usage_percent: if self.max_size > 0 {
                (self.total_size as f64 / self.max_size as f64) * 100.0
            } else {
                0.0
            },
        }
    }
}

/// Store usage statistics.
#[derive(Debug, Clone)]
pub struct StoreStats {
    /// Number of stored images.
    pub image_count: usize,
    /// Current total size in bytes.
    pub total_size: usize,
    /// Maximum allowed size in bytes.
    pub max_size: usize,
    /// Usage percentage (0-100).
    pub usage_percent: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_image(id: u32, size: usize) -> DecodedImage {
        DecodedImage {
            id,
            width: 1,
            height: 1,
            rgba_data: vec![0; size],
            rgba_base64: String::new(),
        }
    }

    // =========================================================================
    // Basic Operations
    // =========================================================================

    #[test]
    fn test_store_creation() {
        let store = ImageStore::new(1024);
        assert_eq!(store.max_size(), 1024);
        assert_eq!(store.total_size(), 0);
        assert!(store.is_empty());
    }

    #[test]
    fn test_store_default() {
        let store = ImageStore::default();
        assert_eq!(store.max_size(), DEFAULT_QUOTA);
    }

    #[test]
    fn test_insert_and_get() {
        let mut store = ImageStore::new(1024);
        let image = make_image(1, 100);

        store.insert(image).unwrap();

        assert_eq!(store.len(), 1);
        assert_eq!(store.total_size(), 100);
        assert!(store.contains(1));
        assert!(store.get(1).is_some());
        assert_eq!(store.get(1).unwrap().id, 1);
    }

    #[test]
    fn test_insert_replace_existing() {
        let mut store = ImageStore::new(1024);

        store.insert(make_image(1, 100)).unwrap();
        assert_eq!(store.total_size(), 100);

        // Replace with larger image
        store.insert(make_image(1, 200)).unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store.total_size(), 200);
    }

    #[test]
    fn test_peek_does_not_update_lru() {
        let mut store = ImageStore::new(250); // Only space for 2 images

        store.insert(make_image(1, 100)).unwrap();
        store.insert(make_image(2, 100)).unwrap();

        // Peek at image 1 (should not update LRU)
        assert!(store.peek(1).is_some());

        // Insert image 3, which should evict image 1 (oldest)
        store.insert(make_image(3, 100)).unwrap();

        assert!(!store.contains(1)); // Should be evicted
        assert!(store.contains(2));
        assert!(store.contains(3));
    }

    #[test]
    fn test_get_updates_lru() {
        let mut store = ImageStore::new(250); // Only space for 2 images

        store.insert(make_image(1, 100)).unwrap();
        store.insert(make_image(2, 100)).unwrap();

        // Access image 1 (updates LRU)
        assert!(store.get(1).is_some());

        // Insert image 3, which should evict image 2 (now oldest)
        store.insert(make_image(3, 100)).unwrap();

        assert!(store.contains(1)); // Should not be evicted
        assert!(!store.contains(2)); // Should be evicted
        assert!(store.contains(3));
    }

    #[test]
    fn test_remove() {
        let mut store = ImageStore::new(1024);

        store.insert(make_image(1, 100)).unwrap();
        store.insert(make_image(2, 200)).unwrap();

        let removed = store.remove(1);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, 1);
        assert_eq!(store.len(), 1);
        assert_eq!(store.total_size(), 200);
        assert!(!store.contains(1));
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut store = ImageStore::new(1024);
        assert!(store.remove(999).is_none());
    }

    #[test]
    fn test_clear() {
        let mut store = ImageStore::new(1024);

        store.insert(make_image(1, 100)).unwrap();
        store.insert(make_image(2, 200)).unwrap();

        store.clear();

        assert!(store.is_empty());
        assert_eq!(store.total_size(), 0);
    }

    // =========================================================================
    // LRU Eviction
    // =========================================================================

    #[test]
    fn test_lru_eviction_single() {
        let mut store = ImageStore::new(200);

        store.insert(make_image(1, 100)).unwrap();
        store.insert(make_image(2, 100)).unwrap();

        // Store is full, insert third image should evict first
        store.insert(make_image(3, 100)).unwrap();

        assert!(!store.contains(1)); // Evicted
        assert!(store.contains(2));
        assert!(store.contains(3));
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_lru_eviction_multiple() {
        let mut store = ImageStore::new(300);

        store.insert(make_image(1, 100)).unwrap();
        store.insert(make_image(2, 100)).unwrap();
        store.insert(make_image(3, 100)).unwrap();

        // Insert large image, should evict multiple
        store.insert(make_image(4, 200)).unwrap();

        assert!(!store.contains(1)); // Evicted
        assert!(!store.contains(2)); // Evicted
        assert!(store.contains(3));
        assert!(store.contains(4));
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_lru_order_with_access() {
        let mut store = ImageStore::new(300);

        store.insert(make_image(1, 100)).unwrap();
        store.insert(make_image(2, 100)).unwrap();
        store.insert(make_image(3, 100)).unwrap();

        // Access image 1 to make it most recent
        store.get(1);

        // Insert image 4, should evict image 2 (oldest after access)
        store.insert(make_image(4, 100)).unwrap();

        assert!(store.contains(1)); // Recently accessed
        assert!(!store.contains(2)); // Evicted (was oldest)
        assert!(store.contains(3));
        assert!(store.contains(4));
    }

    // =========================================================================
    // Error Cases
    // =========================================================================

    #[test]
    fn test_image_too_large() {
        let mut store = ImageStore::new(1024);
        let image = make_image(1, MAX_IMAGE_SIZE + 1);

        let result = store.insert(image);
        assert!(matches!(result, Err(StoreError::ImageTooLarge { .. })));
    }

    #[test]
    fn test_quota_exceeded_single_image() {
        let mut store = ImageStore::new(100);
        let image = make_image(1, 200);

        let result = store.insert(image);
        assert!(matches!(result, Err(StoreError::QuotaExceeded { .. })));
    }

    #[test]
    fn test_store_error_display() {
        let err = StoreError::ImageTooLarge {
            size: 200,
            max: 100,
        };
        assert!(err.to_string().contains("200"));
        assert!(err.to_string().contains("100"));

        let err = StoreError::NotFound(42);
        assert!(err.to_string().contains("42"));
    }

    // =========================================================================
    // Statistics
    // =========================================================================

    #[test]
    fn test_store_stats() {
        let mut store = ImageStore::new(1000);

        store.insert(make_image(1, 100)).unwrap();
        store.insert(make_image(2, 200)).unwrap();

        let stats = store.stats();
        assert_eq!(stats.image_count, 2);
        assert_eq!(stats.total_size, 300);
        assert_eq!(stats.max_size, 1000);
        assert!((stats.usage_percent - 30.0).abs() < 0.01);
    }

    #[test]
    fn test_ids() {
        let mut store = ImageStore::new(1024);

        store.insert(make_image(1, 100)).unwrap();
        store.insert(make_image(2, 100)).unwrap();
        store.insert(make_image(3, 100)).unwrap();

        let ids = store.ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
    }
}
