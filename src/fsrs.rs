use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::sm2::Flashcard;
use crate::database::DatabaseManager;
use rusqlite::params;

// FSRS parameters based on the algorithm's default values
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FsrsParameters {
    pub request_retention: f64, // Target retention rate (default: 0.9)
    pub maximum_interval: u32,  // Maximum interval in days (default: 36500)
    pub w: [f64; 4],            // Weights for different memory states
    pub enable_stability_boost: bool, // Whether to apply stability boost
}

impl Default for FsrsParameters {
    fn default() -> Self {
        FsrsParameters {
            request_retention: 0.9,
            maximum_interval: 365 * 4, // 4 years instead of 100 years (more reasonable max)
            w: [0.4, 0.6, 2.4, 5.8], // Default weights based on FSRS research
            enable_stability_boost: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MemoryState {
    pub difficulty: f64,    // Item difficulty (higher = more difficult)
    pub stability: f64,     // Memory stability
    pub retrievability: f64, // Probability of recall at review time
}

impl Default for MemoryState {
    fn default() -> Self {
        MemoryState {
            difficulty: 5.0,     // Middle of the scale (1-10)
            stability: 0.0,      // New card starts at 0
            retrievability: 1.0,  // Initial retrievability
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsrsCard {
    pub question: String,
    pub answer: String,
    pub guidance: String,
    pub state: MemoryState,
    pub last_review: u64,
    pub next_review: u64,
    pub review_count: u32,
}

impl FsrsCard {
    #[allow(dead_code)]
    pub fn new(question: String, answer: String, guidance: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
            .as_secs();

        FsrsCard {
            question,
            answer,
            guidance,
            state: MemoryState::default(),
            last_review: now,
            next_review: now, // Due immediately for first review
            review_count: 0,
        }
    }

    pub fn update(&mut self, rating: u32, params: &FsrsParameters) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
            .as_secs();
        
        let days_since_last_review = if self.last_review > 0 && self.last_review <= now {
            ((now - self.last_review) as f64) / 86400.0
        } else {
            // If last_review is 0 or in the future (shouldn't happen), treat as new card
            0.0
        };

        // Update retrievability based on time passed
        if self.state.stability > 0.0 {
            self.state.retrievability = (-days_since_last_review / self.state.stability).exp();
        }

        // Calculate difficulty based on rating
        match rating {
            0 | 1 => self.state.difficulty = (self.state.difficulty + 1.0).min(10.0), // Failed badly
            2 => self.state.difficulty = (self.state.difficulty + 0.5).min(10.0),    // Failed
            3 => {}                                                                  // Hard but correct
            4 => self.state.difficulty = (self.state.difficulty - 0.5).max(1.0),     // Good
            5 => self.state.difficulty = (self.state.difficulty - 1.0).max(1.0),     // Easy
            _ => {}                                                                  // Invalid rating
        }

        // Calculate new stability based on rating
        let new_stability = if rating < 3 {
            // Failed: reset stability but keep some memory of previous learning
            self.state.stability * 0.5
        } else {
            // Successful: increase stability based on difficulty and previous stability
            let rating_weight = match rating {
                3 => params.w[0], // Hard
                4 => params.w[1], // Good
                5 => params.w[2], // Easy
                _ => 0.0,         // Should never happen
            };

            // Stability boost for repeated cards
            let stability_boost = if params.enable_stability_boost && self.review_count > 0 {
                params.w[3]
            } else {
                1.0
            };

            // The core FSRS stability update formula
            let difficulty_factor = 11.0 - self.state.difficulty;
            let s_factor = if self.state.stability > 0.0 {
                self.state.stability.powf(0.1)
            } else {
                0.1 // Initial stability factor
            };

            // Combine factors to determine new stability
            if self.state.stability <= 0.0 {
                // First successful review - use proper initial values based on rating
                // This ensures new cards with high ratings get reasonable intervals
                match rating {
                    3 => rating_weight * difficulty_factor / 10.0, // Hard ~ 1 day
                    4 => rating_weight * difficulty_factor / 8.0,  // Good ~ 3-4 days
                    5 => rating_weight * difficulty_factor / 6.0,  // Easy ~ 5-7 days
                    _ => rating_weight * difficulty_factor / 10.0, // Default
                }
            } else {
                self.state.stability * (1.0 + rating_weight * s_factor * stability_boost * self.state.retrievability)
            }
        };

        // Update state
        self.state.stability = new_stability;
        
        // Calculate next review interval based on desired retention
        let interval = if rating < 3 {
            // Failed review - schedule for tomorrow or sooner
            if rating == 0 {
                0 // Due immediately
            } else {
                1 // Due tomorrow
            }
        } else {
            // Calculate interval that would give the requested retention
            let days = (-self.state.stability * params.request_retention.ln()).max(1.0);
            
            // For new cards (first review), ensure minimum intervals based on rating
            let min_interval = if self.review_count == 0 {
                match rating {
                    3 => 1,   // Hard: at least 1 day
                    4 => 3,   // Good: at least 3 days
                    5 => 5,   // Easy: at least 5 days
                    _ => 1,
                }
            } else {
                1
            };
            
            (days.max(min_interval as f64)).min(params.maximum_interval as f64).ceil() as u32
        };

        // Update card
        self.last_review = now;
        self.next_review = now + (interval as u64) * 86400;
        self.review_count += 1;
    }

    // Helper method to get scheduled interval in days
    pub fn get_interval_days(&self) -> u32 {
        if self.next_review <= self.last_review {
            return 0;
        }
        ((self.next_review - self.last_review) / 86400) as u32
    }
}

// Convert between the old SM2 model and FSRS model
pub fn sm2_to_fsrs(interval: u32, repetitions: u32, ease_factor: f32) -> MemoryState {
    let difficulty = 10.0 - ((ease_factor as f64) - 1.3) * 10.0; // Convert ease (1.3-2.5) to difficulty (10-1)
    let stability = if repetitions == 0 {
        0.0 // New card
    } else {
        interval as f64 / -0.9_f64.ln() // Reverse-engineer stability from interval and 90% retention
    };
    
    MemoryState {
        difficulty: difficulty.max(1.0).min(10.0),
        stability,
        retrievability: 1.0, // Just reviewed
    }
}

// In general implementation, this conversion is not directly used right now but kept for future use
#[allow(dead_code)]
pub fn fsrs_to_sm2_interval(state: &MemoryState, params: &FsrsParameters) -> u32 {
    if state.stability <= 0.0 {
        return 1;
    }
    
    // Calculate interval that would give the requested retention
    let days = (-state.stability * params.request_retention.ln()).max(1.0);
    days.min(params.maximum_interval as f64).ceil() as u32
}

// Direct conversion function for database operations
pub fn flashcard_to_fsrs(card: &Flashcard, difficulty: f64, stability: f64, retrievability: f64) -> FsrsCard {
    // Calculate last_review safely
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_secs();
    
    let last_review = if card.interval > 0 && card.next_review > (card.interval as u64 * 86400) {
        card.next_review - (card.interval as u64 * 86400)
    } else {
        // If the card has no interval or would result in an overflow, set to now
        now
    };

    FsrsCard {
        question: card.question.clone(),
        answer: card.answer.clone(),
        guidance: card.guidance.clone(),
        state: MemoryState {
            difficulty,
            stability,
            retrievability,
        },
        last_review,
        next_review: card.next_review,
        review_count: card.repetitions,
    }
}

pub struct FsrsAlgorithm {
    params: FsrsParameters,  // Add parameters field
}

impl FsrsAlgorithm {
    pub fn new() -> Self {
        Self {
            params: FsrsParameters::default(),
        }
    }
    
    pub fn _calibrate_parameters(&self, db: &DatabaseManager, user_id: i64) -> Result<FsrsParameters, anyhow::Error> {
        // Get recent review history for this user
        let mut reviews = db.conn.prepare(
            "SELECT r.flashcard_id, r.performance, r.timestamp, r.old_interval,
                    f.interval, f.difficulty, f.stability, f.retrievability
             FROM reviews r
             JOIN flashcards f ON r.flashcard_id = f.id
             JOIN decks d ON f.deck_id = d.id
             WHERE r.user_id = ? AND d.algorithm = 'fsrs'
             ORDER BY r.timestamp DESC
             LIMIT 1000"
        )?;
        
        // Analyze data to find optimal parameters
        let mut total_reviews = 0;
        let mut _correct_predictions = 0;
        
        // These would be adjusted based on actual review outcomes
        let mut w_adjustments = [0.0, 0.0, 0.0, 0.0];
        let mut retention_adjustment = 0.0;
        
        let rows = reviews.query_map(params![user_id], |row| {
            Ok((
                row.get::<_, i64>(0)?, // flashcard_id
                row.get::<_, i32>(1)?, // performance
                row.get::<_, u64>(2)?, // timestamp
                row.get::<_, u32>(3)?, // old_interval
                row.get::<_, u32>(4)?, // interval
                row.get::<_, f64>(5)?, // difficulty
                row.get::<_, f64>(6)?, // stability
                row.get::<_, f64>(7)?, // retrievability
            ))
        })?;
        
        for row_result in rows {
            let (_, performance, _, _old_interval, _new_interval, _difficulty, _stability, retrievability) = row_result?;
            
            // Actual review success (3+ is considered correct recall)
            let actual_success = performance >= 3;
            
            // Predicted success based on retrievability
            let predicted_success = retrievability > self.params.request_retention;
            
            if predicted_success == actual_success {
                _correct_predictions += 1;
            }
            
            // Based on the outcome, adjust parameters
            // This is highly simplified - real calibration would use more sophisticated methods
            if actual_success && !predicted_success {
                // We were too pessimistic - increase weights slightly
                match performance {
                    3 => w_adjustments[0] += 0.01,
                    4 => w_adjustments[1] += 0.01,
                    5 => w_adjustments[2] += 0.01,
                    _ => {}
                }
                // Lower retention target slightly
                retention_adjustment -= 0.001;
            } else if !actual_success && predicted_success {
                // We were too optimistic - decrease weights slightly
                match performance {
                    0 | 1 | 2 => w_adjustments[3] -= 0.01,
                    _ => {}
                }
                // Raise retention target slightly
                retention_adjustment += 0.001;
            }
            
            total_reviews += 1;
        }
        
        if total_reviews < 100 {
            // Not enough data for calibration
            return Ok(self.params);
        }
        
        // Apply adjustments (with limits to prevent extreme changes)
        let new_params = FsrsParameters {
            request_retention: (self.params.request_retention + retention_adjustment).max(0.8).min(0.95),
            w: [
                (self.params.w[0] + w_adjustments[0]).max(0.1).min(1.0),
                (self.params.w[1] + w_adjustments[1]).max(0.2).min(1.2),
                (self.params.w[2] + w_adjustments[2]).max(1.0).min(3.0),
                (self.params.w[3] + w_adjustments[3]).max(3.0).min(8.0),
            ],
            ..self.params
        };
        
        Ok(new_params)
    }
}

impl crate::algorithm::SpacedRepetitionAlgorithm for FsrsAlgorithm {
    fn _name(&self) -> &'static str {
        "fsrs"
    }
    
    fn _description(&self) -> &str {
        "Free Spaced Repetition Scheduler (FSRS) algorithm"
    }
    
    fn process(&self, card: &mut Flashcard, rating: u32) -> anyhow::Result<()> {
        // Use existing values if card has been reviewed before
        let mut difficulty = 5.0;
        let mut stability = 0.0;
        let mut retrievability = 1.0;
        
        // If card already has an ID, try to read existing FSRS values from DB
        if card.id.is_some() && card.repetitions > 0 {
            // Get values from the database if available
            if let Ok(db) = crate::database::DatabaseManager::new("words.db") {
                if let Ok((d, s, r)) = db.get_fsrs_fields(card.id.unwrap()) {
                    difficulty = d;
                    stability = s;
                    retrievability = r;
                }
            }
        }
        
        let mut fsrs_card = flashcard_to_fsrs(card, difficulty, stability, retrievability);
        
        // Update using FSRS
        fsrs_card.update(rating, &self.params);
        
        // Copy updated values back
        card.interval = fsrs_card.get_interval_days();
        card.repetitions = fsrs_card.review_count;
        card.next_review = fsrs_card.next_review;
        
        Ok(())
    }
    
    fn convert(&self, card: &Flashcard) -> (bool, f64, f64, f64) {
        let mem_state = sm2_to_fsrs(card.interval, card.repetitions, card.ease_factor);
        
        (true, mem_state.difficulty, mem_state.stability, mem_state.retrievability)
    }
    
    fn due_cards_query(&self) -> &'static str {
        "SELECT id, question, answer, guidance, interval, repetitions, ease_factor, next_review,
                difficulty, stability, retrievability
         FROM flashcards
         WHERE deck_id = ?1 AND next_review <= ?2
         ORDER BY retrievability ASC, next_review ASC"
    }
}

// Add this function to provide a fast-track graduation mechanism for well-known cards
pub fn graduate_well_known_cards(db: &DatabaseManager, deck_id: i64) -> Result<usize, anyhow::Error> {
    // This SQL query identifies cards that meet the graduation criteria
    // Note: It's not actually checking for "consecutive" high ratings,
    // but rather for cards that have at least 3 total high ratings (4-5)
    // and where ALL ratings are high (minimum rating >= 4)
    let sql = "
        WITH card_high_ratings AS (
            SELECT 
                f.id,
                COUNT(*) as high_rating_count,
                MIN(r.performance) as min_rating
            FROM flashcards f
            JOIN reviews r ON f.id = r.flashcard_id
            WHERE f.deck_id = ?
                AND f.stability <= 7.0   -- Cards still in learning phase
                AND f.repetitions >= 3    -- Cards with at least 3 reviews
                AND r.performance >= 4    -- Only look at high ratings (4-5)
            GROUP BY f.id
            HAVING COUNT(*) >= 3          -- At least 3 high ratings
                   AND MIN(r.performance) >= 4  -- All ratings must be high
        )
        SELECT f.id, f.stability
        FROM flashcards f
        JOIN card_high_ratings chr ON f.id = chr.id";
    
    let mut stmt = db.conn.prepare(sql)?;
    let cards_to_graduate = stmt.query_map([deck_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
    })?;
    
    // Graduate these cards by boosting their stability above the 7-day threshold
    let mut graduated_count = 0;
    
    for result in cards_to_graduate {
        let (card_id, _current_stability) = result?;
        // Set stability to 8.0 days (just above the threshold) to graduate the card
        // This is a gentle nudge to help cards transition to the review phase
        let new_stability = 8.0;
        
        db.conn.execute(
            "UPDATE flashcards SET stability = ? WHERE id = ?",
            [new_stability, card_id as f64],
        )?;
        
        graduated_count += 1;
    }
    
    Ok(graduated_count)
}
