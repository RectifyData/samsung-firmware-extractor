//! Core of the Samsung firmware extractor.
//!
//! * `walker`  finds every `.lz4` image inside zip / tar / tar.md5 firmware, nested ones included.
//! * `extract` decodes selected images to raw `.img` files (LZ4 frame format).
//! * `sparse`  turns Android sparse images into raw images.
//! * `jni`     exposes the above to Kotlin (Jetpack Compose UI) on Android (feature `android`).

pub mod extract;
pub mod frame;
pub mod sparse;
pub mod walker;

#[cfg(all(unix, feature = "android"))]
mod jni; // Android bridge; needs Unix file descriptors

pub use extract::{extract_images, Events};
pub use walker::{list_images, ImageInfo};

use std::sync::atomic::AtomicBool;

/// Set by the UI to stop a running scan or extraction at the next read.
pub static CANCEL: AtomicBool = AtomicBool::new(false);
