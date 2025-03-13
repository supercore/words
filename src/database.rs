use crate::sm2::Flashcard;
use anyhow::{Context, Result};
use prettytable::{row, Table};
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::{SystemTime, UNIX_EPOCH};

// Define the function locally instead of importing it
fn get_current_timestamp() -> Result<u64> {
    let start = SystemTime::now();
    let since_epoch = start
        .duration_since(UNIX_EPOCH)
        .context("Time went backwards")?;
    Ok(since_epoch.as_secs())
}

pub struct DatabaseManager {
    pub conn: Connection,
}

impl DatabaseManager {
    pub fn new(db_file: &str) -> Result<Self> {
        let conn = Connection::open(db_file).context("Failed to open database")?;

        // Initialize database schema
        Self::initialize_schema(&conn)?;
        Self::create_indices(&conn)?;

        Ok(DatabaseManager { conn })
    }

    fn initialize_schema(conn: &Connection) -> Result<()> {
        // First, create the basic schema if the tables don't exist
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

        conn.execute_batch(schema)
            .context("Failed to initialize database schema")?;
        
        // Now, check if the deck table has the algorithm column and add it if missing
        let has_algorithm_column = conn
            .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='decks'")?
            .query_row([], |row| {
                let table_sql: String = row.get(0)?;
                Ok(table_sql.contains("algorithm"))
            })
            .unwrap_or(false);

        if !has_algorithm_column {
            // SQLite doesn't support IF NOT EXISTS for ALTER TABLE
            // So we'll check if the column exists and add it if it doesn't
            println!("Upgrading database: Adding 'algorithm' column to 'decks' table...");
            conn.execute(
                "ALTER TABLE decks ADD COLUMN algorithm TEXT NOT NULL DEFAULT 'sm2'",
                [],
            ).context("Failed to add algorithm column to decks table")?;
        }
        
        // Now add the FSRS fields to the flashcards table if they don't exist
        let has_difficulty_column = conn
            .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='flashcards'")?
            .query_row([], |row| {
                let table_sql: String = row.get(0)?;
                Ok(table_sql.contains("difficulty"))
            })
            .unwrap_or(false);
            
        if !has_difficulty_column {
            println!("Upgrading database: Adding FSRS fields to 'flashcards' table...");
            
            // Add each column individually
            conn.execute(
                "ALTER TABLE flashcards ADD COLUMN difficulty REAL DEFAULT 5.0",
                [],
            ).context("Failed to add difficulty column")?;
            
            conn.execute(
                "ALTER TABLE flashcards ADD COLUMN stability REAL DEFAULT 0.0",
                [],
            ).context("Failed to add stability column")?;
            
            conn.execute(
                "ALTER TABLE flashcards ADD COLUMN retrievability REAL DEFAULT 1.0",
                [],
            ).context("Failed to add retrievability column")?;
        }

        // Now, check if the reviews table has the old_interval column and add it if missing
        let has_old_interval = conn
            .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='reviews'")?
            .query_row([], |row| {
                let table_sql: String = row.get(0)?;
                Ok(table_sql.contains("old_interval"))
            })
            .unwrap_or(false);

        if !has_old_interval {
            println!("Upgrading database: Adding 'old_interval' column to 'reviews' table...");
            conn.execute(
                "ALTER TABLE reviews ADD COLUMN old_interval INTEGER",
                [],
            ).context("Failed to add old_interval column to reviews table")?;
        }

        Ok(())
    }

