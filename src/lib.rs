//! Log Scout Analyzer - LSP Server Library
//!
//! Core modules for the Language Server Protocol implementation.

pub mod config;
pub mod diagnostics;
pub mod document;
pub mod pattern_engine;
pub mod server;
pub mod tagscout;

pub use server::LogScoutServer;
