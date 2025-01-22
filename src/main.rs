use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Flashcard {
    question: String,
    answer: String,
    guidance: String,
    interval: u32,
    repetitions: u32,
    ease_factor: f32,
    next_review: u64,
}

impl Flashcard {
    fn new(question: String, answer: String, guidance: String) -> Self {
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

    fn update(&mut self, performance: u32) {
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
        self.next_review = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(n) => n.as_secs() + self.interval as u64 * 86400,
            Err(_) => 0,
        };
    }
}

struct SpacedRepetitionManager {
    flashcards: HashMap<String, Flashcard>,
}

impl SpacedRepetitionManager {
    fn new() -> Self {
        SpacedRepetitionManager {
            flashcards: HashMap::new(),
        }
    }

    fn add_flashcard(&mut self, question: String, answer: String, guidance: String) {
        let flashcard = Flashcard::new(question.clone(), answer, guidance);
        self.flashcards.insert(question, flashcard);
    }

    fn batch_add_flashcards(&mut self, file_path: &str) -> io::Result<()> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            let parts: Vec<&str> = line.split('~').collect();
            if parts.len() == 3 {
                let question = parts[0].trim().to_string();
                let answer = parts[1].trim().to_string();
                let guidance = parts[2].trim().to_string();
                self.add_flashcard(question, answer, guidance);
            }
        }

        self.save()?;
        Ok(())
    }

    fn review_flashcards(&mut self) -> io::Result<()> {
        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(n) => n.as_secs(),
            Err(_) => return Err(io::Error::new(io::ErrorKind::Other, "SystemTime error")),
        };

        let mut review_count = 0;
        for (_, flashcard) in &mut self.flashcards {
            if flashcard.next_review <= now {
                review_count += 1;
                println!("Review #{}:", review_count);
                println!("Question: {}", flashcard.question);
                println!("Hint: {}", flashcard.guidance);
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                println!("Answer: {}", flashcard.answer);
                println!("How well did you remember? (0-5):");
                let mut performance = String::new();
                io::stdin().read_line(&mut performance)?;
                let performance: u32 = match performance.trim().parse() {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                flashcard.update(performance);
                println!();

                if review_count % 20 == 0 {
                    println!("You have reviewed 20 flashcards. Do you want to continue? (y/n):");
                    let mut choice = String::new();
                    io::stdin().read_line(&mut choice)?;
                    if choice.trim().to_lowercase() != "y" {
                        break;
                    }
                }
            }
        }

        self.save()?;
        Ok(())
    }

    fn save(&self) -> io::Result<()> {
        let flashcards: Vec<Flashcard> = self.flashcards.values().cloned().collect();
        let data = serde_json::to_string(&flashcards)?;
        fs::write("flashcards.json", data)?;
        Ok(())
    }

    fn load(&mut self) -> io::Result<()> {
        let data = fs::read_to_string("flashcards.json")?;
        let flashcards: Vec<Flashcard> = serde_json::from_str(&data)?;
        for flashcard in flashcards {
            self.flashcards
                .insert(flashcard.question.clone(), flashcard);
        }
        Ok(())
    }
}

fn main() -> io::Result<()> {
    let mut manager = SpacedRepetitionManager::new();

    // Load progress if file exists.
    let _ = manager.load();

    loop {
        println!("Choose an option:");
        println!("1. Review Flashcards");
        println!("2. Add Flashcard");
        println!("3. Import Flashcards from CSV");
        // println!("4. Load Preset Flashcards");
        println!("x. Exit");
        let mut choice = String::new();
        io::stdin().read_line(&mut choice)?;

        match choice.trim() {
            "1" => manager.review_flashcards()?,
            "2" => {
                println!("Enter the question:");
                let mut question = String::new();
                io::stdin().read_line(&mut question)?;
                println!("Enter the answer:");
                let mut answer = String::new();
                io::stdin().read_line(&mut answer)?;
                println!("Enter a hint or guidance:");
                let mut guidance = String::new();
                io::stdin().read_line(&mut guidance)?;
                manager.add_flashcard(
                    question.trim().to_string(),
                    answer.trim().to_string(),
                    guidance.trim().to_string(),
                );
                manager.save()?;
            }
            "3" => {
                println!("Enter the path to the CSV file:(default: flashcards.csv)");
                let mut file_path = String::new();
                io::stdin().read_line(&mut file_path)?;
                let file_path = if file_path.trim().is_empty() {
                    "flashcards.csv".to_string()
                } else {
                    file_path
                };
                manager.batch_add_flashcards(file_path.trim())?;
            }
            // "4" => manager.load_preset_flashcards()?,
            "x" => break,
            _ => println!("Invalid option. Please try again."),
        }
    }

    Ok(())
}
