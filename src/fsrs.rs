use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::sm2::Flashcard;
// use crate::get_current_timestamp;

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
            maximum_interval: 36500, // 100 years
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
        
        let days_since_last_review = if self.last_review > 0 {
            ((now - self.last_review) as f64) / 86400.0
        } else {
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
                // First successful review
                rating_weight * difficulty_factor / 10.0
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
            days.min(params.maximum_interval as f64).ceil() as u32
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
    FsrsCard {
        question: card.question.clone(),
        answer: card.answer.clone(),
        guidance: card.guidance.clone(),
        state: MemoryState {
            difficulty,
            stability,
            retrievability,
        },
        last_review: card.next_review - (card.interval as u64 * 86400),
        next_review: card.next_review,
        review_count: card.repetitions,
    }
}
