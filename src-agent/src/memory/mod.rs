//! Memory layer: traits, implementations, and legacy hybrid-search store.

pub mod store;
pub mod traits;
pub mod short_term;
pub mod long_term;
pub mod episodic;

// Re-export new trait types
pub use traits::{MemoryStorage, MemorySystem};
pub use short_term::ShortTermMemory;

// Re-export legacy store types (backward compatibility)
pub use store::{HybridSearchConfig, MemoryStore};

// Re-export from sibling modules
pub use crate::memory_cache::MemoryCache;
