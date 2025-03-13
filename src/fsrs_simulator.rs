use crate::fsrs::{FsrsCard, FsrsParameters, MemoryState, flashcard_to_fsrs};
use crate::sm2::Flashcard;
use prettytable::{row, Table};

/// Simulates how FSRS schedules a card based on different rating scenarios
pub fn simulate_fsrs_scheduling(question: &str) -> String {
    // Create a new card for simulation
    let mut output = String::new();
    output.push_str(&format!("📊 FSRS Simulation for card: {}\n\n", question));
    
    output.push_str("This simulation shows how the FSRS algorithm schedules cards\n");
    output.push_str("based on different performance ratings and review history.\n\n");
    
    // Default FSRS parameters
    let params = FsrsParameters::default();
    
    // Scenario 1: New card, different ratings
    output.push_str("Scenario 1: NEW CARD (first review)\n");
    output.push_str("=======================================\n");
    let mut table = Table::new();
    table.add_row(row!["Rating", "Interval", "Next Review", "Difficulty", "Stability"]);
    
    // Simulate all possible ratings
    for rating in 3..=5 {
        let card = create_new_card(question);
        let mut fsrs_card = create_fsrs_card(&card);
        fsrs_card.update(rating, &params);
        
        let interval = fsrs_card.get_interval_days();
        let days_from_now = format_days_from_now(fsrs_card.next_review);
        
        table.add_row(row![
            format_rating(rating),
            format!("{} days", interval),
            days_from_now,
            format!("{:.1}", fsrs_card.state.difficulty),
            format!("{:.1}", fsrs_card.state.stability)
        ]);
    }
    output.push_str(&table.to_string());
    output.push_str("\n\n");
    
    // Scenario 2: Card after first successful review
    output.push_str("Scenario 2: LEARNING CARD (second review)\n");
    output.push_str("================================================\n");
    output.push_str("This assumes the first review was rated '4' (Good)\n\n");
    
    let mut table = Table::new();
    table.add_row(row!["Rating", "Interval", "Next Review", "Difficulty", "Stability"]);
    
    let mut card = create_new_card(question);
    let mut fsrs_card = create_fsrs_card(&card);
    
    // First review
    fsrs_card.update(4, &params);
    let first_stability = fsrs_card.state.stability;
    
    for rating in 1..=5 {
        let mut second_card = fsrs_card.clone();
        second_card.update(rating, &params);
        
        let interval = second_card.get_interval_days();
        let days_from_now = format_days_from_now(second_card.next_review);
        let stability_change = 
            if second_card.state.stability > first_stability {
                format!("+{:.1}", second_card.state.stability - first_stability)
            } else if second_card.state.stability < first_stability {
                format!("-{:.1}", first_stability - second_card.state.stability)
            } else {
                "=".to_string()
            };
        
        table.add_row(row![
            format_rating(rating),
            format!("{} days", interval),
            days_from_now,
            format!("{:.1}", second_card.state.difficulty),
            format!("{:.1} ({})", second_card.state.stability, stability_change)
        ]);
    }
    output.push_str(&table.to_string());
    output.push_str("\n\n");
    
    // Scenario 3: Mature card
    output.push_str("Scenario 3: MATURE CARD (after many reviews)\n");
    output.push_str("=============================================\n");
    output.push_str("This simulates a well-established card (stability: 100 days)\n\n");
    
    let mut table = Table::new();
    table.add_row(row!["Rating", "Interval", "Next Review", "Difficulty", "Stability", "Retention"]);
    
    // Create a mature card (high stability)
    let mut mature_card = create_fsrs_card(&card);
    mature_card.state.difficulty = 4.0; // Moderately easy
    mature_card.state.stability = 100.0; // About 100 days
    mature_card.state.retrievability = 0.9; // 90% retention
    mature_card.review_count = 5; // Has been reviewed several times
    
    for rating in 1..=5 {
        let mut card_copy = mature_card.clone();
        card_copy.update(rating, &params);
        
        let interval = card_copy.get_interval_days();
        let days_from_now = format_days_from_now(card_copy.next_review);
        let stability_change = 
            if card_copy.state.stability > mature_card.state.stability {
                format!("+{:.1}", card_copy.state.stability - mature_card.state.stability)
            } else if card_copy.state.stability < mature_card.state.stability {
                format!("-{:.1}", mature_card.state.stability - card_copy.state.stability)
            } else {
                "=".to_string()
            };
        
        table.add_row(row![
            format_rating(rating),
            format!("{} days", interval),
            days_from_now,
            format!("{:.1}", card_copy.state.difficulty),
            format!("{:.1} ({})", card_copy.state.stability, stability_change),
            format!("{:.0}%", card_copy.state.retrievability * 100.0)
        ]);
    }
    output.push_str(&table.to_string());
    output.push_str("\n\n");
    
    // New scenario: 10-round review simulation
    output.push_str("Scenario 4: LONG-TERM PROGRESSION (10 rounds of reviews)\n");
    output.push_str("==========================================================\n");
    output.push_str("This simulates how a card progresses through 10 review cycles with consistent ratings\n\n");
    
    // Create tables for different rating patterns
    let mut perfect_table = Table::new();
    perfect_table.add_row(row!["Review #", "Rating", "Interval", "Next Review", "Difficulty", "Stability", "Retention"]);
    
    let mut good_table = Table::new();
    good_table.add_row(row!["Review #", "Rating", "Interval", "Next Review", "Difficulty", "Stability", "Retention"]);
    
    let mut mixed_table = Table::new();
    mixed_table.add_row(row!["Review #", "Rating", "Interval", "Next Review", "Difficulty", "Stability", "Retention"]);
    
    // Simulation 1: Perfect 5-star reviews
    let mut perfect_card = create_fsrs_card(&create_new_card(question));
    for i in 1..=10 {
        // Apply the rating
        perfect_card.update(5, &params);
        
        let interval = perfect_card.get_interval_days();
        let days_from_now = format_days_from_now(perfect_card.next_review);
        
        perfect_table.add_row(row![
            i,
            "5 (Easy)",
            format!("{} days", interval),
            days_from_now,
            format!("{:.1}", perfect_card.state.difficulty),
            format!("{:.1}", perfect_card.state.stability),
            format!("{:.0}%", perfect_card.state.retrievability * 100.0)
        ]);
        
        // Fast-forward time to next review (simulating reviewing exactly when due)
        perfect_card.last_review = perfect_card.next_review;
    }
    
    // Simulation 2: Consistent 4-star reviews
    let mut good_card = create_fsrs_card(&create_new_card(question));
    for i in 1..=10 {
        // Apply the rating
        good_card.update(4, &params);
        
        let interval = good_card.get_interval_days();
        let days_from_now = format_days_from_now(good_card.next_review);
        
        good_table.add_row(row![
            i,
            "4 (Good)",
            format!("{} days", interval),
            days_from_now,
            format!("{:.1}", good_card.state.difficulty),
            format!("{:.1}", good_card.state.stability),
            format!("{:.0}%", good_card.state.retrievability * 100.0)
        ]);
        
        // Fast-forward time to next review
        good_card.last_review = good_card.next_review;
    }
    
    // Simulation 3: Mixed ratings (alternates between hard and easy)
    let mut mixed_card = create_fsrs_card(&create_new_card(question));
    let mixed_ratings = [4, 3, 5, 4, 3, 5, 4, 5, 4, 3]; // Mix of ratings
    
    for i in 0..10 {
        // Apply the rating
        mixed_card.update(mixed_ratings[i], &params);
        
        let interval = mixed_card.get_interval_days();
        let days_from_now = format_days_from_now(mixed_card.next_review);
        
        mixed_table.add_row(row![
            i + 1,
            format_rating(mixed_ratings[i]),
            format!("{} days", interval),
            days_from_now,
            format!("{:.1}", mixed_card.state.difficulty),
            format!("{:.1}", mixed_card.state.stability),
            format!("{:.0}%", mixed_card.state.retrievability * 100.0)
        ]);
        
        // Fast-forward time to next review
        mixed_card.last_review = mixed_card.next_review;
    }
    
    // Add all simulations to output
    output.push_str("Simulation A: Perfect Reviews (Rating 5)\n");
    output.push_str(&perfect_table.to_string());
    output.push_str("\n\n");
    
    output.push_str("Simulation B: Good Reviews (Rating 4)\n");
    output.push_str(&good_table.to_string());
    output.push_str("\n\n");
    
    output.push_str("Simulation C: Mixed Reviews (Ratings 3-5)\n");
    output.push_str(&mixed_table.to_string());
    output.push_str("\n\n");
    
    // Add comparison summary
    output.push_str("Comparison after 10 review cycles:\n");
    let mut comparison = Table::new();
    comparison.add_row(row!["Review Pattern", "Final Interval", "Total Days", "Difficulty", "Stability"]);
    
    comparison.add_row(row![
        "All Perfect (5)",
        format!("{} days", perfect_card.get_interval_days()),
        calculate_total_days(&perfect_card),
        format!("{:.1}", perfect_card.state.difficulty),
        format!("{:.1}", perfect_card.state.stability)
    ]);
    
    comparison.add_row(row![
        "All Good (4)",
        format!("{} days", good_card.get_interval_days()),
        calculate_total_days(&good_card),
        format!("{:.1}", good_card.state.difficulty),
        format!("{:.1}", good_card.state.stability)
    ]);
    
    comparison.add_row(row![
        "Mixed (3-5)",
        format!("{} days", mixed_card.get_interval_days()),
        calculate_total_days(&mixed_card),
        format!("{:.1}", mixed_card.state.difficulty),
        format!("{:.1}", mixed_card.state.stability)
    ]);
    
    output.push_str(&comparison.to_string());
    
    output.push_str("\n\nUnderstanding FSRS:\n");
    output.push_str("• Rating 3 (Hard): Shorter intervals, higher difficulty\n");
    output.push_str("• Rating 4 (Good): Balanced intervals, maintains difficulty\n");
    output.push_str("• Rating 5 (Easy): Longer intervals, lowers difficulty\n");
    output.push_str("• Failing (1-2): Resets progress but maintains some memory\n\n");
    
    output.push_str("The algorithm is based on a memory model that predicts memory decay over time.\n");
    output
}

