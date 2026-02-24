//! Instance management for AstrBot.
//!
//! New architecture:
//! - versions/ stores only zip files (core.zip)
//! - instances/{id}/core/ - extracted code for this instance
//! - instances/{id}/venv/ - virtual environment for this instance
//! - instances/{id}/core/data/ - instance data (including data/dist for webui)

mod cleanup;
mod crud;
mod deploy;
pub(crate) mod lifecycle;
mod types;

// Re-export types
pub use types::InstanceStatus;

// Re-export CRUD operations
pub use crud::{create_instance, delete_instance, list_instances, update_instance};

// Re-export cleanup
pub use cleanup::{clear_instance_data, clear_instance_venv, clear_pycache};
