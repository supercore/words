use anyhow::Result;

// Module definitions
pub mod sm2;
pub mod database;
pub mod commands;
pub mod fsrs;
pub mod algorithm;
pub mod fsrs_simulator;

// Re-exports of commonly used types to improve ergonomics
pub use database::DatabaseManager;
pub use sm2::Flashcard;
pub use fsrs::{FsrsCard, FsrsParameters, MemoryState};

// Constants
pub const DEFAULT_DIFFICULTY: f64 = 5.0;
pub const DEFAULT_STABILITY: f64 = 0.0;
pub const DEFAULT_RETRIEVABILITY: f64 = 1.0;

// Utility functions
pub fn get_current_timestamp() -> Result<i64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs() as i64)
}

// Algorithm identifiers
pub const ALGORITHM_SM2: &str = "sm2";
pub const ALGORITHM_FSRS: &str = "fsrs";

// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
