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
        self.next_review = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|n| n.as_secs() + self.interval as u64 * 86400)
            .unwrap_or_else(|_| {
                eprintln!("Error calculating next review time");
                0
            });
    }
}

struct SpacedRepetitionManager {
    flashcards: HashMap<String, Flashcard>,
    batch_size: usize,
    flashcards_file: String,
}

impl SpacedRepetitionManager {
    fn new(batch_size: usize, flashcards_file: String) -> Self {
        SpacedRepetitionManager {
            flashcards: HashMap::new(),
            batch_size,
            flashcards_file,
        }
    }

    fn add_flashcard(&mut self, question: String, answer: String, guidance: String) {
        let mut unique_question = question.clone();
        let mut counter = 1;
        while self.flashcards.contains_key(&unique_question) {
            unique_question = format!("{} ({})", question, counter);
            counter += 1;
        }
        let flashcard = Flashcard::new(unique_question.clone(), answer, guidance);
        self.flashcards.insert(unique_question, flashcard);
    }

    fn batch_add_flashcards(&mut self, file_path: &str) -> io::Result<()> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            let trimmed_line = line.trim();
            if trimmed_line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = trimmed_line.split('~').collect();
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
        let now = SystemTime::now().duration_since(UNIX_EPOCH)
            .map(|n| n.as_secs())
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "SystemTime error"))?;

        let mut flashcards: Vec<&mut Flashcard> = self.flashcards.values_mut().collect();
        flashcards.sort_by_key(|f| f.next_review);

        let total_to_be_reviewed_count = flashcards.iter().filter(|f| f.next_review <= now).count();
        let mut review_count = 0;

        for flashcard in flashcards {
            if flashcard.next_review <= now {
                review_count += 1;
                println!("Review {}/{}:", review_count, total_to_be_reviewed_count);
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
                    Err(_) => {
                        eprintln!("Invalid performance input");
                        continue;
                    },
                };
                flashcard.update(performance);
                println!();

                if review_count % self.batch_size == 0 {
                    println!("You have reviewed {} flashcards. Do you want to continue? (y/n):", self.batch_size);
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
        fs::write(&self.flashcards_file, data)?;
        Ok(())
    }

    fn load(&mut self) -> io::Result<()> {
        let data = fs::read_to_string(&self.flashcards_file)?;
        let flashcards: Vec<Flashcard> = serde_json::from_str(&data)?;
        for flashcard in flashcards {
            self.flashcards
                .insert(flashcard.question.clone(), flashcard);
        }
        Ok(())
    }
}

fn main() -> io::Result<()> {
    let batch_size = 5;
    let flashcards_file = "flashcards.json".to_string();
    let mut manager = SpacedRepetitionManager::new(batch_size, flashcards_file);

    // Load progress if file exists.
    let _ = manager.load();

    loop {
        println!("Choose an option:");
        println!("1. Review Flashcards");
        println!("2. Add Flashcard");
        println!("3. Import Flashcards from CSV");
        println!("x. Exit");
        let mut choice = String::new();
        io::stdin().read_line(&mut choice)?;

        match choice.trim() {
            "1" => manager.review_flashcards()?,
            "2" => add_flashcard(&mut manager)?,
            "3" => import_flashcards(&mut manager)?,
            "x" => break,
            _ => println!("Invalid option. Please try again."),
        }
    }

    Ok(())
}

fn add_flashcard(manager: &mut SpacedRepetitionManager) -> io::Result<()> {
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
    Ok(())
}

fn import_flashcards(manager: &mut SpacedRepetitionManager) -> io::Result<()> {
    println!("Enter the path to the CSV file:(default: flashcards.csv)");
    let mut file_path = String::new();
    io::stdin().read_line(&mut file_path)?;
    let file_path = if file_path.trim().is_empty() {
        "flashcards.csv".to_string()
    } else {
        file_path
    };
    manager.batch_add_flashcards(file_path.trim())?;
    Ok(())
}