// Add this function to demonstrate the retention model
pub fn simulate_retention_targets(original_retention: f64) -> String {
    let mut output = String::new();
    output.push_str("📊 FSRS Retention Target Impact\n\n");
    
    // Create comparison table for different retention targets
    let mut table = Table::new();
    table.add_row(row!["Target", "Interval (day 1)", "Interval (day 30)", "Interval (day 100)"]);
    
    for retention in [0.85, 0.9, 0.95] {
        // Calculate intervals for different memory strengths
        let day1 = (-1.0 * (retention as f64).ln()).ceil();
        let day30 = (-30.0 * (retention as f64).ln()).ceil();
        let day100 = (-100.0 * (retention as f64).ln()).ceil();
        
        table.add_row(row![
            format!("{}%", (retention * 100.0) as i32),
            format!("{} days", day1),
            format!("{} days", day30),
            format!("{} days", day100)
        ]);
    }
    
    output.push_str(&table.to_string());
    output.push_str("\n\nNotes on Retention Targets:\n");
    output.push_str("• Higher target (95%) = Shorter intervals, more reviews, higher accuracy\n");
    output.push_str("• Lower target (85%) = Longer intervals, fewer reviews, more forgetting\n");
    output.push_str("• Default (90%) = Balanced approach for most learning scenarios\n");
    output.push_str("\nConsider 95% for critical information and 85% for supplementary material.\n");
    
    output
}

