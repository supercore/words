use anyhow::{Result, Context};
use rusqlite::{Connection, params};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::flashcard::Flashcard;
use std::fs::File;
use std::io::{BufReader, BufRead};

pub struct DatabaseManager {
    pub conn: Connection,
}

impl DatabaseManager {
    pub fn new(db_file: &str) -> Result<Self> {
        let conn = Connection::open(db_file)
            .context("Failed to open database")?;
        
        // Initialize database schema
        Self::initialize_schema(&conn)?;

        Ok(DatabaseManager { conn })
    }

    fn initialize_schema(conn: &Connection) -> Result<()> {
        let schema = "
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY,
            username TEXT UNIQUE NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS decks (
            id INTEGER PRIMARY KEY,
            user_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY(user_id) REFERENCES users(id),
            UNIQUE(user_id, name)
        );

        CREATE TABLE IF NOT EXISTS flashcards (
            id INTEGER PRIMARY KEY,
            deck_id INTEGER NOT NULL,
            question TEXT NOT NULL,
            answer TEXT NOT NULL,
            guidance TEXT NOT NULL,
            interval INTEGER NOT NULL,
            repetitions INTEGER NOT NULL,
            ease_factor REAL NOT NULL,
            next_review INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY(deck_id) REFERENCES decks(id)
        );

        CREATE TABLE IF NOT EXISTS reviews (
            id INTEGER PRIMARY KEY,
            flashcard_id INTEGER NOT NULL,
            user_id INTEGER NOT NULL,
            performance INTEGER NOT NULL,
            timestamp INTEGER NOT NULL,
            FOREIGN KEY(flashcard_id) REFERENCES flashcards(id),
            FOREIGN KEY(user_id) REFERENCES users(id)
        );

        CREATE TABLE IF NOT EXISTS operations_log (
            id INTEGER PRIMARY KEY,
            user_id INTEGER NOT NULL,
            operation_type TEXT NOT NULL,
            entity_type TEXT NOT NULL,
            entity_id INTEGER NOT NULL,
            details TEXT,
            timestamp INTEGER NOT NULL,
            FOREIGN KEY(user_id) REFERENCES users(id)
        );
        ";

        conn.execute_batch(schema).context("Failed to initialize database schema")?;
        Ok(())
    }

