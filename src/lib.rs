// Suppress warnings from cocoa/objc crates (deprecated APIs and cfg warnings)
#![allow(deprecated)]

#[cfg(feature = "android")]
pub mod android;
pub mod app_protocol;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod server;
pub mod tools;
#[cfg(target_os = "windows")]
pub mod windows;

// Re-export platform module as `platform` for unified access
#[cfg(target_os = "macos")]
pub use macos as platform;
#[cfg(target_os = "windows")]
pub use windows as platform;
