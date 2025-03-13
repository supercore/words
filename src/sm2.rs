use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flashcard {
    pub question: String,
    pub answer: String,
    pub guidance: String,
    pub interval: u32,
    pub repetitions: u32,
    pub ease_factor: f32,
    pub next_review: u64,
    pub id: Option<i64>,  // Add optional id field
}

impl Flashcard {
    pub fn new(question: String, answer: String, guidance: String) -> Self {
        Flashcard {
            question,
            answer,
            guidance,
            interval: 0,
            repetitions: 0,
            ease_factor: 2.5,
            next_review: 0,
            id: None,  // Initialize as None for new cards
        }
    }

    pub fn update(&mut self, performance: u32) {
        match performance {
            0 => {
                self.interval = 1;
                self.repetitions = 0;
            }
            1 => {
                self.interval = 1;
            }
            _ => {
                if self.repetitions == 0 {
                    self.interval = 1;
                } else if self.repetitions == 1 {
                    self.interval = 6;
                } else {
                    self.interval = (self.interval as f32 * self.ease_factor).round() as u32;
                }
                self.repetitions += 1;
            }
        }
        self.ease_factor = (self.ease_factor + 0.1 - (5 - performance) as f32 * 0.08).max(1.3);
        self.next_review = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|n| n.as_secs() + self.interval as u64 * 86400)
            .unwrap_or_else(|_| {
                eprintln!("Error calculating next review time");
                0
            });
    }
}

pub struct Sm2Algorithm;  // Rename from Sm2Algo to Sm2Algorithm

impl Sm2Algorithm {
    pub fn new() -> Self {
        Self {}
    }
}

impl crate::algorithm::SpacedRepetitionAlgorithm for Sm2Algorithm {
    fn name(&self) -> &'static str {
        "sm2"
    }
    fn description(&self) -> &str {
        "SuperMemo-2 algorithm (classic spaced repetition)"
    }
        
    fn process(&self, card: &mut Flashcard, rating: u32) -> anyhow::Result<()> {
        card.update(rating);
        Ok(())
    }
    
    fn convert(&self, card: &Flashcard) -> (bool, f64, f64, f64) {
        // SM2 doesn't use the extra parameters, so return default values
        let _ = card;
        (true, 5.0, 0.0, 1.0)
    }
    
    fn due_cards_query(&self) -> &'static str {
        "SELECT id, question, answer, guidance, interval, repetitions, ease_factor, next_review,
                difficulty, stability, retrievability
         FROM flashcards
         WHERE deck_id = ?1 AND next_review <= ?2
         ORDER BY repetitions ASC, next_review ASC"
    }
    
}