// Helper functions
fn create_new_card(question: &str) -> Flashcard {
    Flashcard::new(
        question.to_string(),
        "Test Answer".to_string(),
        "Test Guidance".to_string()
    )
}

fn create_fsrs_card(card: &Flashcard) -> FsrsCard {
    let state = MemoryState {
        difficulty: 5.0,
        stability: 0.0,
        retrievability: 1.0,
    };
    
    flashcard_to_fsrs(card, state.difficulty, state.stability, state.retrievability)
}

fn format_rating(rating: u32) -> String {
    match rating {
        0 => "0 (Blackout)".to_string(),
        1 => "1 (Again)".to_string(),
        2 => "2 (Wrong)".to_string(),
        3 => "3 (Hard)".to_string(),
        4 => "4 (Good)".to_string(),
        5 => "5 (Easy)".to_string(),
        _ => format!("{} (Unknown)", rating),
    }
}

fn format_days_from_now(timestamp: u64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_secs();
    
    if timestamp <= now {
        return "Due now".to_string();
    }
    
    let days = (timestamp - now) / 86400;
    if days == 0 {
        "Today".to_string()
    } else if days == 1 {
        "Tomorrow".to_string()
    } else {
        format!("In {} days", days)
    }
}

// Calculate total elapsed days across all reviews
fn calculate_total_days(card: &FsrsCard) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_secs();
    
    // Use card creation time as reference point to calculate total span
    let first_timestamp = now - (60 * 60 * 24 * 365); // Assume creation was 1 year ago max
    
    // Total days from first review to last scheduled review
    let total_days = (card.next_review - first_timestamp) / 86400;
    
    format!("{} days", total_days)
}
