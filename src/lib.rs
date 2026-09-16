//! High-performance traceroute utility written in idiomatic Rust.
//! Designed and documented following Australian English conventions.

#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]

pub mod cli;
pub mod config;
pub mod engine;
pub mod packet;
pub mod service;
pub mod telemetry;

pub const APPLICATION_NAME: &str = "github.com/jimmystewpot/traceroute";
pub const TRACER_NAME_FORMAT: &str = "{}/traceroute";
