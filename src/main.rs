use anyhow::Result;
use clap::Parser;
mod algorithm;
mod commands;
mod database;
mod fsrs;
mod fsrs_simulator;
mod sm2; // Add this line to include the simulator

use commands::{handle_command, Commands};
use database::DatabaseManager;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, value_name = "FILE", default_value = "flashcards.db")]
    db_file: String,

    #[command(subcommand)]
    command: Commands,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    println!("Words - Now with FSRS Algorithm Support!");
    println!("Run 'words convert-to-fsrs --help' to learn how to upgrade your decks");

    let mut db_manager = DatabaseManager::new(&cli.db_file)?;
    handle_command(&cli.command, &mut db_manager)?;

    Ok(())
}
