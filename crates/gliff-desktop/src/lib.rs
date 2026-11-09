//! Portable session core and window adapter for the Gliff desktop client.
#![deny(unsafe_code)]
pub mod clipboard;
pub mod config;
pub mod geometry;
pub mod input;
#[cfg(any(windows, test))]
pub mod physical_keyboard;
pub mod session;
pub mod ssh;
pub mod ui;