    pub fn create_user(&self, username: &str) -> Result<i64> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT INTO users (username, created_at) VALUES (?1, ?2)",
            params![username, timestamp],
        ).context("Failed to create user")?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn create_deck(&self, user_id: i64, name: &str) -> Result<i64> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs() as i64;

        // Check if the deck already exists
        let mut stmt = self.conn.prepare(
            "SELECT id FROM decks WHERE user_id = ?1 AND name = ?2"
        )?;
        let existing_deck_id: Result<i64, _> = stmt.query_row(params![user_id, name], |row| row.get(0));

        match existing_deck_id {
            Ok(deck_id) => {
                // Deck already exists, return its ID
                Ok(deck_id)
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // Deck does not exist, create a new one
                self.conn.execute(
                    "INSERT INTO decks (user_id, name, created_at) VALUES (?1, ?2, ?3)",
                    params![user_id, name, timestamp],
                ).context("Failed to create deck")?;

                let deck_id = self.conn.last_insert_rowid();
                self.log_operation(user_id, "CREATE", "DECK", deck_id, Some(name))?;
                Ok(deck_id)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn create_or_get_deck(&self, user_id: i64, deck_name: &str) -> Result<i64> {
        // Check if the deck already exists
        match self.get_deck_id(user_id, deck_name) {
            Ok(deck_id) => Ok(deck_id),
            Err(_) => {
                // Deck does not exist, create a new one
                
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .context("Failed to get system time")?
                    .as_secs() as i64;

                self.conn.execute(
                    "INSERT INTO decks (user_id, name, created_at) VALUES (?1, ?2, ?3)",
                    params![user_id, deck_name, timestamp],
                ).context("Failed to create deck")?;

                let deck_id = self.conn.last_insert_rowid();
                self.log_operation(user_id, "CREATE", "DECK", deck_id, Some(deck_name))?;
                Ok(deck_id)
            }
        }
    }

    pub fn add_flashcard(&self, deck_id: i64, user_id: i64, card: &Flashcard) -> Result<i64> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs() as i64;

        // Generate a unique question if necessary
        let mut unique_question = card.question.clone();
        let mut suffix = 1;

        while self.flashcard_exists(deck_id, &unique_question)? {
            unique_question = format!("{}({})", card.question, suffix);
            suffix += 1;
        }

        self.conn.execute(
            "INSERT INTO flashcards (
                deck_id, question, answer, guidance, interval,
                repetitions, ease_factor, next_review, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                deck_id,
                unique_question,
                card.answer,
                card.guidance,
                card.interval,
                card.repetitions,
                card.ease_factor,
                card.next_review,
                timestamp
            ],
        ).context("Failed to add flashcard")?;

        let card_id = self.conn.last_insert_rowid();
        self.log_operation(user_id, "CREATE", "FLASHCARD", card_id, Some(&unique_question))?;
        Ok(card_id)
    }

    fn flashcard_exists(&self, deck_id: i64, question: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare(
            "SELECT 1 FROM flashcards WHERE deck_id = ?1 AND question = ?2"
        )?;
        let result: Result<i64, _> = stmt.query_row(params![deck_id, question], |row| row.get(0));

        match result {
            Ok(_) => Ok(true),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    pub fn update_flashcard(&self, user_id: i64, card_id: i64, performance: i32) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs() as i64;

        // Get current flashcard
        let mut stmt = self.conn.prepare(
            "SELECT question, answer, guidance, interval, repetitions, ease_factor, next_review 
             FROM flashcards WHERE id = ?"
        )?;
        
        let mut card: Flashcard = stmt.query_row(params![card_id], |row| {
            Ok(Flashcard {
                question: row.get(0)?,
                answer: row.get(1)?,
                guidance: row.get(2)?,
                interval: row.get(3)?,
                repetitions: row.get(4)?,
                ease_factor: row.get(5)?,
                next_review: row.get(6)?,
            })
        })?;

        // Use the SM2 algorithm implementation from Flashcard
        card.update(performance as u32);

        // Update database with new values
        self.conn.execute(
            "UPDATE flashcards 
             SET interval = ?1, repetitions = ?2, ease_factor = ?3, next_review = ?4
             WHERE id = ?5",
            params![card.interval, card.repetitions, card.ease_factor, card.next_review, card_id],
        )?;

        // Record the review
        self.conn.execute(
            "INSERT INTO reviews (flashcard_id, user_id, performance, timestamp)
             VALUES (?1, ?2, ?3, ?4)",
            params![card_id, user_id, performance, timestamp],
        )?;

        self.log_operation(
            user_id,
            "REVIEW",
            "FLASHCARD",
            card_id,
            Some(&format!("Performance: {}", performance)),
        )?;

        Ok(())
    }

    pub fn get_due_flashcards(&self, deck_id: i64) -> Result<Vec<(i64, Flashcard)>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs() as i64;

        let mut stmt = self.conn.prepare(
            "SELECT id, question, answer, guidance, interval, repetitions, ease_factor, next_review
             FROM flashcards
             WHERE deck_id = ?1 AND next_review <= ?2
             ORDER BY repetitions ASC, next_review ASC"
        )?;

        let cards = stmt.query_map(params![deck_id, now], |row| {
            Ok((row.get(0)?, Flashcard {
                question: row.get(1)?,
                answer: row.get(2)?,
                guidance: row.get(3)?,
                interval: row.get(4)?,
                repetitions: row.get(5)?,
                ease_factor: row.get(6)?,
                next_review: row.get(7)?,
            }))
        })?;

        Ok(cards.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn log_operation(&self, user_id: i64, operation: &str, entity: &str, entity_id: i64, details: Option<&str>) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT INTO operations_log (
                user_id, operation_type, entity_type, entity_id, details, timestamp
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![user_id, operation, entity, entity_id, details, timestamp],
        )?;

        Ok(())
    }

    pub fn get_review_stats(&self, user_id: i64, deck_id: i64) -> Result<String> {
        let mut stmt = self.conn.prepare(
            "SELECT 
                COUNT(*) as total_reviews,
                AVG(performance) as avg_performance,
                COUNT(DISTINCT flashcard_id) as unique_cards
             FROM reviews r
             JOIN flashcards f ON r.flashcard_id = f.id
             WHERE r.user_id = ? AND f.deck_id = ?"
        )?;

        let (total, avg, unique): (i64, f64, i64) = stmt.query_row(
            params![user_id, deck_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        )?;

        Ok(format!(
            "Total reviews: {}\nAverage performance: {:.2}\nUnique cards reviewed: {}",
            total, avg, unique
        ))
    }

    pub fn authenticate_user(&self, username: &str) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM users WHERE username = ?"
        )?;
        
        let result: Result<i64, _> = stmt.query_row(params![username], |row| row.get(0));
        
        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_deck_details(&self, user_id: i64, deck_id: i64) -> Result<(String, i64, i64)> {
        let mut stmt = self.conn.prepare(
            "SELECT 
                d.name,
                COUNT(f.id) as total,
                SUM(CASE WHEN f.next_review <= ? THEN 1 ELSE 0 END) as due
             FROM decks d
             LEFT JOIN flashcards f ON d.id = f.deck_id
             WHERE d.id = ? AND d.user_id = ?
             GROUP BY d.name"
        ).context("Failed to prepare statement")?;
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs() as i64;

        stmt.query_row(params![now, deck_id, user_id], |row| {
            let deck_name: String = row.get(0)?;
            let total: i64 = row.get(1)?;
            let due: i64 = row.get(2).unwrap_or(0);
            Ok((deck_name, total, due))
        }).context("Failed to query row")
    }

    pub fn list_users_and_decks(&self) -> Result<Vec<(i64, String, Vec<(i64, String)>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT u.id, u.username, d.id, d.name 
             FROM users u
             LEFT JOIN decks d ON u.id = d.user_id
             ORDER BY u.username, d.name"
        )?;

        let mut users_decks = Vec::new();
        let mut current_user_id = -1; // Initialize to -1 to ensure the first user is processed correctly
        let mut current_username = String::new();
        let mut current_decks = Vec::new();

        let rows = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;

        for row in rows {
            let (user_id, username, deck_id, deck_name): (i64, String, Option<i64>, Option<String>) = row?;
            println!("Row: user_id={}, username={}, deck_id={:?}, deck_name={:?}", user_id, username, deck_id, deck_name);
            if user_id != current_user_id {
                if current_user_id != -1 {
                    users_decks.push((current_user_id, current_username.clone(), current_decks.clone()));
                }
                current_user_id = user_id;
                current_username = username;
                current_decks.clear();
            }
            if let (Some(deck_id), Some(deck_name)) = (deck_id, deck_name) {
                current_decks.push((deck_id, deck_name));
            }
        }

        if current_user_id != -1 {
            users_decks.push((current_user_id, current_username, current_decks));
        }

        println!("Users and Decks: {:?}", users_decks);

        Ok(users_decks)
    }

    pub fn get_deck_id(&self, user_id: i64, deck_name: &str) -> Result<i64> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM decks WHERE user_id = ?1 AND name = ?2"
        )?;
        let deck_id: i64 = stmt.query_row(params![user_id, deck_name], |row| row.get(0))
            .context("Failed to get deck ID")?;
        Ok(deck_id)
    }

    pub fn import_flashcards_from_csv(&self, user_id: i64, deck_id: i64, csv_path: &str) -> Result<()> {
        let file = File::open(csv_path)?;
        let reader = BufReader::new(file);

        let mut total_imported = 0;
        let mut total_skipped = 0;

        println!("Starting import from '{}'", csv_path);

        for (line_number, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split(" ~ ").collect();
            if parts.len() < 3 {
                println!("Skipping invalid line {}: {}", line_number + 1, line);
                total_skipped += 1;
                continue;
            }

            let question = parts[0].trim().to_string();
            let answer = parts[1].trim().to_string();
            let guidance = parts[2].trim().to_string();

            let card = Flashcard::new(question, answer, guidance);

            match self.add_flashcard(deck_id, user_id, &card) {
                Ok(_) => {
                    println!("Imported flashcard: {}", card.question);
                    total_imported += 1;
                }
                Err(e) => {
                    println!("Failed to import flashcard '{}': {}", card.question, e);
                    total_skipped += 1;
                }
            }
        }

        println!("Import completed. Total imported: {}, Total skipped: {}", total_imported, total_skipped);

        Ok(())
    }
}