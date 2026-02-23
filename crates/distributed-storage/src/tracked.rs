//! Tracked storage with automatic compression, indexing, and audit logging.
//!
//! This module provides a wrapper around any DistributedStore that automatically:
//! - Compresses data using LZ4 for efficient storage
//! - Tracks all writes with block numbers
//! - Maintains indexes for fast lookups
//! - Logs all operations to an audit trail

use async_trait::async_trait;
use lz4_flex::{compress_prepend_size, decompress_size_prepended};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::audit::{AuditEntry, AuditLog};
use crate::error::{StorageError, StorageResult};
use crate::index::{AtomicCounter, IndexDefinition, IndexManager};
use crate::query::{QueryBuilder, QueryResult};
use crate::store::{
    DistributedStore, GetOptions, ListResult, PutOptions, StorageKey, StorageStats, StoredValue,
    ValueMetadata,
};

/// Compression mode for stored values
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionMode {
    /// No compression
    None,
    /// LZ4 fast compression
    #[default]
    Lz4,
}

/// Configuration for tracked storage
#[derive(Clone, Debug)]
pub struct TrackedStorageConfig {
    /// Compression mode to use
    pub compression: CompressionMode,
    /// Minimum size in bytes before compression is applied
    pub compression_threshold: usize,
    /// Whether to enable audit logging
    pub enable_audit: bool,
    /// Whether to enable automatic indexing
    pub enable_indexing: bool,
    /// Validator ID for audit entries
    pub validator_id: String,
}

impl Default for TrackedStorageConfig {
    fn default() -> Self {
        Self {
            compression: CompressionMode::Lz4,
            compression_threshold: 256, // Only compress values > 256 bytes
            enable_audit: true,
            enable_indexing: true,
            validator_id: "unknown".to_string(),
        }
    }
}

/// Header prepended to compressed data
const COMPRESSED_HEADER: &[u8] = b"LZ4\x01";
const HEADER_LEN: usize = 4;

/// Tracked storage wrapper with compression, indexing, and audit
pub struct TrackedStorage {
    /// Inner storage backend
    inner: Arc<dyn DistributedStore>,
    /// Configuration
    config: TrackedStorageConfig,
    /// Current block number (updated externally)
    current_block: AtomicU64,
    /// Current epoch (updated externally)
    current_epoch: AtomicU64,
    /// Index manager
    index_manager: IndexManager,
    /// Audit log
    audit_log: AuditLog,
    /// Atomic counters
    counters: AtomicCounter,
    /// Stats tracking
    stats: RwLock<TrackedStorageStats>,
}

/// Statistics for tracked storage
#[derive(Clone, Debug, Default)]
pub struct TrackedStorageStats {
    /// Total bytes written (uncompressed)
    pub bytes_written_uncompressed: u64,
    /// Total bytes written (compressed)
    pub bytes_written_compressed: u64,
    /// Total writes
    pub total_writes: u64,
    /// Total reads
    pub total_reads: u64,
    /// Compression ratio (compressed/uncompressed)
    pub compression_ratio: f64,
}

impl TrackedStorage {
    /// Create a new tracked storage wrapper
    pub fn new(inner: Arc<dyn DistributedStore>, config: TrackedStorageConfig) -> Self {
        let index_manager = IndexManager::new(Arc::clone(&inner));
        let audit_log = AuditLog::new(Arc::clone(&inner));
        let counters = AtomicCounter::new(Arc::clone(&inner));

        Self {
            inner,
            config,
            current_block: AtomicU64::new(0),
            current_epoch: AtomicU64::new(0),
            index_manager,
            audit_log,
            counters,
            stats: RwLock::new(TrackedStorageStats::default()),
        }
    }

    /// Update the current block number
    pub fn set_block(&self, block: u64) {
        self.current_block.store(block, Ordering::SeqCst);
        self.current_epoch.store(block / 360, Ordering::SeqCst);
    }

    /// Get the current block number
    pub fn current_block(&self) -> u64 {
        self.current_block.load(Ordering::SeqCst)
    }

