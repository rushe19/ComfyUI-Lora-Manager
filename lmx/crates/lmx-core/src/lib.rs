//! lmx core: configuration, sidecar I/O, library scanning, persistent index and queries.

pub mod config;
pub mod library;
pub mod model_type;
pub mod paths;
pub mod record;
pub mod scan;
pub mod sidecar;
pub mod store;

pub use config::Config;
pub use library::{Library, Query, SortKey};
pub use model_type::ModelType;
pub use record::ModelRecord;
