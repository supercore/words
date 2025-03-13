use crate::sm2::Flashcard;
// Remove migration import
use crate::database::DatabaseManager;
// use crate::fsrs_simulator::simulate_fsrs_scheduling;
use anyhow::Result;
use clap::Subcommand;
use prettytable::{row, Cell, Row, Table};
// use rusqlite::params;

fn get_valid_rating() -> Result<i32> {
    loop {
        println!("\nRate your recall (0-5 stars):");
        println!("0: Complete blackout | 1: Wrong answer but familiar");
        println!("2: Wrong but related | 3: Correct with difficulty"); 
        println!("4: Correct with hesitation | 5: Perfect recall");
        
        let mut input = String::new();
        match std::io::stdin().read_line(&mut input) {
            Ok(_) => match input.trim().parse::<i32>() {
                Ok(rating) if (0..=5).contains(&rating) => {
                    return Ok(rating);
                }
                _ => {
                    println!("❌ Please enter a number between 0 and 5.");
                    continue;
                }
            },
            Err(e) => {
                println!(
                    "❌ Error reading input: {}. Press Ctrl+C to exit or try again.",
                    e
                );
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

fn get_user_input(prompt: &str) -> Result<String> {
    println!("{}", prompt);
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn get_confirmed_input(prompt: &str) -> Result<bool> {
    let input = get_user_input(prompt)?;
    Ok(input.to_lowercase() == "y")
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new user
    NewUser { username: String },
    
    /// Create a new deck
    /// 
    /// # Usage:
    /// ```
    /// words new-deck username deck_name --algorithm fsrs
    /// ```
    /// 
    /// The algorithm parameter can be either "sm2" (default) or "fsrs"
    NewDeck { username: String, name: String, algorithm: Option<String> },
    
    /// Add a new flashcard
    AddCard {
        username: String,
        deck_id: i64,
        question: String,
        answer: String,
        guidance: String,
    },
    
    /// Review due flashcards
    /// 
    /// # Usage:
    /// ```
    /// words review username deck_id [batch_size]
    /// ```
    /// 
    /// Reviews cards in batches, with a default batch size of 10 if not specified
    Review {
        username: String,
        deck_id: i64,
        #[clap(default_value = "10")]
        batch: usize,
    },
    
    /// Get review statistics
    Stats { 
        username: String, 
        deck_id: i64,
        /// Output format (basic or detailed)
        #[arg(short, long, default_value = "detailed")]
        format: String, 
    },
    
    /// List all users and their decks
    List,
    
    /// Import flashcards from a CSV file
    Import {
        username: String,
        deck: String,
        path: String,
        #[clap(long, short)]
        algorithm: Option<String>,
    },
    
    /// Look up a flashcard by question
    Find { query: String },
    
    /// Analyze the review order
    Analyze { 
        username: String, 
        deck_id: i64,
        /// Analysis type (order, intervals, queue, forecast)
        #[arg(short, long, default_value = "all")]
        type_: String,
        /// Forecast days ahead for upcoming reviews
        #[arg(short, long, default_value = "30")]
        days: u32,
    },
    
    /// Create a new FSRS deck
    /// 
    /// # Usage:
    /// ```
    /// words new-fsrs username deck_name
    /// ```
    /// 
    /// Shorthand for creating a deck with FSRS algorithm
    NewFsrs { username: String, name: String },
    
    /// Convert deck to FSRS
    /// 
    /// # Usage:
    /// ```
    /// words to-fsrs username deck_id
    /// ```
    /// 
    /// Changes the algorithm of an existing deck from SM2 to FSRS
    /// The change affects scheduling of future reviews
    ToFsrs { username: String, deck_id: i64 },
    
    /// Get FSRS recommendations
    /// 
    /// # Usage:
    /// ```
    /// words fsrs-tips username
    /// ```
    /// 
    /// Analyzes all decks and suggests which ones would benefit most from FSRS
    FsrsTips { 
        username: String,
        /// Show balanced review load analysis
        #[arg(short, long)]
        balance: bool,
        /// Days to forecast for load balancing (default: 30)
        #[arg(short, long, default_value = "30")]
        days: u32,
    },
    
    /// Analyze performance trend
    Trends { 
        username: String, 
        deck_id: i64,
        /// Analysis type (performance, efficiency, retention, all)
        #[arg(short, long, default_value = "all")]
        type_: String,
    },
    
    /// View recent review history
    /// 
    /// # Usage:
    /// ```
    /// words history username deck_id [--limit N]
    /// ```
    /// 
    /// For FSRS decks, shows additional metrics: Difficulty, Stability, Retrievability
    History { username: String, deck_id: i64, limit: Option<usize> },
    
    /// Simulate FSRS algorithm's behavior with different ratings
    /// 
    /// # Usage:
    /// ```
    /// words simulate [--question "Custom question"]
    /// ```
    /// 
    /// Shows how FSRS schedules reviews based on different performance ratings
    Simulate { question: Option<String> },
    
    /// Accelerate graduation of well-known cards
    /// 
    /// # Usage:
    /// ```
    /// words graduate username deck_id
    /// ```
    /// 
    /// Helps cards with consistently high ratings (4-5) graduate from learning to review stage
    Graduate { username: String, deck_id: i64 },
}

// Helper function to format a timestamp as a readable date
fn format_date_time(timestamp: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp as i64, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "Unknown date".to_string())
}

pub fn handle_command(cmd: &Commands, db: &DatabaseManager) -> Result<()> {
    match cmd {
        Commands::NewUser { username } => {
            db.create_user(username)?;
            println!("User '{}' created successfully!", username);
        }
        
        Commands::NewDeck { username, name, algorithm } => {
            if let Some(user_id) = db.authenticate_user(username)? {
                let algorithm_name = algorithm.as_deref().unwrap_or("sm2");
                
                // Validate algorithm name
                if algorithm_name != "sm2" && algorithm_name != "fsrs" {
                    return Err(anyhow::anyhow!("Invalid algorithm: {}. Valid options are 'sm2' or 'fsrs'", algorithm_name));
                }
                
                // Create deck with specified algorithm (SM2 or FSRS)
                let _deck_id = db.create_deck_with_algorithm(user_id, name, algorithm_name)?;
                println!(
                    "Deck '{}' created successfully for user '{}' with {} algorithm",
                    name, username, algorithm_name
                );
            } else {
                println!("User '{}' not found!", username);
            }
        }
        
        Commands::AddCard { username, deck_id, question, answer, guidance } => {
            if let Some(user_id) = db.authenticate_user(username)? {
                let card = Flashcard::new(question.clone(), answer.clone(), guidance.clone());
                db.add_flashcard(*deck_id, user_id, &card)?;
                println!("Flashcard added successfully to deck '{}'", deck_id);
            } else {
                println!("User '{}' not found!", username);
            }
        }
        
        Commands::Review { username, deck_id, batch } => {
            if let Some(user_id) = db.authenticate_user(username)? {
                // First determine which algorithm the deck uses
                let algorithm = db.get_deck_algorithm(*deck_id)?;
                
                // Get due cards based on algorithm
                let (cards, total_cards, queue_info) = if algorithm == "fsrs" {
                    // Use the advanced FSRS queue system
                    let (new_cards, learning_cards, review_cards) = db.get_due_cards_by_queue(*deck_id)?;
                    
                    // Calculate total cards
                    let total = new_cards.len() + learning_cards.len() + review_cards.len();
                    
                    if total == 0 {
                        println!("No cards due for review!");
                        return Ok(());
                    }
                    
                    // Build queue info string for display
                    let queue_info = Some(format!(
                        "Learning: {} | Review: {} | New: {}", 
                        learning_cards.len(), review_cards.len(), new_cards.len()
                    ));
                    
                    // Optimized allocation for FSRS transition period:
                    // - 10% new cards (maintain vocabulary growth)
                    // - 10% review cards (maintain long-term retention)
                    // - 80% learning cards (focus on clearing learning backlog)
                    let mut ordered_cards = Vec::new();
                    
                    // Calculate how many cards of each type to include
                    let new_card_count = new_cards.len();
                    let review_card_count = review_cards.len();
                    let learning_card_count = learning_cards.len();
                    
                    // Allocate 10% of batch size to new cards
                    let desired_new_cards = (*batch / 10).max(1);
                    let new_cards_to_include = new_card_count.min(desired_new_cards);
                    
                    // Allocate 10% of batch size to review cards
                    let desired_review_cards = (*batch / 10).max(1);
                    let review_cards_to_include = review_card_count.min(desired_review_cards);
                    
                    // Calculate remaining slots for learning cards (about 80%)
                    let remaining_slots = *batch - new_cards_to_include - review_cards_to_include;
                    let learning_cards_to_include = learning_card_count.min(remaining_slots);
                    
                    // Add selected new cards
                    ordered_cards.extend(new_cards.into_iter().take(new_cards_to_include));
                    
                    // Add selected review cards
                    ordered_cards.extend(review_cards.into_iter().take(review_cards_to_include));
                    
                    // Add selected learning cards (filling remaining slots)
                    ordered_cards.extend(learning_cards.into_iter().take(learning_cards_to_include));
                    
                    (ordered_cards, total, queue_info)
                } else {
                    // Use standard card fetching for SM2
                    let cards = db.get_due_flashcards(*deck_id)?;
                    let total = cards.len();
                    
                    if total == 0 {
                        println!("No cards due for review!");
                        return Ok(());
                    }
                    
                    (cards, total, None)
                };
                
                // Now use the batch size (which has a default of 10 if not specified)
                println!("\n📚 Starting review session - {} cards due (batch size: {})", total_cards, batch);
                
                // Show queue info for FSRS decks
                if let Some(info) = queue_info {
                    println!("Card distribution: {}", info);
                    println!("⭐ Using FSRS transition mode: 10% new, 10% review, 80% learning!");
                    println!("   This allocation helps clear the learning backlog during the FSRS calibration period.");
                }
                println!();
                
                // Keep track of reviewed cards for the summary
                let mut review_summary = Vec::new();
                
                let mut card_iter = cards.into_iter().peekable();
                let mut batch_count = 0;

                while card_iter.peek().is_some() {
                    for (i, (card_id, card)) in card_iter.by_ref().take(*batch).enumerate() {
                        get_user_input("\nPress Enter to the next card...")?;
                        let current_card = i + batch_count * (*batch) + 1;
                        println!(
                            "📝 Card {}/{}  repetition {}",
                            current_card, total_cards, card.repetitions
                        );
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
                                        // Handle both trailing and embedded bracket characters
                                        let clean_sentence =
                                            sentence.replace(']', "").trim().to_owned();
                                        println!("📝 {}: {}", i + 1, clean_sentence);
                                    }
                                } else {
                                    // No pronunciation, all parts are example sentences
                                    for (i, sentence) in guidance_parts.iter().enumerate() {
                                        // Handle both trailing and embedded bracket characters
                                        let clean_sentence =
                                            sentence.replace(']', "").trim().to_string();
                                        println!("📝 {}: {}", i + 1, clean_sentence);
                                    }
                                }
                            }
                        }

                        // Wait for user to view answer
                        get_user_input("\nPress Enter to see the answer...")?;

                        println!("\nAnswer");

                        // Parse and display multiple meanings with word classes
                        let answer_parts: Vec<&str> = card
                            .answer
                            .split('[')
                            .map(|s| s.trim_end_matches(']').trim())
                            .filter(|s| !s.is_empty())
                            .collect();

                        for (i, part) in answer_parts.iter().enumerate() {
                            // Handle both trailing and embedded bracket characters
                            let clean_part = part.replace(']', "").trim().to_owned();
                            println!("💡 {}: {}", i + 1, clean_part);
                        }

                        let rating = get_valid_rating()?;
                        
                        // Store original next_review and card details for summary
                        let original_next_review = card.next_review;
                        let original_interval = card.interval;
                        
                        // Update the card in the database - THIS HAPPENS IMMEDIATELY AFTER RATING
                        db.update_flashcard(user_id, card_id, rating)?;
                        
                        // Get the updated card data for the summary
                        let updated_card = db.get_flashcard(card_id)?;
                        
                        // Store review details for summary
                        review_summary.push((
                            card.question.clone(),
                            rating,
                            original_interval,
                            updated_card.interval,
                            original_next_review,
                            updated_card.next_review,
                            card_id
                        ));
                        
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

                        // Enhanced review interval feedback with specific date
                        let next_review_date = format_date_time(updated_card.next_review);
                        
                        // Show interval change
                        let interval_change = if updated_card.interval > original_interval {
                            format!(" (+{} days)", updated_card.interval - original_interval)
                        } else if updated_card.interval < original_interval {
                            format!(" (-{} days)", original_interval - updated_card.interval)
                        } else {
                            String::new()
                        };
                        
                        println!("📆 Next review: {} | Interval: {} days{}", 
                            next_review_date,
                            updated_card.interval,
                            interval_change);
                    }

                    // Continue to next batch or exit
                    if card_iter.peek().is_some() && !continue_review()? {
                        break;
                    }
                    batch_count += 1;
                }

                println!("\n──────────────────────────────────────────────────────────────────────────────────────────");
                println!("Review session completed!");
                let cards_reviewed = review_summary.len();
                println!("Cards reviewed: {}/{}", cards_reviewed, total_cards);
                
                // Display review summary if any cards were reviewed
                if !review_summary.is_empty() {
                    println!("\n📊 Review Summary:");
                    let mut summary_table = Table::new();
                    summary_table.add_row(row![
                        "Question",
                        "Rating",
                        "Old Interval",
                        "New Interval",
                        "Next Review"
                    ]);
                    
                    // Add algorithm type to summary table
                    let algo_name = db.get_deck_algorithm(*deck_id)?;
                    
                    for (question, rating, old_interval, new_interval, _, next_review, card_id) in &review_summary {
                        let display_question = if question.len() > 30 {
                            format!("{}...", &question[0..27])
                        } else {
                            question.clone()
                        };
                        
                        let stars = "⭐".repeat((*rating as usize).clamp(1, 5));
                        let next_review_date = format_date_time(*next_review);
                        
                        let interval_change = if new_interval > old_interval {
                            format!("+{}", new_interval - old_interval)
                        } else if new_interval < old_interval {
                            format!("-{}", old_interval - new_interval)
                        } else {
                            "=".to_string()
                        };
                        
                        summary_table.add_row(row![
                            display_question,
                            format!("{} {}", rating, stars),
                            old_interval,
                            format!("{} ({})", new_interval, interval_change),
                            next_review_date
                        ]);
                        
                        // Add FSRS-specific metrics for FSRS decks
                        // - Difficulty: How hard the card is for the user (1-10)
                        // - Stability: How well the memory is consolidated (in days)
                        // - Retention: Probability of successful recall
                        if algo_name == "fsrs" {
                            if let Ok((difficulty, stability, retrievability)) = db.get_fsrs_fields(*card_id) {
                                // Make retrievability a percentage
                                let ret_percent = (retrievability * 100.0).round() as u8;
                                
                                // Add FSRS details as a subrow
                                summary_table.add_row(row![
                                    "",
                                    format!("FSRS Details:"),
                                    format!("Difficulty: {:.1}", difficulty),
                                    format!("Stability: {:.1}d", stability),
                                    format!("Retention: {}%", ret_percent)
                                ]);
                            }
                        }
                    }
                    
                    summary_table.printstd();
                    
                    // Save session statistics
                    let session_id = db.log_review_session(user_id, *deck_id, cards_reviewed)?;
                    println!("\nSession ID: {} - Use 'history' command to view past reviews", session_id);
                }
                
            } else {
                println!("User '{}' not found!", username);
            }
        },
        
        Commands::Stats { username, deck_id, format } => {
            if let Some(user_id) = db.authenticate_user(username)? {
                // Get deck details for the header
                let (deck_name, total_cards, due_cards) = db.get_deck_details(user_id, *deck_id)?;
                println!("\n📊 Statistics for '{}' (Total: {}, Due: {})", deck_name, total_cards, due_cards);
                
                // Get review stats
                let stats = db.get_review_stats(user_id, *deck_id)?;
                println!("{}", stats);
                
                // If detailed format is requested, add more analysis
                if format == "detailed" {
                    // Add interval statistics
                    let intervals = db.get_interval_statistics(*deck_id)?;
                    
                    println!("\n📈 Memory Interval Distribution:");
                    let mut interval_table = prettytable::Table::new();
                    interval_table.add_row(row!["Interval Range", "Card Count", "Percentage"]);
                    
                    for (label, count, percentage) in intervals {
                        interval_table.add_row(row![label, count, format!("{:.1}%", percentage)]);
                    }
                    interval_table.printstd();
                    
                    // Add queue distribution for more insights
                    println!("\n🎯 Learning Progress Analysis:");
                    let queue_dist = db.get_queue_distribution(*deck_id)?;
                    println!("{}", queue_dist);
                }
            } else {
                println!("User '{}' not found!", username);
            }
        }
        
        Commands::List => {
            let users = db.list_users_and_decks()?;
            let mut table = Table::new();
            table.add_row(row![
                "User ID",
                "Username",
                "Deck ID",
                "Deck Name",
                "Total Cards",
                "Cards Due for Review"
            ]);
            for (user_id, username, decks) in users.into_iter() {
                for (deck_id, _deck_name) in decks {
                    let (deck_name, total, due) = db.get_deck_details(user_id, deck_id)?;
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
        
        Commands::Import { username, deck, path, algorithm } => {
            if let Some(user_id) = db.authenticate_user(username)? {
                // Import with algorithm if specified
                let (imported, skipped, errors) = db.import_flashcards_from_csv_with_algorithm(
                    user_id, deck, path, algorithm.as_deref()
                )?;
                
                println!("Import completed:");
                println!("- Successfully imported: {} flashcards", imported);
                println!("- Skipped: {} lines", skipped);
                
                if !errors.is_empty() {
                    println!("\nErrors encountered:");
                    for error in errors.iter().take(5) {
                        println!("- {}", error);
                    }
                    if errors.len() > 5 {
                        println!("... and {} more errors", errors.len() - 5);
                    }
                }
            } else {
                println!("User '{}' not found!", username);
            }
        }
        
        Commands::Find { query } => {
            let matches = db.search_flashcard_globally(query)?;

            if matches.is_empty() {
                println!("No flashcards found matching '{}'", query);
                return Ok(());
            }

            println!(
                "\n📚 Found {} flashcards matching '{}':",
                matches.len(),
                query
            );

            for (i, (user_id, username, deck_id, deck_name, card, algorithm)) in matches.iter().enumerate() {
                println!("\n─────────────────────────────────────────────");
                println!("📝 Match {}/{}", i + 1, matches.len());
                println!("─────────────────────────────────────────────");
                println!("👤 User: {} (ID: {})", username, user_id);
                println!("📁 Deck: {} (ID: {}) [Algorithm: {}]", deck_name, deck_id, algorithm);
                println!("Question: {}", card.question);

                // Display answer information
                println!("Definitions:");
                let answer_parts: Vec<&str> = card
                    .answer
                    .split('[')
                    .map(|s| s.trim_end_matches(']').trim())
                    .filter(|s| !s.is_empty())
                    .collect();

                for (i, part) in answer_parts.iter().enumerate() {
                    let clean_part = part.replace(']', "").trim().to_owned();
                    println!("💡 {}: {}", i + 1, clean_part);
                }

                // Display guidance information
                println!("\nGuidance:");
                let guidance_parts: Vec<&str> = card
                    .guidance
                    .split('[')
                    .map(|s| s.trim_end_matches(']').trim())
                    .filter(|s| !s.is_empty())
                    .collect();

                if !guidance_parts.is_empty() {
                    // Check for pronunciation
                    if let Some(first) = guidance_parts.first() {
                        if first.starts_with('/') && first.ends_with('/') {
                            println!("📢 Pronunciation: {}", first);
                            // Print example sentences starting from second element
                            for (i, sentence) in guidance_parts.iter().skip(1).enumerate() {
                                let clean_sentence = sentence.replace(']', "").trim().to_owned();
                                println!("📝 {}: {}", i + 1, clean_sentence);
                            }
                        } else {
                            // No pronunciation, all parts are example sentences
                            for (i, sentence) in guidance_parts.iter().enumerate() {
                                let clean_sentence = sentence.replace(']', "").trim().to_string();
                                println!("📝 {}: {}", i + 1, clean_sentence);
                            }
                        }
                    }
                }

                // Display card statistics
                println!("\nCard Statistics:");
                println!("Repetitions: {}", card.repetitions);
                println!("Ease Factor: {:.2}", card.ease_factor);

                // Format next review date
                let next_review =
                    chrono::DateTime::<chrono::Utc>::from_timestamp(card.next_review as i64, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "Invalid date".to_string());
                println!("Next Review: {}", next_review);
                
                // Display algorithm-specific info
                if algorithm == "fsrs" {
                    // Try to fetch FSRS-specific data
                    if let Some(card_id) = card.id {
                        if let Ok((difficulty, stability, retrievability)) = db.get_fsrs_fields(card_id) {
                            println!("\nFSRS Details:");
                            println!("Difficulty: {:.2}", difficulty);
                            println!("Stability: {:.2} days", stability);
                            println!("Retrievability: {:.1}%", retrievability * 100.0);
                        }
                    }
                }
            }
            println!("\nTotal matches: {}", matches.len());
        }
        
        Commands::Analyze { username, deck_id, type_, days } => {
            if let Some(user_id) = db.authenticate_user(username)? {
                // Get deck details to display name
                let (deck_name, _, _) = db.get_deck_details(user_id, *deck_id)?;
                
                println!("\n🔍 Analysis for '{}' (ID: {})", deck_name, deck_id);
                
                match type_.as_str() {
                    "order" => {
                        // Analyze review ordering
                        let order_analysis = db.analyze_review_order(*deck_id)?;
                        println!("{}", order_analysis);
                    },
                    "intervals" => {
                        // Analyze interval distribution
                        let intervals = db.get_interval_statistics(*deck_id)?;
                        
                        println!("\n📊 Memory Interval Distribution:");
                        let mut interval_table = prettytable::Table::new();
                        interval_table.add_row(row!["Interval Range", "Card Count", "Percentage"]);
                        
                        for (label, count, percentage) in intervals {
                            interval_table.add_row(row![label, count, format!("{:.1}%", percentage)]);
                        }
                        interval_table.printstd();
                    },
                    "queue" => {
                        // Show queue distribution
                        let queue_dist = db.get_queue_distribution(*deck_id)?;
                        println!("{}", queue_dist);
                    },
                    "forecast" => {
                        // Show upcoming review forecast
                        let upcoming = db.get_upcoming_reviews(user_id, *deck_id)?;
                        
                        println!("\n📆 Upcoming Reviews (Next {} Days):", days);
                        let mut forecast_table = prettytable::Table::new();
                        forecast_table.add_row(row!["Date", "Cards Due", "Workload"]);
                        
                        for (date, count) in upcoming {
                            // Indicate workload
                            let workload = if count > 50 {
                                "⚠️ Heavy"
                            } else if count > 30 {
                                "Medium"
                            } else {
                                "Light"
                            };
                            
                            forecast_table.add_row(row![date, count, workload]);
                        }
                        forecast_table.printstd();
                    },
                    _ => {
                        // Default to comprehensive analysis
                        println!("\n🔢 Review Order Analysis:");
                        let order_analysis = db.analyze_review_order(*deck_id)?;
                        println!("{}", order_analysis);
                        
                        println!("\n📊 Memory Stage Distribution:");
                        let queue_dist = db.get_queue_distribution(*deck_id)?;
                        println!("{}", queue_dist);
                        
                        println!("\n📆 Upcoming Reviews:");
                        let upcoming = db.get_upcoming_reviews(user_id, *deck_id)?;
                        
                        // Only show first 7 days
                        let mut forecast_table = prettytable::Table::new();
                        forecast_table.add_row(row!["Date", "Cards Due", "Workload"]);
                        
                        let limited_upcoming = upcoming.into_iter().take(7).collect::<Vec<_>>();
                        for (date, count) in limited_upcoming {
                            // Indicate workload
                            let workload = if count > 50 {
                                "⚠️ Heavy"
                            } else if count > 30 {
                                "Medium"
                            } else {
                                "Light"
                            };
                            
                            forecast_table.add_row(row![date, count, workload]);
                        }
                        forecast_table.printstd();
                    }
                }
            } else {
                println!("User '{}' not found!", username);
            }
        },
        
        Commands::NewFsrs { username, name } => {
            if let Some(user_id) = db.authenticate_user(username)? {
                db.create_deck_with_algorithm(user_id, name, "fsrs")?;
                println!(
                    "FSRS Deck '{}' created successfully for user '{}'",
                    name, username
                );
            } else {
                println!("User '{}' not found!", username);
            }
        }
        
        Commands::ToFsrs { username, deck_id } => {
            if let Some(user_id) = db.authenticate_user(username)? {
                // First, verify the deck belongs to the user and get its details
                let (deck_name, total_cards, _) = match db.get_deck_details(user_id, *deck_id) {
                    Ok(details) => details,
                    Err(_) => {
                        println!("Deck with ID {} not found or doesn't belong to '{}'", deck_id, username);
                        return Ok(());
                    }
                };
                
                // Get current algorithm
                let algorithm = db.get_deck_algorithm(*deck_id)?;
                if algorithm == "fsrs" {
                    println!("Deck '{}' is already using FSRS algorithm", deck_name);
                    return Ok(());
                }
                
                // Confirm conversion
                if !get_confirmed_input(&format!("Convert deck '{}' with {} cards to FSRS algorithm? This will modify how cards are scheduled. (y/n)", deck_name, total_cards))? {
                    println!("Conversion cancelled");
                    return Ok(());
                }
                
                // Update deck algorithm - this changes how future reviews are scheduled
                // FSRS provides more precise memory modeling than SM2
                db.update_deck_algorithm(*deck_id, "fsrs")?;
                
                // Log the operation
                db.log_operation(
                    user_id,
                    "CONVERT",
                    "DECK",
                    *deck_id,
                    Some(&format!("Algorithm converted from {} to FSRS", algorithm)),
                )?;
                
                println!("Deck '{}' successfully converted to FSRS algorithm", deck_name);
                println!("Note: Existing cards will use FSRS scheduling on their next review.");
            } else {
                println!("User '{}' not found!", username);
            }
        }
        
        Commands::FsrsTips { username, balance, days } => {
            if let Some(user_id) = db.authenticate_user(username)? {
                // Get FSRS recommendations
                let recommendations = db.analyze_fsrs_recommendations(user_id)?;
                println!("{}", recommendations);
                
                // If balance flag is set, show review load balance
                if *balance {
                    println!("\n🔄 Review Load Balance Analysis");
                    println!("=============================\n");
                    let balance_analysis = db.balance_review_load(user_id, *days)?;
                    println!("{}", balance_analysis);
                }
            } else {
                println!("User '{}' not found!", username);
            }
        },
        
        Commands::Trends { username, deck_id, type_ } => {
            if let Some(user_id) = db.authenticate_user(username)? {
                // Get deck name for the header
                let (deck_name, _, _) = db.get_deck_details(user_id, *deck_id)?;
                println!("\n📈 Trend Analysis for '{}'", deck_name);
                
                match type_.as_str() {
                    "performance" => {
                        // Performance trends only
                        let trend_analysis = db.get_performance_trend(user_id, *deck_id)?;
                        println!("{}", trend_analysis);
                    },
                    "efficiency" => {
                        // Learning efficiency analysis
                        let efficiency_analysis = db.analyze_learning_efficiency(user_id, *deck_id)?;
                        println!("{}", efficiency_analysis);
                    },
                    "retention" => {
                        // Focus on retention metrics from performance trend
                        let trend_analysis = db.get_performance_trend(user_id, *deck_id)?;
                        // We only want the retention part
                        if let Some(pos) = trend_analysis.find("FSRS Retention Analysis") {
                            println!("{}", &trend_analysis[pos..]);
                        } else {
                            println!("Retention analysis only available for FSRS decks.");
                        }
                    },
                    _ => {
                        // Default to all analyses
                        let trend_analysis = db.get_performance_trend(user_id, *deck_id)?;
                        println!("{}", trend_analysis);
                        
                        println!("\n"); // Add separation
                        let efficiency_analysis = db.analyze_learning_efficiency(user_id, *deck_id)?;
                        println!("{}", efficiency_analysis);
                    }
                }
            } else {
                println!("User '{}' not found!", username);
            }
        },
        
        Commands::History { username, deck_id, limit } => {
            if let Some(user_id) = db.authenticate_user(username)? {
                let max_items = limit.unwrap_or(40);
                
                // Get deck details to display name
                let (deck_name, _, _) = db.get_deck_details(user_id, *deck_id)?;
                
                println!("\n📚 Review History for '{}' (ID: {})", deck_name, deck_id);
                
                // Get review history with enhanced details
                let history = db.get_review_history(user_id, *deck_id, max_items)?;
                
                if history.is_empty() {
                    println!("No review history found for this deck.");
                    return Ok(());
                }
                
                // Create a pretty table for display
                let mut history_table = prettytable::Table::new();
                history_table.add_row(row!["Time", "Question", "Rating", "Interval Change", "Algorithm"]);
                
                for (timestamp, question, performance, old_interval, new_interval, next_review, algorithm, _) in history {
                    // Format timestamp for display
                    let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp as i64, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "Invalid date".to_string());
                    
                    // Truncate question if too long
                    let display_q = if question.len() > 25 {
                        format!("{}...", &question[0..22])
                    } else {
                        question
                    };
                    
                    // Format interval change
                    let interval_change = format!("{} → {} days", old_interval, new_interval);
                    
                    // Add stars for rating
                    let stars = match performance {
                        0 | 1 => "⭐",
                        2 => "⭐⭐",
                        3 => "⭐⭐⭐",
                        4 => "⭐⭐⭐⭐",
                        5 => "⭐⭐⭐⭐⭐",
                        _ => "",
                    };
                    
                    // Calculate days until next review
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    
                    let days_until = if next_review > now {
                        format!("(in {} days)", (next_review - now) / 86400)
                    } else {
                        "".to_string()
                    };
                    
                    history_table.add_row(row![
                        datetime,
                        display_q,
                        format!("{} {}", performance, stars),
                        format!("{} {}", interval_change, days_until),
                        algorithm
                    ]);
                }
                
                history_table.printstd();
            } else {
                println!("User '{}' not found!", username);
            }
        },
        
        Commands::Simulate { question } => {
            // This command simulates how the FSRS algorithm would schedule reviews
            // based on different performance ratings (0-5).
            // It helps users understand the algorithm's behavior.
            let card_question = question.clone().unwrap_or_else(|| "Example Card".to_string());
            let simulation = crate::fsrs_simulator::simulate_fsrs_scheduling(&card_question);
            println!("{}", simulation);
        },
        
        Commands::Graduate { username, deck_id } => {
            if let Some(user_id) = db.authenticate_user(username)? {
                // Verify it's an FSRS deck
                let algorithm = db.get_deck_algorithm(*deck_id)?;
                if algorithm != "fsrs" {
                    println!("This command only works with FSRS decks");
                    return Ok(());
                }
                
                // Get the deck name for display
                let (deck_name, _, _) = db.get_deck_details(user_id, *deck_id)?;
                
                // Graduate cards that are ready
                let graduated_count = crate::fsrs::graduate_well_known_cards(db, *deck_id)?;
                
                if graduated_count > 0 {
                    println!("Successfully graduated {} cards from learning to review stage", graduated_count);
                    println!("These cards had consistently high ratings (4-5) but were stuck in the learning phase.");
                    println!("They will now appear in the review queue instead of the learning queue.");
                    
                    // Log the operation
                    db.log_operation(
                        user_id, 
                        "GRADUATE",
                        "DECK", 
                        *deck_id,
                        Some(&format!("Graduated {} cards from learning to review", graduated_count)),
                    )?;
                } else {
                    println!("No cards in deck '{}' currently qualify for graduation.", deck_name);
                    println!("Cards need at least 3 consecutive high ratings (4-5) to qualify.");
                }
            } else {
                println!("User '{}' not found!", username);
            }
        },
    }
    Ok(())
}