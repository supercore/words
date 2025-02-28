use anyhow::Result;
use clap::Parser;
mod commands;
mod database;
mod flashcard;
mod migration;
use commands::{handle_command, Commands};
use database::DatabaseManager;

#[derive(Parser)]
// #[command(name = "spaced_repetition")]
// #[command(about = "A spaced repetition system for flashcards", long_about = None)]
struct Cli {
    #[arg(short, long, value_name = "FILE", default_value = "flashcards.db")]
    db_file: String,

    #[command(subcommand)]
    command: Commands,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let db_manager = DatabaseManager::new(&cli.db_file)?;

    handle_command(&cli.command, &db_manager)?;

    Ok(())
}
