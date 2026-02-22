// Suppress warnings from cocoa/objc crates (deprecated APIs and cfg warnings)
#![allow(deprecated)]

pub mod android;
pub mod app_protocol;
pub mod cli;
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