    fn create_indices(conn: &Connection) -> Result<()> {
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_flashcards_deck ON flashcards (deck_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_reviews_user ON reviews (user_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_reviews_flashcard ON reviews (flashcard_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_reviews_timestamp ON reviews (timestamp)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_flashcards_next_review ON flashcards (next_review)",
            [],
        )?;
        Ok(())
    }

    pub fn create_user(&self, username: &str) -> Result<i64> {
        let timestamp = get_current_timestamp()?;

        self.conn
            .execute(
                "INSERT INTO users (username, created_at) VALUES (?1, ?2)",
                params![username, timestamp],
            )
            .context("Failed to create user")?;

        Ok(self.conn.last_insert_rowid())
    }

    // pub fn create_deck(&self, user_id: i64, name: &str) -> Result<i64> {
    //     // By default, create SM2 decks for backward compatibility
    //     self.create_deck_with_algorithm(user_id, name, "sm2")
    // }

    // pub fn create_or_get_deck(&self, user_id: i64, deck_name: &str) -> Result<i64> {
    //     // Check if the deck already exists
    //     match self.get_deck_id(user_id, deck_name) {
    //         Ok(deck_id) => Ok(deck_id),
    //         Err(_) => {
    //             // Create with default SM2 algorithm
    //             self.create_deck_with_algorithm(user_id, deck_name, "sm2")
    //         }
    //     }
    // }

    pub fn add_flashcard(&self, deck_id: i64, user_id: i64, card: &Flashcard) -> Result<i64> {
        let timestamp = get_current_timestamp()?;

        // Generate a unique question if necessary
        let mut unique_question = card.question.clone();
        let mut suffix = 1;

        while self.flashcard_exists(deck_id, &unique_question)? {
            unique_question = format!("{}({})", card.question, suffix);
            suffix += 1;
        }

        self.conn
            .execute(
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
            )
            .context("Failed to add flashcard")?;

        let card_id = self.conn.last_insert_rowid();
        self.log_operation(
            user_id,
            "CREATE",
            "FLASHCARD",
            card_id,
            Some(&unique_question),
        )?;
        Ok(card_id)
    }

    fn flashcard_exists(&self, deck_id: i64, question: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM flashcards WHERE deck_id = ?1 AND question = ?2")?;
        let result: Result<i64, _> = stmt.query_row(params![deck_id, question], |row| row.get(0));

        match result {
            Ok(_) => Ok(true),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    pub fn update_flashcard(&self, user_id: i64, card_id: i64, rating: i32) -> Result<()> {
        let timestamp = get_current_timestamp()?;

        // Get current flashcard state before update (for history)
        let current_card = self.get_flashcard(card_id)?;
        let old_interval = current_card.interval;

        // Get current flashcard and deck info
        let mut stmt = self.conn.prepare(
            "SELECT f.question, f.answer, f.guidance, f.interval, f.repetitions, f.ease_factor, 
                   f.next_review, f.difficulty, f.stability, f.retrievability, d.algorithm, f.deck_id
             FROM flashcards f
             JOIN decks d ON f.deck_id = d.id
             WHERE f.id = ?"
        )?;

        let row = stmt.query_row(params![card_id], |row| {
            Ok((
                row.get::<_, String>(0)?, // question
                row.get::<_, String>(1)?, // answer
                row.get::<_, String>(2)?, // guidance
                row.get::<_, u32>(3)?,    // interval
                row.get::<_, u32>(4)?,    // repetitions
                row.get::<_, f32>(5)?,    // ease_factor
                row.get::<_, u64>(6)?,    // next_review
                row.get::<_, f64>(7).unwrap_or(5.0), // difficulty
                row.get::<_, f64>(8).unwrap_or(0.0), // stability
                row.get::<_, f64>(9).unwrap_or(1.0), // retrievability
                row.get::<_, String>(10).unwrap_or_else(|_| "sm2".to_string()), // algorithm
                row.get::<_, i64>(11)?,   // deck_id
            ))
        })?;
        
        let (question, answer, guidance, interval, repetitions, ease_factor, next_review, 
            difficulty, stability, retrievability, algo_name, _deck_id) = row;

        // Create a mutable flashcard
        let mut card = Flashcard {
            question,
            answer,
            guidance,
            interval,
            repetitions,
            ease_factor,
            next_review,
            id: Some(card_id),
        };

        // Get algorithm and update card
        let algo = crate::algorithm::get_algo(&algo_name);
        algo.process(&mut card, rating as u32)?;

        // Update DB based on algorithm
        if algo_name == "fsrs" {
            // Create FSRS card from updated card
            let fsrs_card = crate::fsrs::flashcard_to_fsrs(&card, difficulty, stability, retrievability);
            
            // Update with FSRS fields
            self.conn.execute(
                "UPDATE flashcards 
                 SET interval = ?1, repetitions = ?2, next_review = ?3, 
                     difficulty = ?4, stability = ?5, retrievability = ?6
                 WHERE id = ?7",
                params![
                    card.interval,
                    card.repetitions,
                    card.next_review,
                    fsrs_card.state.difficulty,
                    fsrs_card.state.stability,
                    fsrs_card.state.retrievability,
                    card_id
                ],
            )?;
        } else {
            // Update with standard SM2 fields
            self.conn.execute(
                "UPDATE flashcards 
                 SET interval = ?1, repetitions = ?2, ease_factor = ?3, next_review = ?4
                 WHERE id = ?5",
                params![
                    card.interval,
                    card.repetitions,
                    card.ease_factor,
                    card.next_review,
                    card_id
                ],
            )?;
        }

        // Record review with old interval info
        self.conn.execute(
            "INSERT INTO reviews (flashcard_id, user_id, performance, timestamp, old_interval)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![card_id, user_id, rating, timestamp, old_interval],
        )?;

        self.log_operation(
            user_id,
            "REVIEW",
            "FLASHCARD",
            card_id,
            Some(&format!("Performance: {} (Algorithm: {})", rating, algo_name)),
        )?;

        Ok(())
    }

    pub fn get_due_flashcards(&self, deck_id: i64) -> Result<Vec<(i64, Flashcard)>> {
        let now = get_current_timestamp()?;

        // Get algorithm used by deck
        let algo_name: String = self.conn.query_row(
            "SELECT algorithm FROM decks WHERE id = ?",
            params![deck_id],
            |row| row.get::<_, String>(0)
        ).unwrap_or_else(|_| "sm2".to_string());
        
        let algo = crate::algorithm::get_algo(&algo_name);

        // Get due cards with algorithm-specific query
        let query = algo.due_cards_query();
        let mut stmt = self.conn.prepare(query)?;

        let cards = stmt.query_map(params![deck_id, now], |row| {
            // We still use the standard Flashcard type for compatibility with existing UI
            let card_id = row.get(0)?;
            Ok((
                card_id,
                Flashcard {
                    question: row.get(1)?,
                    answer: row.get(2)?,
                    guidance: row.get(3)?,
                    interval: row.get(4)?,
                    repetitions: row.get(5)?,
                    ease_factor: row.get(6)?,
                    next_review: row.get(7)?,
                    id: Some(card_id),  // Set the ID field
                }
            ))
        })?;

        let mut result = Vec::new();
        for card in cards {
            result.push(card?);
        }

        // Log retrieval with algo info
        self.log_operation(
            0,
            "GET",
            "DUE",
            deck_id,
            Some(&format!("Got {} cards using {}", result.len(), algo_name)),
        )?;

        Ok(result)
    }

    pub fn log_operation(
        &self,
        user_id: i64,
        operation: &str,
        entity: &str,
        entity_id: i64,
        details: Option<&str>,
    ) -> Result<()> {
        let timestamp = get_current_timestamp()?;

        self.conn.execute(
            "INSERT INTO operations_log (
                user_id, operation_type, entity_type, entity_id, details, timestamp
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![user_id, operation, entity, entity_id, details, timestamp],
        )?;

        Ok(())
    }

    pub fn get_review_stats(&self, user_id: i64, deck_id: i64) -> Result<String> {
        // Get basic stats
        let mut stmt = self.conn.prepare(
            "SELECT 
                COUNT(*) as total_reviews,
                AVG(performance) as avg_performance,
                COUNT(DISTINCT flashcard_id) as unique_cards
             FROM reviews r
             JOIN flashcards f ON r.flashcard_id = f.id
             WHERE r.user_id = ? AND f.deck_id = ?",
        )?;

        let (total, avg, unique): (i64, f64, i64) = stmt
            .query_row(params![user_id, deck_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;

        // Get deck name
        let deck_name = self.conn.query_row(
            "SELECT name FROM decks WHERE id = ?",
            params![deck_id],
            |row| row.get::<_, String>(0),
        )?;

        let mut output = String::new();
        output.push_str(&format!("Review Statistics for \"{}\"\n\n", deck_name));

        // Create summary table
        let mut summary_table = Table::new();
        summary_table.add_row(row!["Metric", "Value"]);
        summary_table.add_row(row!["Total reviews", total]);
        summary_table.add_row(row!["Average performance", format!("{:.2}", avg)]);
        summary_table.add_row(row!["Unique cards", unique]);
        output.push_str(&summary_table.to_string());
        output.push_str("\n\nDaily Review Activities:\n");

        // Create daily activities table
        let mut daily_table = Table::new();
        daily_table.add_row(row!["Date", "Reviews", "Words Reviewed"]);

        let mut stmt_daily = self.conn.prepare(
            "SELECT 
                strftime('%Y-%m-%d', datetime(timestamp, 'unixepoch')) as review_date,
                COUNT(*) as review_count,
                COUNT(DISTINCT flashcard_id) as words_reviewed
             FROM reviews
             WHERE user_id = ? 
             AND flashcard_id IN (SELECT id FROM flashcards WHERE deck_id = ?)
             GROUP BY review_date
             ORDER BY review_date DESC
             LIMIT 20;",
        )?;

        let daily_iter = stmt_daily.query_map(params![user_id, deck_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;

        for daily in daily_iter {
            let (date, review_count, words_reviewed) = daily?;
            daily_table.add_row(row![date, review_count, words_reviewed]);
        }

        output.push_str(&daily_table.to_string());
        output.push_str("\n\nDifficult Words (Performance < 4):\n");

        // Create difficult words table
        let mut difficult_table = Table::new();
        difficult_table.add_row(row!["Word", "Score", "Reps", "Last Review"]);

        let mut stmt_low_perf = self.conn.prepare(
            "SELECT f.question, r.performance, f.repetitions, MAX(r.timestamp)
             FROM reviews r
             JOIN flashcards f ON r.flashcard_id = f.id
             WHERE r.user_id = ? 
             AND f.deck_id = ? 
             AND r.performance < 4
             GROUP BY f.question
             ORDER BY MAX(r.timestamp) DESC
             LIMIT 20;",
        )?;

        let low_perf_iter = stmt_low_perf.query_map(params![user_id, deck_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;

        for entry in low_perf_iter {
            let (question, performance, repetitions, timestamp) = entry?;
            let review_date = chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "Invalid date".to_string());

            let display_word = if question.len() > 25 {
                format!("{}...", &question[0..22])
            } else {
                question
            };

            difficult_table.add_row(row![display_word, performance, repetitions, review_date]);
        }

        output.push_str(&difficult_table.to_string());

        // Add repetition distribution
        output.push_str("\n\nRepetition Distribution:\n");
        let mut repetition_table = Table::new();
        repetition_table.add_row(row!["Repetitions", "Count", "Percentage"]);

        let mut stmt_rep_dist = self.conn.prepare(
        "SELECT 
            repetitions, 
            COUNT(*) as count,
            ROUND(COUNT(*) * 100.0 / (SELECT COUNT(*) FROM flashcards WHERE deck_id = ?), 2) as percentage
         FROM flashcards
         WHERE deck_id = ?
         GROUP BY repetitions
         ORDER BY repetitions ASC"
    )?;

        let rep_dist_iter = stmt_rep_dist.query_map(params![deck_id, deck_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?;

        for dist in rep_dist_iter {
            let (repetition, count, percentage) = dist?;
            repetition_table.add_row(row![repetition, count, format!("{:.2}%", percentage)]);
        }

        output.push_str(&repetition_table.to_string());

        // Add performance distribution
        output.push_str("\nRating Distribution:\n");
        let mut perf_table = Table::new();
        perf_table.add_row(row!["Rating", "Count", "Percentage"]);

        let mut stmt_perf_dist = self.conn.prepare(
            "SELECT 
            performance, 
            COUNT(*) as count,
            ROUND(COUNT(*) * 100.0 / (SELECT COUNT(*) FROM reviews r 
                JOIN flashcards f ON r.flashcard_id = f.id
                WHERE r.user_id = ? AND f.deck_id = ?), 2) as percentage
         FROM reviews r
         JOIN flashcards f ON r.flashcard_id = f.id
         WHERE r.user_id = ? AND f.deck_id = ?
         GROUP BY performance
         ORDER BY performance ASC",
        )?;

        let perf_dist_iter =
            stmt_perf_dist.query_map(params![user_id, deck_id, user_id, deck_id], |row| {
                Ok((
                    row.get::<_, i32>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })?;

        for dist in perf_dist_iter {
            let (performance, count, percentage) = dist?;

            // Add star emoji for each performance level
            let stars = match performance {
                0 => "⭐",
                1 => "⭐",
                2 => "⭐⭐",
                3 => "⭐⭐⭐",
                4 => "⭐⭐⭐⭐",
                5 => "⭐⭐⭐⭐⭐",
                _ => "",
            };

            perf_table.add_row(row![
                format!("{} {}", performance, stars),
                count,
                format!("{:.2}%", percentage)
            ]);
        }

        output.push_str(&perf_table.to_string());
        Ok(output)
    }

    pub fn authenticate_user(&self, username: &str) -> Result<Option<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM users WHERE username = ?")?;

        let result: Result<i64, _> = stmt.query_row(params![username], |row| row.get(0));

        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_deck_details(&self, user_id: i64, deck_id: i64) -> Result<(String, i64, i64)> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT 
                d.name,
                COUNT(f.id) as total,
                SUM(CASE WHEN f.next_review <= ? THEN 1 ELSE 0 END) as due
             FROM decks d
             LEFT JOIN flashcards f ON d.id = f.deck_id
             WHERE d.id = ? AND d.user_id = ?
             GROUP BY d.name",
            )
            .context("Failed to prepare statement")?;

        let now = get_current_timestamp()?;

        stmt.query_row(params![now, deck_id, user_id], |row| {
            let deck_name: String = row.get(0)?;
            let total: i64 = row.get(1)?;
            let due: i64 = row.get(2).unwrap_or(0);
            Ok((deck_name, total, due))
        })
        .context("Failed to query row")
    }

    pub fn list_users_and_decks(&self) -> Result<Vec<(i64, String, Vec<(i64, String)>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT u.id, u.username, d.id, d.name 
             FROM users u
             LEFT JOIN decks d ON u.id = d.user_id
             ORDER BY u.username, d.name",
        )?;

        let mut users_decks = Vec::new();
        let mut current_user_id = -1; // Initialize to -1 to ensure the first user is processed correctly
        let mut current_username = String::new();
        let mut current_decks = Vec::new();

        let rows = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;

        for row in rows {
            let (user_id, username, deck_id, deck_name): (
                i64,
                String,
                Option<i64>,
                Option<String>,
            ) = row?;
            println!(
                "Row: user_id={}, username={}, deck_id={:?}, deck_name={:?}",
                user_id, username, deck_id, deck_name
            );
            if user_id != current_user_id {
                if current_user_id != -1 {
                    users_decks.push((
                        current_user_id,
                        current_username.clone(),
                        current_decks.clone(),
                    ));
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
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM decks WHERE user_id = ?1 AND name = ?2")?;
        let deck_id: i64 = stmt
            .query_row(params![user_id, deck_name], |row| row.get(0))
            .context("Failed to get deck ID")?;
        Ok(deck_id)
    }

    pub fn import_flashcards_from_csv(
        &self,
        user_id: i64,
        deck_id: i64,
        csv_path: &str,
    ) -> Result<(usize, usize, Vec<String>)> { // Return imported count, skipped count, and errors
        // Get the deck's algorithm type first
        let algorithm = self.get_deck_algorithm(deck_id)?;
        println!("Importing cards into deck with algorithm: {}", algorithm);
        
        let file = File::open(csv_path)?;
        let reader = BufReader::new(file);

        let mut cards_to_import = Vec::new();
        let mut invalid_lines = Vec::new();
        let mut error_messages = Vec::new();

        println!("Starting import from '{}'", csv_path);

        for (line_number, line) in reader.lines().enumerate() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    let error = format!("Error reading line {}: {}", line_number + 1, e);
                    error_messages.push(error);
                    continue;
                }
            };
            
            if line.trim().is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split(" ~ ").collect();
            if parts.len() < 3 {
                let error = format!("Line {}: Invalid format (expected 'question ~ answer ~ guidance')", line_number + 1);
                println!("Skipping: {}", error);
                error_messages.push(error);
                invalid_lines.push(line_number + 1);
                continue;
            }

            let question = parts[0].trim().to_string();
            let answer = parts[1].trim().to_string();
            let guidance = parts[2].trim().to_string();

            let card = Flashcard::new(question, answer, guidance);
            cards_to_import.push(card);
        }

        // Handle duplicate questions by making them unique before batch insertion
        let mut processed_cards = Vec::new();
        let mut existing_questions = self.get_existing_questions(deck_id)?;
        let mut duplicates_renamed = 0;

        for mut card in cards_to_import {
            let mut unique_question = card.question.clone();
            let mut suffix = 1;

            while existing_questions.contains(&unique_question) {
                unique_question = format!("{}({})", card.question, suffix);
                suffix += 1;
                duplicates_renamed += 1;
            }

            if unique_question != card.question {
                card.question = unique_question.clone();
            }

            existing_questions.insert(unique_question);
            processed_cards.push(card);
        }

        // Convert Vec<Flashcard> to Vec<&Flashcard> for batch_add_flashcards
        let card_refs: Vec<&Flashcard> = processed_cards.iter().collect();

        // Use batch operation for actual insertion - now with algorithm awareness
        match self.batch_add_flashcards(deck_id, user_id, card_refs) {
            Ok(_) => {
                println!(
                    "Import completed. Imported: {}, Skipped: {}, Renamed: {}",
                    processed_cards.len(),
                    invalid_lines.len(),
                    duplicates_renamed
                );

                // Log the operation with algorithm info
                self.log_operation(
                    user_id,
                    "IMPORT",
                    "FLASHCARDS",
                    deck_id,
                    Some(&format!(
                        "Imported {} cards from CSV (Algorithm: {})",
                        processed_cards.len(),
                        algorithm
                    )),
                )?;

                Ok((processed_cards.len(), invalid_lines.len(), error_messages))
            }
            Err(e) => {
                println!("Failed to import flashcards: {}", e);
                error_messages.push(format!("Database error: {}", e));
                Err(e)
            }
        }
    }

    // Add this helper method to get all existing questions in a deck
    fn get_existing_questions(&self, deck_id: i64) -> Result<HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT question FROM flashcards WHERE deck_id = ?")?;

        let rows = stmt.query_map(params![deck_id], |row| row.get::<_, String>(0))?;

        let mut questions = HashSet::new();
        for question in rows {
            questions.insert(question?);
        }

        Ok(questions)
    }

    pub fn batch_add_flashcards(
        &self,
        deck_id: i64,
        user_id: i64,
        cards: Vec<&Flashcard>,
    ) -> Result<()> {
        // Verify deck belongs to user and get algorithm
        let mut stmt = self
            .conn
            .prepare("SELECT 1, algorithm FROM decks WHERE id = ? AND user_id = ?")?;
        
        let result: Result<(i64, String), _> = stmt.query_row(params![deck_id, user_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        });

        let algorithm_name = match result {
            Ok((_, alg)) => alg,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return Err(anyhow::anyhow!(
                    "Deck doesn't belong to user or doesn't exist"
                ));
            },
            Err(e) => return Err(e.into()),
        };

        let timestamp = get_current_timestamp()?;
        let algo = crate::algorithm::get_algo(&algorithm_name);

        self.conn.execute_batch("BEGIN TRANSACTION;")?;

        // Insert cards based on algorithm
        if algorithm_name == "fsrs" {
            // Use FSRS fields
            let mut stmt = self.conn.prepare(
                "INSERT INTO flashcards (
                    deck_id, question, answer, guidance, interval, repetitions, ease_factor, 
                    next_review, created_at, difficulty, stability, retrievability)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )?;

            for card in cards {
                // Convert the card using the algorithm's conversion method
                let (_, difficulty, stability, retrievability) = algo.convert(card);
                
                stmt.execute(params![
                    deck_id,
                    &card.question,
                    &card.answer,
                    &card.guidance,
                    card.interval,
                    card.repetitions,
                    card.ease_factor,
                    card.next_review,
                    timestamp,
                    difficulty,
                    stability,
                    retrievability
                ])?;
            }
        } else {
            // Use standard SM2 fields
            let mut stmt = self.conn.prepare(
                "INSERT INTO flashcards (
                    deck_id, question, answer, guidance, interval, repetitions, 
                    ease_factor, next_review, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )?;

            for card in cards {
                stmt.execute(params![
                    deck_id,
                    &card.question,
                    &card.answer,
                    &card.guidance,
                    card.interval,
                    card.repetitions,
                    card.ease_factor,
                    card.next_review,
                    timestamp
                ])?;
            }
        }

        self.conn.execute_batch("COMMIT;")?;
        Ok(())
    }

    pub fn search_flashcard_globally(
        &self,
        question: &str,
    ) -> Result<Vec<(i64, String, i64, String, Flashcard, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT u.id as user_id, u.username, d.id as deck_id, d.name as deck_name, 
             f.id, f.question, f.answer, f.guidance, f.interval, f.repetitions, f.ease_factor, f.next_review,
             d.algorithm
             FROM flashcards f
             JOIN decks d ON f.deck_id = d.id
             JOIN users u ON d.user_id = u.id
             WHERE f.question LIKE ?
             ORDER BY u.username, d.name"
        )?;

        let search_pattern = format!("%{}%", question);

        let card_iter = stmt.query_map(params![search_pattern], |row| {
            let user_id: i64 = row.get(0)?;
            let username: String = row.get(1)?;
            let deck_id: i64 = row.get(2)?;
            let deck_name: String = row.get(3)?;
            let card_id: i64 = row.get(4)?;
            let algorithm: String = row.get::<_, String>(12).unwrap_or_else(|_| "sm2".to_string());

            // For display purposes, we'll always return the SM2 format since our UI expects this
            let flashcard = Flashcard {
                question: row.get(5)?,
                answer: row.get(6)?,
                guidance: row.get(7)?,
                interval: row.get(8)?,
                repetitions: row.get(9)?,
                ease_factor: row.get(10)?,
                next_review: row.get(11)?,
                id: Some(card_id),  // Set the ID field
            };

            Ok((user_id, username, deck_id, deck_name, flashcard, algorithm))
        })?;

        let mut results = Vec::new();
        for card in card_iter {
            results.push(card?);
        }

        Ok(results)
    }

    // Also needed for the Find command:
    pub fn get_fsrs_fields(&self, card_id: i64) -> Result<(f64, f64, f64)> {
        self.conn.query_row(
            "SELECT difficulty, stability, retrievability
             FROM flashcards
             WHERE id = ?",
            params![card_id],
            |row| Ok((
                row.get::<_, f64>(0).unwrap_or(5.0),
                row.get::<_, f64>(1).unwrap_or(0.0),
                row.get::<_, f64>(2).unwrap_or(1.0),
            ))
        ).map_err(|e| anyhow::anyhow!("Failed to get FSRS fields: {}", e))
    }

    // And for the Import command with algorithm option:
    pub fn import_flashcards_from_csv_with_algorithm(
        &self,
        user_id: i64,
        deck_name: &str,
        csv_path: &str,
        algorithm: Option<&str>,
    ) -> Result<(usize, usize, Vec<String>)> {
        // Create or get deck with specified algorithm
        let algorithm_name = algorithm.unwrap_or("sm2");
        
        // Validate algorithm name
        if algorithm_name != "sm2" && algorithm_name != "fsrs" {
            return Err(anyhow::anyhow!("Invalid algorithm: {}. Valid options are 'sm2' or 'fsrs'", algorithm_name));
        }
        
        // Check if the deck already exists
        let deck_id = match self.get_deck_id(user_id, deck_name) {
            Ok(id) => {
                // If deck exists but algorithm is specified, warn that we're not changing it
                if algorithm.is_some() {
                    let current_algo = self.get_deck_algorithm(id)?;
                    if current_algo != algorithm_name {
                        println!("Warning: Not changing existing deck algorithm from {} to {}", 
                                  current_algo, algorithm_name);
                    }
                }
                id
            },
            Err(_) => {
                // Create new deck with specified algorithm
                println!("Creating new deck '{}' with {} algorithm", deck_name, algorithm_name);
                self.create_deck_with_algorithm(user_id, deck_name, algorithm_name)?
            }
        };
        
        // Import flashcards to the deck
        self.import_flashcards_from_csv(user_id, deck_id, csv_path)
    }

    pub fn analyze_review_order(&self, deck_id: i64) -> Result<String> {
        let now = get_current_timestamp()?;
        
        // First check which algorithm this deck uses
        let algorithm = self.get_deck_algorithm(deck_id)?;
        
        if algorithm == "fsrs" {
            // For FSRS, we'll use the queue-based approach that matches the FSRS 80/10/10 strategy
            // Get cards by queue (new, learning, review)
            let (new_cards, learning_cards, review_cards) = self.get_due_cards_by_queue(deck_id)?;
            
            let total_cards = new_cards.len() + learning_cards.len() + review_cards.len();
            
            if total_cards == 0 {
                return Ok("No due cards found.".to_string());
            }
            
            // Create concise output
            let mut output = String::new();
            output.push_str(&format!("FSRS Review Analysis: {} cards due\n", total_cards));
            output.push_str(&format!("- Learning cards: {} (should be reviewed first)\n", learning_cards.len()));
            output.push_str(&format!("- New cards: {} (introduce gradually)\n", new_cards.len()));
            output.push_str(&format!("- Review cards: {} (maintain long-term retention)\n", review_cards.len()));
            
            // Display recommended allocation based on FSRS 80/10/10 strategy
            output.push_str("\nRecommended FSRS Review Strategy:\n");
            
            let batch_size = 20; // Example batch size
            let learning_allocation = (batch_size as f64 * 0.8).ceil() as usize;
            let new_allocation = (batch_size as f64 * 0.1).ceil() as usize;
            let review_allocation = batch_size - learning_allocation - new_allocation;
            
            output.push_str(&format!("For a batch of {} cards, aim for:\n", batch_size));
            output.push_str(&format!("- {} learning cards (80%)\n", learning_allocation));
            output.push_str(&format!("- {} new cards (10%)\n", new_allocation));
            output.push_str(&format!("- {} review cards (10%)\n", review_allocation));
            
            // Show samples from each queue
            if !learning_cards.is_empty() {
                output.push_str("\nExample Learning Cards (highest priority):\n");
                let mut learning_table = Table::new();
                learning_table.add_row(row!["ID", "Question", "Reps", "Retrievability"]);
                
                let display_limit = std::cmp::min(3, learning_cards.len());
                for i in 0..display_limit {
                    let (card_id, card) = &learning_cards[i];
                    // Get FSRS fields
                    let (_, _stability, retrievability) = self.get_fsrs_fields(*card_id).unwrap_or((5.0, 0.0, 1.0));
                    
                    // Truncate question if too long
                    let display_question = if card.question.len() > 20 {
                        format!("{}...", &card.question[0..17])
                    } else {
                        card.question.clone()
                    };
                    
                    learning_table.add_row(row![
                        card_id,
                        display_question,
                        card.repetitions,
                        format!("{:.1}%", retrievability * 100.0)
                    ]);
                }
                
                output.push_str(&learning_table.to_string());
            }
            
            // Show recommended review order summary
            output.push_str("\n\nOptimized Review Order:\n");
            output.push_str("1. Learning cards (ordered by retrievability - lowest first)\n");
            output.push_str("2. Mix of new cards and review cards\n");
            
            output.push_str("\nThis 80/10/10 strategy helps balance:\n");
            output.push_str("- Learning progress (80% focus on active learning cards)\n");
            output.push_str("- Vocabulary growth (10% new cards)\n");
            output.push_str("- Long-term retention (10% review cards)\n");
            
            Ok(output)
        } else {
            // For SM2, use the original priority-based approach
            // ...existing code for the SM2 algorithm...
            // Fetch due cards ordered by current algorithm
            let mut stmt = self.conn.prepare(
                "SELECT id, question, repetitions, ease_factor, next_review, 
                 (next_review - ?) as overdue_seconds,
                 interval, difficulty, stability, retrievability
                 FROM flashcards
                 WHERE deck_id = ? AND next_review <= ?
                 ORDER BY repetitions ASC, next_review ASC"
            )?;
            
            let card_iter = stmt.query_map(params![now, deck_id, now], |row| {
                Ok((
                    row.get::<_, i64>(0)?,                     // id
                    row.get::<_, String>(1)?,                  // question
                    row.get::<_, u32>(2)?,                     // repetitions
                    row.get::<_, f32>(3)?,                     // ease_factor
                    row.get::<_, u64>(4)?,                     // next_review
                    row.get::<_, i64>(5)?,                     // overdue_seconds
                    row.get::<_, u32>(6)?,                     // interval
                    row.get::<_, f64>(7).unwrap_or(5.0),       // difficulty
                    row.get::<_, f64>(8).unwrap_or(1.0),       // stability
                    row.get::<_, f64>(9).unwrap_or(1.0),       // retrievability
                ))
            })?;
            
            let cards = card_iter.collect::<Result<Vec<_>, _>>()?;
            let total_cards = cards.len();
            
            if cards.is_empty() {
                return Ok("No due cards found.".to_string());
            }
            
            // Create a vector to store card data along with calculated priority
            let mut cards_with_priority = Vec::new();
            
            // Generate analysis data with SM2 priority calculation
            for (id, question, repetitions, ease_factor, next_review, overdue_seconds, interval, _, _, _) in &cards {
                // Original SM2 priority calculation
                let interval_seconds = (*interval as i64) * 86400; // interval in seconds
                let overdue_percentage = if interval_seconds > 0 {
                    (*overdue_seconds as f64 / interval_seconds as f64) * 100.0
                } else {
                    100.0 // If interval is 0, consider it 100% overdue
                };
                
                // Normalize repetitions impact (fewer repetitions = higher priority)
                let repetition_factor = 1.0 / (*repetitions as f64 + 1.0);
                
                // Original priority score: weighted combination
                let priority = (overdue_percentage * 0.7) + (repetition_factor * 100.0 * 0.3);
                
                // Store card data with its priority
                cards_with_priority.push((*id, question.clone(), *repetitions, *ease_factor, 
                                      *next_review, *overdue_seconds, *interval, priority));
            }
            
            // Calculate ordering issues by comparing adjacent priorities
            let mut ordering_issues = Vec::new();
            for i in 1..cards_with_priority.len() {
                let (id, _, _, _, _, _, _, priority) = cards_with_priority[i];
                let (_, _, _, _, _, _, _, prev_priority) = cards_with_priority[i-1];
                if priority > prev_priority {  // Higher priority should come first
                    ordering_issues.push((i, id, priority, prev_priority));
                }
            }
            
            // Create concise output - show only summary and limited examples
            let mut output = String::new();
            output.push_str(&format!("Due Cards Analysis: {} cards due for review\n", total_cards));
            
            // Display only up to 5 example cards from the current order
            if !cards_with_priority.is_empty() {
                output.push_str("\nCurrent Review Order (sample):\n");
                let mut sample_table = Table::new();
                sample_table.add_row(row!["ID", "Question", "Reps", "Overdue By", "Priority"]);
                
                // Show at most 5 example cards
                let display_limit = std::cmp::min(5, cards_with_priority.len());
                for i in 0..display_limit {
                    let (id, question, repetitions, _, _, overdue_seconds, _, priority) = &cards_with_priority[i];
                    
                    // Format overdue display more concisely
                    let overdue_display = if *overdue_seconds < 3600 {
                        format!("{}m", overdue_seconds / 60)
                    } else if *overdue_seconds < 86400 {
                        format!("{:.1}h", *overdue_seconds as f64 / 3600.0)
                    } else {
                        format!("{:.1}d", *overdue_seconds as f64 / 86400.0)
                    };
                    
                    // Truncate question if too long
                    let display_question = if question.len() > 20 {
                        format!("{}...", &question[0..17])
                    } else {
                        question.clone()
                    };
                    
                    sample_table.add_row(row![
                        id,
                        display_question,
                        repetitions,
                        overdue_display,
                        format!("{:.1}", priority)
                    ]);
                }
                
                // If there are more cards, indicate that
                if cards_with_priority.len() > display_limit {
                    sample_table.add_row(row![
                        "...",
                        format!("({} more)", total_cards - display_limit),
                        "", "", ""
                    ]);
                }
                
                output.push_str(&sample_table.to_string());
            }
            
            // If ordering issues exist, provide a brief recommendation summary
            if !ordering_issues.is_empty() {
                // Sort by priority in descending order for ideal order
                cards_with_priority.sort_by(|a, b| {
                    let (_, _, _, _, _, _, _, priority_a) = a;
                    let (_, _, _, _, _, _, _, priority_b) = b;
                    priority_b.partial_cmp(priority_a).unwrap_or(std::cmp::Ordering::Equal)
                });
                
                output.push_str(&format!("\n\nFound {} cards that might benefit from reordering.\n", ordering_issues.len()));
                
                // Only show a few examples of the recommended order
                output.push_str("\nRecommended Order (sample):\n");
                let mut rec_table = Table::new();
                rec_table.add_row(row!["ID", "Question", "Reps", "Overdue By", "Priority"]);
                
                // Show at most 5 example cards from the recommended order
                let display_limit = std::cmp::min(5, cards_with_priority.len());
                for i in 0..display_limit {
                    let (id, question, repetitions, _, _, overdue_seconds, _, priority) = &cards_with_priority[i];
                    
                    // Format overdue display more concisely
                    let overdue_display = if *overdue_seconds < 3600 {
                        format!("{}m", overdue_seconds / 60)
                    } else if *overdue_seconds < 86400 {
                        format!("{:.1}h", *overdue_seconds as f64 / 3600.0)
                    } else {
                        format!("{:.1}d", *overdue_seconds as f64 / 86400.0)
                    };
                    
                    // Truncate question if too long
                    let display_question = if question.len() > 20 {
                        format!("{}...", &question[0..17])
                    } else {
                        question.clone()
                    };
                    
                    rec_table.add_row(row![
                        id,
                        display_question,
                        repetitions,
                        overdue_display,
                        format!("{:.1}", priority)
                    ]);
                }
                
                // If there are more cards, indicate that
                if cards_with_priority.len() > display_limit {
                    rec_table.add_row(row![
                        "...",
                        format!("({} more)", total_cards - display_limit),
                        "", "", ""
                    ]);
                }
                
                output.push_str(&rec_table.to_string());
                
                output.push_str("\n\nAnalysis Summary:\n");
                output.push_str(&format!("- {} cards need review\n", total_cards));
                output.push_str(&format!("- {} cards ({:.1}%) would benefit from reordering\n", 
                    ordering_issues.len(), 
                    (ordering_issues.len() as f64 / total_cards as f64) * 100.0));
            } else {
                output.push_str("\n\nAnalysis Summary:\n");
                output.push_str("✅ The current review order appears optimal.\n");
                output.push_str("Cards are properly sequenced based on priority factors.\n");
            }
            
            output.push_str("\nSM2 Priority Factors:\n");
            output.push_str("- Repetition count: Fewer repetitions prioritized\n");
            output.push_str("- Overdue status: More overdue cards prioritized\n");
            
            Ok(output)
        }
    }

    pub fn create_deck_with_algorithm(&self, user_id: i64, name: &str, algorithm: &str) -> Result<i64> {
        let timestamp = get_current_timestamp()?;

        // Check if the deck already exists
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM decks WHERE user_id = ?1 AND name = ?2")?;
        let existing_deck_id: Result<i64, _> =
            stmt.query_row(params![user_id, name], |row| row.get(0));

        match existing_deck_id {
            Ok(deck_id) => {
                // Deck already exists, return its ID
                Ok(deck_id)
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // Deck does not exist, create a new one
                self.conn
                    .execute(
                        "INSERT INTO decks (user_id, name, created_at, algorithm) VALUES (?1, ?2, ?3, ?4)",
                        params![user_id, name, timestamp, algorithm],
                    )
                    .context("Failed to create deck")?;

                let deck_id = self.conn.last_insert_rowid();
                self.log_operation(user_id, "CREATE", "DECK", deck_id, Some(&format!("{} (algorithm: {})", name, algorithm)))?;
                Ok(deck_id)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_deck_algorithm(&self, deck_id: i64) -> Result<String> {
        let algorithm = self.conn.query_row(
            "SELECT algorithm FROM decks WHERE id = ?",
            params![deck_id],
            |row| row.get::<_, String>(0),
        )?;
        Ok(algorithm)
    }

    pub fn update_deck_algorithm(&self, deck_id: i64, algorithm: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE decks SET algorithm = ? WHERE id = ?",
            params![algorithm, deck_id],
        )?;
        Ok(())
    }

    pub fn analyze_fsrs_recommendations(&self, user_id: i64) -> Result<String> {
        // Get user's decks
        let mut stmt = self.conn.prepare(
            "SELECT id, name FROM decks WHERE user_id = ? ORDER BY name"
        )?;
        
        let deck_iter = stmt.query_map(params![user_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        
        let mut output = String::new();
        output.push_str("FSRS Algorithm Recommendations\n");
        output.push_str("=============================\n\n");
        
        let mut table = Table::new();
        table.add_row(row!["Deck ID", "Deck Name", "Cards", "Recommendation"]);
        
        for deck_result in deck_iter {
            let (deck_id, deck_name) = deck_result?;
            let algorithm = self.get_deck_algorithm(deck_id)?;
            
            if algorithm == "fsrs" {
                table.add_row(row![deck_id, deck_name, "-", "Already using FSRS ✓"]);
                continue;
            }
            
            // Get card counts and review statistics
            let (total_cards, review_count): (i64, i64) = self.conn.query_row(
                "SELECT COUNT(f.id),
                 (SELECT COUNT(*) FROM reviews r WHERE r.flashcard_id IN (SELECT id FROM flashcards WHERE deck_id = ?))
                 FROM flashcards f
                 WHERE f.deck_id = ?",
                params![deck_id, deck_id],
                |row| Ok((row.get(0)?, row.get(1)?))
            )?;
            
            // Make recommendation based on card count and review history
            let recommendation = if total_cards > 100 || review_count > 500 {
                format!("Highly Recommended ⭐⭐⭐ (convert-to-fsrs --deck-id {})", deck_id)
            } else if total_cards > 30 || review_count > 100 {
                format!("Recommended ⭐⭐ (convert-to-fsrs --deck-id {})", deck_id)
            } else if total_cards > 0 {
                "Optional (small deck, SM2 may be sufficient)".to_string()
            } else {
                "Empty deck".to_string()
            };
            
            table.add_row(row![deck_id, deck_name, total_cards, recommendation]);
        }
        
        output.push_str(&table.to_string());
        output.push_str("\n\nFSRS works best for:");
        output.push_str("\n• Decks with many cards (>100)");
        output.push_str("\n• Cards with long intervals");
        output.push_str("\n• When precise retention is important");
        output.push_str("\n• When you have cards of varying difficulty");
        
        Ok(output)
    }

    pub fn get_performance_trend(&self, user_id: i64, deck_id: i64) -> Result<String> {
        // Get performance over time, grouped by week
        let mut stmt = self.conn.prepare(
            "SELECT 
                strftime('%Y-W%W', datetime(timestamp, 'unixepoch')) as week,
                COUNT(*) as review_count,
                AVG(performance) as avg_performance
             FROM reviews r
             JOIN flashcards f ON r.flashcard_id = f.id
             WHERE r.user_id = ? AND f.deck_id = ?
             GROUP BY week
             ORDER BY week ASC"
        )?;

        let week_iter = stmt.query_map(params![user_id, deck_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?;

        let mut output = String::new();
        output.push_str(&format!("Performance Trend Analysis\n"));
        output.push_str("==========================\n\n");
        
        let mut trend_table = Table::new();
        trend_table.add_row(row!["Week", "Reviews", "Avg Performance", "Trend"]);
        
        let mut prev_performance = 0.0;
        let mut first_week = true;
        
        for week_result in week_iter {
            let (week, count, avg) = week_result?;
            
            // Calculate trend indicator
            let trend = if first_week {
                first_week = false;
                "—"
            } else if avg > prev_performance + 0.2 {
                "↑↑" // Significant improvement
            } else if avg > prev_performance + 0.1 {
                "↑"  // Slight improvement
            } else if avg < prev_performance - 0.2 {
                "↓↓" // Significant decline
            } else if avg < prev_performance - 0.1 {
                "↓"  // Slight decline
            } else {
                "→"  // Stable
            };
            
            trend_table.add_row(row![week, count, format!("{:.2}", avg), trend]);
            prev_performance = avg;
        }
        
        output.push_str(&trend_table.to_string());
        
        // Add retention analysis for FSRS decks
        let algorithm = self.get_deck_algorithm(deck_id)?;
        if algorithm == "fsrs" {
            output.push_str("\n\nFSRS Retention Analysis\n");
            output.push_str("=====================\n\n");
            
            let avg_retrievability: f64 = self.conn.query_row(
                "SELECT AVG(retrievability) FROM flashcards WHERE deck_id = ?",
                params![deck_id],
                |row| row.get(0)
            )?;
            
            output.push_str(&format!("Average memory retention rate: {:.1}%\n", avg_retrievability * 100.0));
            
            // Distribution of retrievability
            output.push_str("\nRetention Distribution:\n");
            let mut ret_table = Table::new();
            ret_table.add_row(row!["Retention Range", "Card Count", "Percentage"]);
            
            let ranges = [
                (0.0, 0.5, "< 50% (At risk)"),
                (0.5, 0.7, "50-70% (Weak)"),
                (0.7, 0.85, "70-85% (Fair)"),
                (0.85, 0.95, "85-95% (Good)"),
                (0.95, 1.01, "> 95% (Strong)"),
            ];
            
            for (min, max, label) in ranges {
                let (count, percentage): (i64, f64) = self.conn.query_row(
                    "SELECT COUNT(*), 
                     ROUND(COUNT(*) * 100.0 / (SELECT COUNT(*) FROM flashcards WHERE deck_id = ?), 2)
                     FROM flashcards 
                     WHERE deck_id = ? AND retrievability >= ? AND retrievability < ?",
                    params![deck_id, deck_id, min, max],
                    |row| Ok((row.get(0)?, row.get(1)?))
                )?;
                
                ret_table.add_row(row![label, count, format!("{:.2}%", percentage)]);
            }
            
            output.push_str(&ret_table.to_string());
        }
        
        Ok(output)
    }

    pub fn get_flashcard(&self, card_id: i64) -> Result<Flashcard> {
        let mut stmt = self.conn.prepare(
            "SELECT question, answer, guidance, interval, repetitions, ease_factor, next_review
             FROM flashcards
             WHERE id = ?"
        )?;
        
        stmt.query_row(params![card_id], |row| {
            Ok(Flashcard {
                question: row.get(0)?,
                answer: row.get(1)?,
                guidance: row.get(2)?,
                interval: row.get(3)?,
                repetitions: row.get(4)?,
                ease_factor: row.get(5)?,
                next_review: row.get(6)?,
                id: Some(card_id),
            })
        }).map_err(|e| anyhow::anyhow!("Failed to retrieve flashcard: {}", e))
    }
    
    pub fn log_review_session(&self, user_id: i64, deck_id: i64, cards_reviewed: usize) -> Result<i64> {
        let timestamp = get_current_timestamp()?;
        
        // Insert session log
        self.conn.execute(
            "INSERT INTO operations_log (user_id, operation_type, entity_type, entity_id, details, timestamp)
             VALUES (?, 'SESSION', 'DECK', ?, ?, ?)",
            params![user_id, deck_id, format!("Reviewed {} cards", cards_reviewed), timestamp],
        )?;
        
        Ok(self.conn.last_insert_rowid())
    }
    
    pub fn get_review_history(&self, user_id: i64, deck_id: i64, limit: usize) -> Result<Vec<(u64, String, i32, u32, u32, u64, String, i64)>> {
        // Add old_interval to reviews table if it doesn't exist yet
        self.add_old_interval_column_if_needed()?;
        
        // Get reviews with their before/after state
        let mut stmt = self.conn.prepare(
            "SELECT 
                r.timestamp,
                f.question,
                r.performance,
                COALESCE(r.old_interval, f.interval) as old_interval,
                f.interval as new_interval,
                f.next_review,
                d.algorithm,
                f.id as card_id
             FROM reviews r
             JOIN flashcards f ON r.flashcard_id = f.id
             JOIN decks d ON f.deck_id = d.id
             WHERE r.user_id = ? AND f.deck_id = ?
             ORDER BY r.timestamp DESC
             LIMIT ?"
        )?;
        
        let rows = stmt.query_map(params![user_id, deck_id, limit as i64], |row| {
            Ok((
                row.get(0)?, // timestamp
                row.get(1)?, // question
                row.get(2)?, // performance
                row.get(3)?, // old_interval
                row.get(4)?, // new_interval
                row.get(5)?, // next_review
                row.get(6)?, // algorithm
                row.get(7)?, // card_id
            ))
        })?;
        
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        
        Ok(result)
    }

    // Helper method to check and add old_interval column to reviews table
    fn add_old_interval_column_if_needed(&self) -> Result<()> {
        let has_old_interval = self.conn
            .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='reviews'")?
            .query_row([], |row| {
                let table_sql: String = row.get(0)?;
                Ok(table_sql.contains("old_interval"))
            })
            .unwrap_or(false);

        if !has_old_interval {
            println!("Upgrading database: Adding 'old_interval' column to 'reviews' table...");
            self.conn.execute(
                "ALTER TABLE reviews ADD COLUMN old_interval INTEGER",
                [],
            ).context("Failed to add old_interval column to reviews table")?;
        }
        
        Ok(())
    }

    // Helper for getting upcoming reviews schedule
    pub fn get_upcoming_reviews(&self, _user_id: i64, deck_id: i64) -> Result<Vec<(String, i64)>> {
        let now = get_current_timestamp()?;
        let thirty_days = now + (30 * 86400);
        
        let mut stmt = self.conn.prepare(
            "SELECT 
                strftime('%Y-%m-%d', datetime(next_review, 'unixepoch')) as review_date,
                COUNT(*) as card_count
             FROM flashcards
             WHERE deck_id = ? 
               AND next_review BETWEEN ? AND ?
             GROUP BY review_date
             ORDER BY review_date ASC"
        )?;
        
        let rows = stmt.query_map(params![deck_id, now, thirty_days], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        
        Ok(result)
    }
    
    pub fn get_interval_statistics(&self, deck_id: i64) -> Result<Vec<(String, i64, f64)>> {
        // Define interval ranges for better analysis
        let ranges = [
            (0, 0, "New (0 days)"),
            (1, 1, "Learning (1 day)"),
            (2, 7, "Short-term (2-7 days)"),
            (8, 30, "Medium-term (8-30 days)"),
            (31, 90, "Long-term (31-90 days)"),
            (91, 180, "Extended (91-180 days)"),
            (181, 365, "Long (181-365 days)"),
            (366, i32::MAX, "Very long (>365 days)"),
        ];
        
        let mut result = Vec::new();
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM flashcards WHERE deck_id = ?",
            params![deck_id],
            |row| row.get(0)
        )?;
        
        for (min, max, label) in ranges.iter() {
            let where_clause = if *max == i32::MAX {
                format!("interval >= {}", min)
            } else {
                format!("interval BETWEEN {} AND {}", min, max)
            };
            
            let count: i64 = self.conn.query_row(
                &format!("SELECT COUNT(*) FROM flashcards WHERE deck_id = ? AND {}", where_clause),
                params![deck_id],
                |row| row.get(0)
            )?;
            
            let percentage = if total > 0 {
                (count as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            
            result.push((label.to_string(), count, percentage));
        }
        
        Ok(result)
    }

    pub fn analyze_learning_efficiency(&self, user_id: i64, deck_id: i64) -> Result<String> {
        // Calculate learning efficiency metrics
        let (_total_reviews, total_time, cards_learned) = self.conn.query_row(
            "SELECT COUNT(*) as review_count,
             SUM((julianday(datetime(timestamp + 60, 'unixepoch')) - 
                  julianday(datetime(timestamp, 'unixepoch'))) * 24 * 60) as minutes,
             COUNT(DISTINCT flashcard_id) as cards
             FROM reviews r
             JOIN flashcards f ON r.flashcard_id = f.id
             WHERE r.user_id = ? AND f.deck_id = ?",
            params![user_id, deck_id],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, f64>(1).unwrap_or(0.0),
                row.get::<_, i64>(2)?
            ))
        )?;
        
        // Calculate retention metrics
        let (avg_retention, _) = self.conn.query_row(
            "SELECT AVG(retrievability) * 100, COUNT(*)
             FROM flashcards
             WHERE deck_id = ? AND repetitions > 0",
            params![deck_id],
            |row| Ok((row.get::<_, f64>(0).unwrap_or(0.0), row.get::<_, i64>(1)?))
        )?;
        
        // Calculate efficiency metrics
        let cards_per_hour = if total_time > 0.0 {
            cards_learned as f64 / (total_time / 60.0)
        } else { 0.0 };
        
        let mut output = String::new();
        output.push_str(&format!("Learning Efficiency Analysis\n"));
        output.push_str("============================\n\n");
        
        let mut efficiency_table = Table::new();
        efficiency_table.add_row(row!["Metric", "Value"]);
        efficiency_table.add_row(row!["Average retention", format!("{:.1}%", avg_retention)]);
        efficiency_table.add_row(row!["Cards learned per study hour", format!("{:.1}", cards_per_hour)]);
        efficiency_table.add_row(row!["Total study time", format!("{:.1} hours", total_time / 60.0)]);
        
        output.push_str(&efficiency_table.to_string());
        Ok(output)
    }
    
    pub fn balance_review_load(&self, user_id: i64, days_ahead: u32) -> Result<String> {
        let now = get_current_timestamp()?;
        let end_period = now + (days_ahead as u64 * 86400);
        
        // Get current distribution of cards due in the upcoming period
        let mut daily_counts = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT 
                strftime('%Y-%m-%d', datetime(next_review, 'unixepoch')) as review_date,
                COUNT(*) as card_count
             FROM flashcards f
             JOIN decks d ON f.deck_id = d.id
             WHERE d.user_id = ? AND f.next_review BETWEEN ? AND ?
               AND d.algorithm = 'fsrs'
             GROUP BY review_date
             ORDER BY review_date"
        )?;
        
        let rows = stmt.query_map(params![user_id, now, end_period], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        
        for row in rows {
            daily_counts.push(row?);
        }
        
        // Find days with excessive reviews and days with few reviews
        let mut output = String::new();
        output.push_str(&format!("Review Load Analysis for Next {} Days\n\n", days_ahead));
        
        let max_per_day = 50; // Reasonable maximum reviews per day
        let mut table = Table::new();
        table.add_row(row!["Date", "Cards Due", "Status"]);
        
        let mut has_imbalance = false;
        let mut total_cards = 0;
        
        for (date, count) in &daily_counts {
            let status = if *count > max_per_day {
                has_imbalance = true;
                format!("⚠️ Overloaded (+{})", count - max_per_day)
            } else if *count<5 {
                "🟢 Light load".to_string()
            } else{
                format!("✓ Balanced")
            };
            table.add_row(row![date, count, status]);
            total_cards += count;
        }
        
        output.push_str(&table.to_string());
        
        // If we found imbalance, suggest rebalancing
        if has_imbalance {
            // Calculate ideal distribution
            let days = days_ahead as i64;
            let ideal_per_day = (total_cards as f64 / days as f64).ceil() as i64;
            
            output.push_str("\n\nRecommended Action: Rebalance review load\n");
            output.push_str(&format!("Ideal cards per day: {} (total {} cards over {} days)\n", 
                                  ideal_per_day, total_cards, days));
            
            // Suggest adjustment
            output.push_str("\nTo rebalance, consider using the 'rebalance' command:\n");
            output.push_str(&format!("$ words rebalance --user-id {} --days {}\n", user_id, days_ahead));
        } else {
            output.push_str("\n\nReview load is well balanced! 👍\n");
        }
        
        Ok(output)
    }

    pub fn _track_review_timing(&self, card_id: i64, response_time_ms: i64) -> Result<()> {
        // Check if the review_metrics table exists, if not create it
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS review_metrics (
                id INTEGER PRIMARY KEY,
                review_id INTEGER,
                response_time_ms INTEGER,
                timestamp INTEGER,
                FOREIGN KEY(review_id) REFERENCES reviews(id)
            )",
            [],
        )?;
        
        // Get the most recent review ID for this card
        let review_id: i64 = self.conn.query_row(
            "SELECT id FROM reviews WHERE flashcard_id = ? ORDER BY timestamp DESC LIMIT 1",
            params![card_id],
            |row| row.get(0)
        )?;
        
        // Insert timing data
        self.conn.execute(
            "INSERT INTO review_metrics (review_id, response_time_ms, timestamp)
             VALUES (?, ?, ?)",
            params![review_id, response_time_ms, get_current_timestamp()?],
        )?;
        
        // Update difficulty based on response time
        if let Ok((difficulty, _, _)) = self.get_fsrs_fields(card_id) {
            // Adjust difficulty based on response time
            // Fast responses (< 3s) suggest the card is easier
            // Slow responses (> 10s) suggest the card is harder
            let normalized_time = response_time_ms as f64 / 1000.0;
            let difficulty_delta = if normalized_time < 3.0 {
                -0.1 // Decrease difficulty slightly for fast responses
            } else if normalized_time > 10.0 {
                0.1 // Increase difficulty slightly for slow responses
            } else {
                0.0 // No change for normal responses
            };
            
            if difficulty_delta != 0.0 {
                let new_difficulty = (difficulty + difficulty_delta).max(1.0).min(10.0);
                self.conn.execute(
                    "UPDATE flashcards SET difficulty = ? WHERE id = ?",
                    params![new_difficulty, card_id],
                )?;
            }
        }
        
        Ok(())
    }

// Advanced FSRS feature: Card queue management system
    // Separates cards into three learning stages for better review prioritization
    //
    // Usage: 
    // ```
    // let (new_cards, learning_cards, review_cards) = db.get_due_cards_by_queue(deck_id)?;
    // ```
    //
    // Returns:
    // - new_queue: Cards that have never been reviewed (repetitions = 0)
    // - learning_queue: Cards being learned (stability ≤ 7 days)
    // - review_queue: Established cards (stability > 7 days)
    //
    // This provides more nuanced review scheduling than SM2 by prioritizing:
    // 1. New cards to introduce fresh material
    // - Limited to 20 cards to avoid overwhelming the user
    //
    // 2. Learning cards that need reinforcement
    // - Cards with low stability (≤ 7 days) that need frequent review
    // - Ordered by retrievability (lowest first) to prioritize at-risk memories
    // - Limited to 20 cards to maintain focused learning
    //
    // 3. Review cards for long-term retention
    // - Cards with established stability (> 7 days)
    // - Ordered by retrievability to catch cards before they're forgotten
    // - Higher limit (40 cards) since these require less time per review
    //
    // Example usage in a review command:
    // ```
    // // Get cards separated by learning stage
    // let (new_cards, learning_cards, review_cards) = db.get_due_cards_by_queue(deck_id)?;
    // 
    // // Show learning stage progress to user
    // println!("Today's review plan:");
    // println!("- New cards: {}", new_cards.len());
    // println!("- Learning cards: {}", learning_cards.len());
    // println!("- Review cards: {}", review_cards.len());
    //
    // // First review learning cards (highest priority)
    // for (card_id, card) in &learning_cards {
    //     // review card...
    // }
    // 
    // // Mix new cards and review cards
    // let mut remaining_cards = new_cards;
    // remaining_cards.extend(review_cards);
    // for (card_id, card) in &remaining_cards {
    //     // review card...
    // }
    // ```
    pub fn get_due_cards_by_queue(&self, deck_id: i64) -> Result<(Vec<(i64, Flashcard)>, Vec<(i64, Flashcard)>, Vec<(i64, Flashcard)>)> {
        let now = get_current_timestamp()?;
        let algorithm = self.get_deck_algorithm(deck_id)?;
        
        if algorithm != "fsrs" {
            // For non-FSRS decks, just return all cards in a single queue
            let all_cards = self.get_due_flashcards(deck_id)?;
            return Ok((all_cards, vec![], vec![]));
        }
        
        // New cards - Never reviewed (repetitions = 0)
        let mut new_stmt = self.conn.prepare(
            "SELECT id, question, answer, guidance, interval, repetitions, ease_factor, next_review,
                    difficulty, stability, retrievability
             FROM flashcards
             WHERE deck_id = ? AND repetitions = 0 AND next_review <= ?
            "
        )?;
        
        // Learning cards - Low stability (<= 7 days)
        let mut learning_stmt = self.conn.prepare(
            "SELECT id, question, answer, guidance, interval, repetitions, ease_factor, next_review,
                    difficulty, stability, retrievability
             FROM flashcards
             WHERE deck_id = ? AND repetitions > 0 AND stability <= 7.0 AND next_review <= ?
             ORDER BY retrievability ASC
             "
        )?;
        
        // Review cards - Established cards (stability > 7 days)
        let mut review_stmt = self.conn.prepare(
            "SELECT id, question, answer, guidance, interval, repetitions, ease_factor, next_review,
                    difficulty, stability, retrievability
             FROM flashcards
             WHERE deck_id = ? AND repetitions > 0 AND stability > 7.0 AND next_review <= ?
             ORDER BY retrievability ASC
             "
        )?;
        
        // Process each queue
        let mut new_queue = Vec::new();
        let mut learning_queue = Vec::new();
        let mut review_queue = Vec::new();
        
        // Helper function to process results
        let process_rows = |stmt: &mut rusqlite::Statement, params: &[&dyn rusqlite::ToSql], queue: &mut Vec<(i64, Flashcard)>| -> Result<()> {
            let rows = stmt.query_map(params, |row| {
                let card_id = row.get(0)?;
                Ok((
                    card_id,
                    Flashcard {
                        question: row.get(1)?,
                        answer: row.get(2)?,
                        guidance: row.get(3)?,
                        interval: row.get(4)?,
                        repetitions: row.get(5)?,
                        ease_factor: row.get(6)?,
                        next_review: row.get(7)?,
                        id: Some(card_id),
                    }
                ))
            })?;
            
            for card in rows {
                queue.push(card?);
            }
            
            Ok(())
        };
        
        // Fill each queue
        process_rows(&mut new_stmt, &[&deck_id, &now], &mut new_queue)?;
        process_rows(&mut learning_stmt, &[&deck_id, &now], &mut learning_queue)?;
        process_rows(&mut review_stmt, &[&deck_id, &now], &mut review_queue)?;
        
        Ok((new_queue, learning_queue, review_queue))
    }

    // Get distribution of cards by learning stage (new, learning, review)
    // This helps users understand the composition of their deck and track progress
    //
    // Usage:
    // ```
    // let queue_stats = db.get_queue_distribution(deck_id)?;
    // println!("{}", queue_stats);
    // ```
    pub fn get_queue_distribution(&self, deck_id: i64) -> Result<String> {
        let algorithm = self.get_deck_algorithm(deck_id)?;
        let mut output = String::new();
        
        output.push_str("\nLearning Stage Distribution:\n");
        
        let total_cards: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM flashcards WHERE deck_id = ?",
            params![deck_id],
            |row| row.get(0)
        )?;
        
        if total_cards == 0 {
            return Ok("No cards in deck.".to_string());
        }
        
        // Get due cards by queue to compare with totals
        // let now = get_current_timestamp()?;
        let (new_due, learning_due, review_due) = if algorithm == "fsrs" {
            self.get_due_cards_by_queue(deck_id)?
        } else {
            (vec![], vec![], vec![])
        };
        
        // Create a comprehensive table that shows both total and due counts
        let mut comprehensive_table = Table::new();
        comprehensive_table.add_row(row![
            "Stage", "Total Cards", "Due Cards", "Not Due", "Percentage", "Description"
        ]);
        
        // Get counts for different learning stages
        if algorithm == "fsrs" {
            // For FSRS, use stability-based classification
            let new_count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM flashcards 
                 WHERE deck_id = ? AND repetitions = 0",
                params![deck_id],
                |row| row.get(0)
            )?;
            
            let learning_count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM flashcards 
                 WHERE deck_id = ? AND repetitions > 0 AND stability <= 7.0",
                params![deck_id],
                |row| row.get(0)
            )?;
            
            let review_count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM flashcards 
                 WHERE deck_id = ? AND repetitions > 0 AND stability > 7.0",
                params![deck_id],
                |row| row.get(0)
            )?;
            
            // Calculate percentages
            let new_pct = (new_count as f64 / total_cards as f64) * 100.0;
            let learning_pct = (learning_count as f64 / total_cards as f64) * 100.0;
            let review_pct = (review_count as f64 / total_cards as f64) * 100.0;
            
            // Calculate not due counts
            let new_not_due = new_count - new_due.len() as i64;
            let learning_not_due = learning_count - learning_due.len() as i64;
            let review_not_due = review_count - review_due.len() as i64;
            
            // Add rows to table with clearer information
            comprehensive_table.add_row(row![
                "New", 
                new_count, 
                new_due.len(),
                new_not_due,
                format!("{:.1}%", new_pct),
                "Never reviewed"
            ]);
            
            comprehensive_table.add_row(row![
                "Learning", 
                learning_count, 
                learning_due.len(),
                learning_not_due,
                format!("{:.1}%", learning_pct),
                "In active learning (stability ≤ 7 days)"
            ]);
            
            comprehensive_table.add_row(row![
                "Review", 
                review_count, 
                review_due.len(),
                review_not_due,
                format!("{:.1}%", review_pct),
                "Established knowledge (stability > 7 days)"
            ]);
            
            output.push_str(&comprehensive_table.to_string());
        } else {
            // For SM2, use repetition-based classification (similar logic as before)
            // ...existing code for SM2...
            let mut dist_table = Table::new();
            dist_table.add_row(row!["Stage", "Count", "Percentage", "Description"]);
            
            let new_count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM flashcards 
                 WHERE deck_id = ? AND repetitions = 0",
                params![deck_id],
                |row| row.get(0)
            )?;
            
            let learning_count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM flashcards 
                 WHERE deck_id = ? AND repetitions BETWEEN 1 AND 3",
                params![deck_id],
                |row| row.get(0)
            )?;
            
            let review_count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM flashcards 
                 WHERE deck_id = ? AND repetitions > 3",
                params![deck_id],
                |row| row.get(0)
            )?;
            
            // Calculate percentages
            let new_pct = (new_count as f64 / total_cards as f64) * 100.0;
            let learning_pct = (learning_count as f64 / total_cards as f64) * 100.0;
            let review_pct = (review_count as f64 / total_cards as f64) * 100.0;
            
            // Add rows to table
            dist_table.add_row(row![
                "New", 
                new_count, 
                format!("{:.1}%", new_pct),
                "Never reviewed"
            ]);
            
            dist_table.add_row(row![
                "Learning", 
                learning_count, 
                format!("{:.1}%", learning_pct),
                "1-3 repetitions"
            ]);
            
            dist_table.add_row(row![
                "Review", 
                review_count, 
                format!("{:.1}%", review_pct),
                "4+ repetitions"
            ]);
            
            output.push_str(&dist_table.to_string());
            
            // Also show due cards for SM2 decks
            let due_cards = self.get_due_flashcards(deck_id)?;
            output.push_str(&format!("\n\nDue cards: {}/{} ({:.1}%)", 
                due_cards.len(), 
                total_cards,
                (due_cards.len() as f64 / total_cards as f64) * 100.0
            ));
        }
        
        // Add explanation about non-due cards
        if algorithm == "fsrs" {
            output.push_str("\n\nNote: 'Not Due' cards are still in their respective learning stages ");
            output.push_str("but scheduled for future review dates.");
        }
        
        Ok(output)
    }
    
}