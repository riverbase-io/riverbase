//! Media file management — OpenDAL-backed storage and metadata stores.

pub mod errors;
pub mod helper;
pub mod metadata;
pub mod model;
pub mod opendal;
pub mod store;

#[cfg(feature = "postgres")]
pub mod postgres;

pub use metadata::MediaMetadataStore;
pub use model::{
    FilesystemConfig, MediaCompressionMethod, MediaEntry, MediaQuery, PutMediaRequest,
};
pub use opendal::OpenDalMediaManager;
pub use store::MediaManager;

#[cfg(feature = "postgres")]
pub use postgres::{establish_media_dbpool, PostgresMediaMetadataStore};
