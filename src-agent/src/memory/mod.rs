//! Memory layer: traits, implementations, and legacy hybrid-search store.

pub mod episodic;
pub mod long_term;
pub mod short_term;
pub mod store;
pub mod traits;

pub mod system_bridge;

// Re-export new trait types
pub use short_term::ShortTermMemory;
pub use system_bridge::MemorySystemBridge;
pub use traits::{MemoryStorage, MemorySystem};

// Re-export legacy store types (backward compatibility)
pub use store::{HybridSearchConfig, MemoryStore};

// Re-export from sibling modules
pub use crate::memory_cache::MemoryCache;
