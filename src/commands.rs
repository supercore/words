use anyhow::Result;
use clap::Subcommand;
use crate::DatabaseManager;
use crate::flashcard::Flashcard;
use crate::migration::Migrator;
use prettytable::{Table, Row, Cell, row};

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new user
    CreateUser {
        username: String,
    },
    /// Create a new deck
    CreateDeck {
        username: String,
        deck_name: String,
    },
    /// Add a new flashcard
    AddFlashcard {
        username: String,
        deck_id: i64,
        question: String,
        answer: String,
        guidance: String,
    },
    /// Review due flashcards
    Review {
        username: String,
        deck_id: i64,
        batch_size: usize,
    },
    /// Get review statistics
    Stats {
        username: String,
        deck_id: i64,
    },
    /// Migrate data from JSON file
    Migrate {
        json_path: String,
        username: String,
        deck_name: String,
    },
    /// List all users and their decks
    ListUsers,
    /// Import flashcards from a CSV file
    ImportCsv {
        username: String,
        deck_name: String,
        csv_path: String,
    },
}

pub fn handle_command(command: &Commands, db_manager: &DatabaseManager) -> Result<()> {
    match command {
        Commands::CreateUser { username } => {
            db_manager.create_user(username)?;
            println!("User '{}' created successfully!", username);
        }
        Commands::CreateDeck { username, deck_name } => {
            if let Some(user_id) = db_manager.authenticate_user(username)? {
                db_manager.create_deck(user_id, deck_name)?;
                println!("Deck '{}' created successfully for user '{}'", deck_name, username);
            } else {
                println!("User '{}' not found!", username);
            }
        }
        Commands::AddFlashcard { username, deck_id, question, answer, guidance } => {
            if let Some(user_id) = db_manager.authenticate_user(username)? {
                let card = Flashcard::new(question.clone(), answer.clone(), guidance.clone());
                db_manager.add_flashcard(*deck_id, user_id, &card)?;
                println!("Flashcard added successfully to deck '{}'", deck_id);
            } else {
                println!("User '{}' not found!", username);
            }
        }
        Commands::Review { username, deck_id, batch_size } => {
            if let Some(user_id) = db_manager.authenticate_user(username)? {
                let cards = db_manager.get_due_flashcards(*deck_id)?;
                let mut card_iter = cards.into_iter().peekable();
                while card_iter.peek().is_some() {
                    for (_i, (card_id, card)) in card_iter.by_ref().take(*batch_size).enumerate() {
                        println!("Question: {}", card.question);
                        println!("Hint: {}", card.guidance);
                        
                        let mut input = String::new();
                        std::io::stdin().read_line(&mut input)?;
                        
                        println!("Answer: {}", card.answer);
                        println!("How well did you remember? (0-5):");
                        
                        let mut performance = String::new();
                        std::io::stdin().read_line(&mut performance)?;
                        let performance: i32 = performance.trim().parse()
                            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid performance rating"))?;
                        
                        db_manager.update_flashcard(user_id, card_id, performance)?;
                    }
                    if card_iter.peek().is_some() {
                        println!("Do you want to continue reviewing more flashcards? (y/n):");
                        let mut continue_input = String::new();
                        std::io::stdin().read_line(&mut continue_input)?;
                        if continue_input.trim().to_lowercase() != "y" {
                            break;
                        }
                    }
                }
            } else {
                println!("User '{}' not found!", username);
            }
        }
        Commands::Stats { username, deck_id } => {
            if let Some(user_id) = db_manager.authenticate_user(username)? {
                let stats = db_manager.get_review_stats(user_id, *deck_id)?;
                println!("{}", stats);
            } else {
                println!("User '{}' not found!", username);
            }
        }
        Commands::Migrate { json_path, username, deck_name } => {
            let mut migrator = Migrator::new()?;
            migrator.migrate_from_json(json_path, username, deck_name)?;
            println!("Migration completed successfully!");
        }
        Commands::ListUsers => {
            let users = db_manager.list_users_and_decks()?;
            let mut table = Table::new();
            table.add_row(row!["User ID", "Username", "Deck ID", "Deck Name", "Total Cards", "Cards Due for Review"]);
            for (user_id, username, decks) in users {
                for (deck_id, _deck_name) in decks {
                    let (deck_name, total, due) = db_manager.get_deck_details(user_id, deck_id)?;
                    table.add_row(Row::new(vec![
                        Cell::new(&user_id.to_string()),
                        Cell::new(&username),
                        Cell::new(&deck_id.to_string()),
                        Cell::new(&deck_name),
                        Cell::new(&total.to_string()),
                        Cell::new(&due.to_string()),
                    ]));
                }
            }
            table.printstd();
        }
        Commands::ImportCsv { username, deck_name, csv_path } => {
            if let Some(user_id) = db_manager.authenticate_user(username)? {
                // Check if the deck already exists
                match db_manager.get_deck_id(user_id, deck_name) {
                    Ok(deck_id) => {
                        db_manager.import_flashcards_from_csv(user_id, deck_id, csv_path)?;
                        println!("Flashcards imported successfully from '{}'", csv_path);
                    }
                    Err(_) => {
                        println!("Deck '{}' does not exist.", deck_name);
                        println!("Do you want to create a new deck? (y/n):");
                        let mut input = String::new();
                        std::io::stdin().read_line(&mut input)?;
                        if input.trim().to_lowercase() == "y" {
                            let deck_id = db_manager.create_deck(user_id, deck_name)?;
                            db_manager.import_flashcards_from_csv(user_id, deck_id, csv_path)?;
                            println!("Flashcards imported successfully from '{}'", csv_path);
                        } else {
                            println!("Please re-enter the correct deck name:");
                            let mut new_deck_name = String::new();
                            std::io::stdin().read_line(&mut new_deck_name)?;
                            let new_deck_name = new_deck_name.trim();
                            let deck_id = db_manager.create_or_get_deck(user_id, new_deck_name)?;
                            db_manager.import_flashcards_from_csv(user_id, deck_id, csv_path)?;
                            println!("Flashcards imported successfully from '{}'", csv_path);
                        }
                    }
                }
            } else {
                println!("User '{}' not found!", username);
            }
        }
    }

    Ok(())
}