    /// Get the current epoch
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch.load(Ordering::SeqCst)
    }

    /// Get the index manager
    pub fn index_manager(&self) -> &IndexManager {
        &self.index_manager
    }

    /// Get the audit log
    pub fn audit_log(&self) -> &AuditLog {
        &self.audit_log
    }

    /// Get the counters
    pub fn counters(&self) -> &AtomicCounter {
        &self.counters
    }

    /// Get storage statistics
    pub async fn tracked_stats(&self) -> TrackedStorageStats {
        self.stats.read().await.clone()
    }

    /// Compress data if it exceeds threshold
    fn compress(&self, data: &[u8]) -> Vec<u8> {
        if self.config.compression == CompressionMode::None
            || data.len() < self.config.compression_threshold
        {
            return data.to_vec();
        }

        let compressed = compress_prepend_size(data);

        // Only use compressed if it's actually smaller
        if compressed.len() + HEADER_LEN < data.len() {
            let mut result = Vec::with_capacity(HEADER_LEN + compressed.len());
            result.extend_from_slice(COMPRESSED_HEADER);
            result.extend_from_slice(&compressed);
            result
        } else {
            data.to_vec()
        }
    }

    /// Decompress data if it has compression header
    fn decompress(&self, data: &[u8]) -> StorageResult<Vec<u8>> {
        if data.len() > HEADER_LEN && &data[..HEADER_LEN] == COMPRESSED_HEADER {
            decompress_size_prepended(&data[HEADER_LEN..])
                .map_err(|e| StorageError::InvalidData(format!("Decompression failed: {}", e)))
        } else {
            Ok(data.to_vec())
        }
    }

    /// Compute hash of data
    fn hash_data(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    /// Create an index for a namespace
    pub async fn create_index(
        &self,
        namespace: &str,
        index_name: &str,
        key_extractor: &str,
        unique: bool,
    ) -> StorageResult<()> {
        let def = IndexDefinition {
            name: index_name.to_string(),
            namespace: namespace.to_string(),
            key_extractor: key_extractor.to_string(),
            unique,
            created_block: self.current_block(),
        };
        self.index_manager.create_index(def).await
    }

    /// Increment a counter
    pub async fn increment_counter(&self, namespace: &str, name: &str) -> StorageResult<i64> {
        self.counters.increment(namespace, name, 1).await
    }

    /// Decrement a counter
    pub async fn decrement_counter(&self, namespace: &str, name: &str) -> StorageResult<i64> {
        self.counters.increment(namespace, name, -1).await
    }

    /// Get a counter value
    pub async fn get_counter(&self, namespace: &str, name: &str) -> StorageResult<u64> {
        self.counters.get(namespace, name).await
    }
}

#[async_trait]
impl DistributedStore for TrackedStorage {
    async fn get(
        &self,
        key: &StorageKey,
        options: GetOptions,
    ) -> StorageResult<Option<StoredValue>> {
        let result = self.inner.get(key, options).await?;

        if let Some(mut stored) = result {
            // Decompress data
            stored.data = self.decompress(&stored.data)?;

            // Update stats
            {
                let mut stats = self.stats.write().await;
                stats.total_reads += 1;
            }

            return Ok(Some(stored));
        }

        Ok(None)
    }

    async fn put(
        &self,
        key: StorageKey,
        value: Vec<u8>,
        options: PutOptions,
    ) -> StorageResult<ValueMetadata> {
        let block = options.block_id.unwrap_or_else(|| self.current_block());
        let uncompressed_len = value.len();

        // Get existing value for audit trail
        let old_hash = if self.config.enable_audit {
            self.inner
                .get(&key, GetOptions::default())
                .await?
                .map(|v| Self::hash_data(&v.data))
        } else {
            None
        };

        // Compress data
        let compressed = self.compress(&value);
        let compressed_len = compressed.len();

        // Create options with block tracking
        let mut put_options = options.clone();
        put_options.block_id = Some(block);

        // Write to inner storage
        let metadata = self.inner.put(key.clone(), compressed, put_options).await?;

        // Update indexes if enabled
        if self.config.enable_indexing {
            if let Err(e) = self.index_manager.on_write(&key, &value, block).await {
                warn!(key = %key, error = %e, "Failed to update indexes");
            }
        }

        // Write audit entry if enabled
        if self.config.enable_audit {
            let new_hash = Self::hash_data(&value);
            let entry = if let Some(prev_hash) = old_hash {
                AuditEntry::update(
                    block,
                    key.clone(),
                    prev_hash,
                    new_hash,
                    self.config.validator_id.clone(),
                )
            } else {
                AuditEntry::create(
                    block,
                    key.clone(),
                    new_hash,
                    self.config.validator_id.clone(),
                )
            };

            if let Err(e) = self.audit_log.append(entry).await {
                warn!(key = %key, error = %e, "Failed to write audit entry");
            }
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.bytes_written_uncompressed += uncompressed_len as u64;
            stats.bytes_written_compressed += compressed_len as u64;
            stats.total_writes += 1;
            if stats.bytes_written_uncompressed > 0 {
                stats.compression_ratio =
                    stats.bytes_written_compressed as f64 / stats.bytes_written_uncompressed as f64;
            }
        }

        debug!(
            key = %key,
            uncompressed = uncompressed_len,
            compressed = compressed_len,
            block = block,
            "TrackedStorage::put completed"
        );

        Ok(metadata)
    }

