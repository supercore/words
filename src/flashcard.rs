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