use crate::sm2::Flashcard;
use anyhow::Result;
use std::sync::Arc;

pub trait SpacedRepetitionAlgorithm: Sync + Send {
    /// Name of the algorithm
    fn _name(&self) -> &str;

    /// Description of the algorithm
    fn _description(&self) -> &str;

    /// Process a card based on user rating
    fn process(&self, card: &mut Flashcard, rating: u32) -> Result<()>;

    /// Convert a standard flashcard to algorithm-specific format
    fn convert(&self, card: &Flashcard) -> (bool, f64, f64, f64);

    /// SQL query to retrieve due cards
    fn due_cards_query(&self) -> &str;

    /// Get help text for this algorithm
    fn _help_text(&self) -> String {
        format!(
            "Algorithm: {}\n{}\n\nRating scale:\n0-2: Again (short interval)\n3-4: Good\n5: Easy (longer interval)",
            self._name(),
            self._description()
        )
    }
}

pub fn get_algo(name: &str) -> Arc<dyn SpacedRepetitionAlgorithm> {
    match name {
        "fsrs" => Arc::new(crate::fsrs::FsrsAlgorithm::new()),
        _ => Arc::new(crate::sm2::Sm2Algorithm::new()),
    }
}

// pub fn get_available_algorithms() -> Vec<(&'static str, &'static str)> {
//     vec![
//         ("sm2", "SuperMemo-2 algorithm (classic spaced repetition)"),
//         ("fsrs", "Free Spaced Repetition Scheduler (advanced algorithm)"),
//     ]
// }