    async fn delete(&self, key: &StorageKey) -> StorageResult<bool> {
        let block = self.current_block();

        // Get existing value for audit trail
        let old_hash = if self.config.enable_audit {
            self.inner
                .get(key, GetOptions::default())
                .await?
                .map(|v| Self::hash_data(&v.data))
        } else {
            None
        };

        let deleted = self.inner.delete(key).await?;

        // Write audit entry if deleted and audit enabled
        if deleted && self.config.enable_audit {
            if let Some(hash) = old_hash {
                let entry =
                    AuditEntry::delete(block, key.clone(), hash, self.config.validator_id.clone());
                if let Err(e) = self.audit_log.append(entry).await {
                    warn!(key = %key, error = %e, "Failed to write delete audit entry");
                }
            }
        }

        Ok(deleted)
    }

    async fn exists(&self, key: &StorageKey) -> StorageResult<bool> {
        self.inner.exists(key).await
    }

    async fn list_prefix(
        &self,
        namespace: &str,
        prefix: Option<&[u8]>,
        limit: usize,
        continuation_token: Option<&[u8]>,
    ) -> StorageResult<ListResult> {
        self.inner
            .list_prefix(namespace, prefix, limit, continuation_token)
            .await
    }

    async fn stats(&self) -> StorageResult<StorageStats> {
        self.inner.stats().await
    }

    async fn list_before_block(
        &self,
        namespace: &str,
        block_id: u64,
        limit: usize,
    ) -> StorageResult<QueryResult> {
        self.inner
            .list_before_block(namespace, block_id, limit)
            .await
    }

    async fn list_after_block(
        &self,
        namespace: &str,
        block_id: u64,
        limit: usize,
    ) -> StorageResult<QueryResult> {
        self.inner
            .list_after_block(namespace, block_id, limit)
            .await
    }

    async fn list_range(
        &self,
        namespace: &str,
        start_block: u64,
        end_block: u64,
        limit: usize,
    ) -> StorageResult<QueryResult> {
        self.inner
            .list_range(namespace, start_block, end_block, limit)
            .await
    }

    async fn count_by_namespace(&self, namespace: &str) -> StorageResult<u64> {
        self.inner.count_by_namespace(namespace).await
    }

    async fn query(&self, query: QueryBuilder) -> StorageResult<QueryResult> {
        self.inner.query(query).await
    }

    async fn put_with_block(
        &self,
        key: StorageKey,
        value: Vec<u8>,
        block_id: u64,
        options: PutOptions,
    ) -> StorageResult<ValueMetadata> {
        let mut opts = options;
        opts.block_id = Some(block_id);
        self.put(key, value, opts).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression() {
        let config = TrackedStorageConfig {
            compression: CompressionMode::Lz4,
            compression_threshold: 10,
            ..Default::default()
        };

        let storage = crate::local::LocalStorage::in_memory("test".to_string()).unwrap();
        let tracked = TrackedStorage::new(Arc::new(storage), config);

        // Small data - no compression
        let small = b"hello";
        let result = tracked.compress(small);
        assert_eq!(result, small.to_vec());

        // Large repetitive data - should compress
        let large = vec![b'a'; 1000];
        let compressed = tracked.compress(&large);
        assert!(compressed.len() < large.len());

        // Should decompress back
        let decompressed = tracked.decompress(&compressed).unwrap();
        assert_eq!(decompressed, large);
    }

    #[test]
    fn test_hash() {
        let data = b"test data";
        let hash = TrackedStorage::hash_data(data);
        assert_eq!(hash.len(), 32);

        // Same data should produce same hash
        let hash2 = TrackedStorage::hash_data(data);
        assert_eq!(hash, hash2);
    }
}
