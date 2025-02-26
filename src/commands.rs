use crate::flashcard::Flashcard;
use crate::migration::Migrator;
use crate::DatabaseManager;
use anyhow::Result;
use clap::Subcommand;
use prettytable::{row, Cell, Row, Table};

fn get_valid_rating() -> Result<i32> {
    loop {
        println!("\nRate your recall (0-5 stars):");
        let mut input = String::new();
        match std::io::stdin().read_line(&mut input) {
            Ok(_) => {
                match input.trim().parse::<i32>() {
                    Ok(rating) if (0..=5).contains(&rating) => {
                        return Ok(rating);
                    }
                    _ => {
                        println!("❌ Please enter a number between 0 and 5.");
                        continue;
                    }
                }
            }
            Err(e) => {
                println!("❌ Error reading input: {}. Press Ctrl+C to exit or try again.", e);
                continue;
            }
        }
    }
}

fn continue_review() -> Result<bool> {
    println!("\n──────────────────────────────────────────────────────────────────────────────────────────");
    println!("Batch complete! Continue reviewing? (y/n):");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_lowercase() == "y")
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new user
    CreateUser { username: String },
    /// Create a new deck
    CreateDeck { username: String, deck_name: String },
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
    Stats { username: String, deck_id: i64 },
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
        Commands::CreateDeck {
            username,
            deck_name,
        } => {
            if let Some(user_id) = db_manager.authenticate_user(username)? {
                db_manager.create_deck(user_id, deck_name)?;
                println!(
                    "Deck '{}' created successfully for user '{}'",
                    deck_name, username
                );
            } else {
                println!("User '{}' not found!", username);
            }
        }
        Commands::AddFlashcard {
            username,
            deck_id,
            question,
            answer,
            guidance,
        } => {
            if let Some(user_id) = db_manager.authenticate_user(username)? {
                let card = Flashcard::new(question.clone(), answer.clone(), guidance.clone());
                db_manager.add_flashcard(*deck_id, user_id, &card)?;
                println!("Flashcard added successfully to deck '{}'", deck_id);
            } else {
                println!("User '{}' not found!", username);
            }
        }
        Commands::Review {
            username,
            deck_id,
            batch_size,
        } => {
            if let Some(user_id) = db_manager.authenticate_user(username)? {
                let cards = db_manager.get_due_flashcards(*deck_id)?;
                let total_cards = cards.len();
                if total_cards == 0 {
                    println!("No cards due for review!");
                    return Ok(());
                }

                println!("\n📚 Starting review session - {} cards due", total_cards);
                let mut card_iter = cards.into_iter().peekable();
                let mut batch_count = 0;

                while card_iter.peek().is_some() {
                    for (i, (card_id, card)) in card_iter.by_ref().take(*batch_size).enumerate() {
                        let current_card = i + batch_count * (*batch_size) + 1;
                        println!("\n");
                        println!("📝 Card {}/{}  repetition {}", current_card, total_cards, card.repetitions);
                        println!("──────────────────────────────────────────────────────────────────────────────────────────");
                        println!("Question: {}", card.question);
                        // Parse and display guidance parts
                        let guidance_parts: Vec<&str> = card
                            .guidance
                            .split('[')
                            .map(|s| s.trim_end_matches(']').trim())
                            .filter(|s| !s.is_empty())
                            .collect();

                        if !guidance_parts.is_empty() {
                            // First part might contain pronunciation
                            if let Some(first) = guidance_parts.first() {
                                if first.starts_with('/') && first.ends_with('/') {
                                    println!("📢 Pronunciation: {}", first);
                                    // Print example sentences starting from second element
                                    for (i, sentence) in guidance_parts.iter().skip(1).enumerate() {
                                        let clean_sentence = sentence.trim_end_matches(']').trim();
                                        println!("📝 {}: {}", i + 1, clean_sentence);
                                    }
                                } else {
                                    // No pronunciation, all parts are example sentences
                                    for (i, sentence) in guidance_parts.iter().enumerate() {
                                        println!("📝 {}: {}", i + 1, sentence);
                                    }
                                }
                            }
                        }
                        println!("\nPress Enter to see the answer...");
                        let mut input = String::new();
                        std::io::stdin().read_line(&mut input)?;

                        println!("\nAnswer");

                        // Parse and display multiple meanings with word classes
                        let answer_parts: Vec<&str> = card.answer
                            .split('[')
                            .map(|s| s.trim_end_matches(']').trim())
                            .filter(|s| !s.is_empty())
                            .collect();

                        for (i, part) in answer_parts.iter().enumerate() {
                            println!("💡 {}: {}", i + 1, part);
                        }

                        let rating = get_valid_rating()?;
                        db_manager.update_flashcard(user_id, card_id, rating)?;

                        // Show feedback
                        let feedback = match rating {
                            0 => "⭐ Keep practicing! This card will appear again soon.",
                            1 => "⭐ Wrong answer, but recognized it.",
                            2 => "⭐⭐ Wrong answer, but getting there.",
                            3 => "⭐⭐⭐ Good effort! Regular review helps.",
                            4 => "⭐⭐⭐⭐ Well done! Nearly perfect.",
                            5 => "⭐⭐⭐⭐⭐ Perfect recall!",
                            _ => unreachable!(),
                        };
                        println!("\n{}", feedback);

                        let interval = match rating {
                            0..=2 => "⏰ Next review: Soon",
                            3..=4 => "📅 Next review: Later",
                            5 => "📆 Next review: Much later",
                            _ => unreachable!(),
                        };
                        println!("{}", interval);
                    }
                    // Continue to next batch or exit
                    if card_iter.peek().is_some() && !continue_review()? {
                        break;
                    }
                    batch_count += 1;
                }

                println!("\n──────────────────────────────────────────────────────────────────────────────────────────");
                println!("Review session completed!");
                println!(
                    "Cards reviewed: {}/{}",
                    total_cards - batch_count * (*batch_size) - card_iter.count(),
                    total_cards
                );
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
        Commands::Migrate {
            json_path,
            username,
            deck_name,
        } => {
            let mut migrator = Migrator::new()?;
            migrator.migrate_from_json(json_path, username, deck_name)?;
            println!("Migration completed successfully!");
        }
        Commands::ListUsers => {
            let users = db_manager.list_users_and_decks()?;
            let mut table = Table::new();
            table.add_row(row![
                "User ID",
                "Username",
                "Deck ID",
                "Deck Name",
                "Total Cards",
                "Cards Due for Review"
            ]);
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
        Commands::ImportCsv {
            username,
            deck_name,
            csv_path,
        } => {
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
