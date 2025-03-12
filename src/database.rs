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

    pub fn create_deck(&self, user_id: i64, name: &str) -> Result<i64> {
        // By default, create SM2 decks for backward compatibility
        self.create_deck_with_algorithm(user_id, name, "sm2")
    }

    pub fn create_or_get_deck(&self, user_id: i64, deck_name: &str) -> Result<i64> {
        // Check if the deck already exists
        match self.get_deck_id(user_id, deck_name) {
            Ok(deck_id) => Ok(deck_id),
            Err(_) => {
                // Create with default SM2 algorithm
                self.create_deck_with_algorithm(user_id, deck_name, "sm2")
            }
        }
    }

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

    pub fn update_flashcard(&self, user_id: i64, card_id: i64, performance: i32) -> Result<()> {
        let timestamp = get_current_timestamp()?;

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
            difficulty, stability, retrievability, algorithm, _deck_id) = row;

        match algorithm.as_str() {
            "fsrs" => {
                // Use FSRS algorithm
                use crate::fsrs::{FsrsParameters, flashcard_to_fsrs};
                
                // Create FSRS card from database values using the direct conversion function
                let mut fsrs_card = flashcard_to_fsrs(
                    &Flashcard {
                        question,
                        answer,
                        guidance,
                        interval,
                        repetitions,
                        ease_factor,
                        next_review,
                    },
                    difficulty,
                    stability,
                    retrievability
                );
                
                // Update card using FSRS
                let params = FsrsParameters::default();
                fsrs_card.update(performance as u32, &params);
                
                // Update database with new values
                self.conn.execute(
                    "UPDATE flashcards 
                     SET interval = ?1, repetitions = ?2, next_review = ?3, 
                         difficulty = ?4, stability = ?5, retrievability = ?6
                     WHERE id = ?7",
                    params![
                        fsrs_card.get_interval_days(),
                        fsrs_card.review_count,
                        fsrs_card.next_review,
                        fsrs_card.state.difficulty,
                        fsrs_card.state.stability,
                        fsrs_card.state.retrievability,
                        card_id
                    ],
                )?;
            },
            _ => {
                // Use default SM2 algorithm
                use crate::sm2::Flashcard;
                
                let mut card = Flashcard {
                    question,
                    answer,
                    guidance,
                    interval,
                    repetitions,
                    ease_factor,
                    next_review,
                };
                
                // Use the SM2 algorithm implementation from Flashcard
                card.update(performance as u32);
                
                // Update database with new values
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
        }

        // Record the review (same for both algorithms)
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
            Some(&format!("Performance: {} (Algorithm: {})", performance, algorithm)),
        )?;

        Ok(())
    }

    pub fn get_due_flashcards(&self, deck_id: i64) -> Result<Vec<(i64, Flashcard)>> {
        let now = get_current_timestamp()?;

        // First get the algorithm used by this deck
        let mut algo_stmt = self.conn.prepare(
            "SELECT algorithm FROM decks WHERE id = ?"
        )?;

        let algorithm: String = algo_stmt.query_row(params![deck_id], |row| {
            row.get::<_, String>(0)
        }).unwrap_or_else(|_| "sm2".to_string());

        // Different query based on algorithm
        let query = if algorithm == "fsrs" {
            // For FSRS: Order by retrievability ASC (review cards most likely to be forgotten first)
            // Lower retrievability means higher risk of forgetting
            "SELECT id, question, answer, guidance, interval, repetitions, ease_factor, next_review,
                    difficulty, stability, retrievability
             FROM flashcards
             WHERE deck_id = ?1 AND next_review <= ?2
             ORDER BY retrievability ASC, next_review ASC"
        } else {
            // For SM2: Use the original ordering (by repetitions, then next_review)
            "SELECT id, question, answer, guidance, interval, repetitions, ease_factor, next_review,
                    difficulty, stability, retrievability
             FROM flashcards
             WHERE deck_id = ?1 AND next_review <= ?2
             ORDER BY repetitions ASC, next_review ASC"
        };

        let mut stmt = self.conn.prepare(query)?;

        let cards = stmt.query_map(params![deck_id, now], |row| {
            // We still use the standard Flashcard type for compatibility with existing UI
            Ok((
                row.get(0)?,
                Flashcard {
                    question: row.get(1)?,
                    answer: row.get(2)?,
                    guidance: row.get(3)?,
                    interval: row.get(4)?,
                    repetitions: row.get(5)?,
                    ease_factor: row.get(6)?,
                    next_review: row.get(7)?,
                }
            ))
        })?;

        let mut result = Vec::new();
        for card in cards {
            result.push(card?);
        }

        // Log retrieval with algorithm info
        self.log_operation(
            0, // System operation
            "RETRIEVE",
            "DUE_CARDS",
            deck_id,
            Some(&format!("Retrieved {} due cards using {} algorithm", result.len(), algorithm)),
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
        output.push_str("\n\nPerformance Distribution:\n");
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
    ) -> Result<()> {
        // Get the deck's algorithm type first
        let algorithm = self.get_deck_algorithm(deck_id)?;
        println!("Importing cards into deck with algorithm: {}", algorithm);
        
        let file = File::open(csv_path)?;
        let reader = BufReader::new(file);

        let mut cards_to_import = Vec::new();
        let mut invalid_lines = Vec::new();

        println!("Starting import from '{}'", csv_path);

        for (line_number, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split(" ~ ").collect();
            if parts.len() < 3 {
                println!("Skipping invalid line {}: {}", line_number + 1, line);
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

        for mut card in cards_to_import {
            let mut unique_question = card.question.clone();
            let mut suffix = 1;

            while existing_questions.contains(&unique_question) {
                unique_question = format!("{}({})", card.question, suffix);
                suffix += 1;
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
                    "Import completed. Total imported: {}, Total skipped: {}",
                    processed_cards.len(),
                    invalid_lines.len()
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

                Ok(())
            }
            Err(e) => {
                println!("Failed to import flashcards: {}", e);
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

        let algorithm = match result {
            Ok((_, alg)) => alg,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return Err(anyhow::anyhow!(
                    "Deck doesn't belong to user or doesn't exist"
                ));
            },
            Err(e) => return Err(e.into()),
        };

        let timestamp = get_current_timestamp()?;

        self.conn.execute_batch("BEGIN TRANSACTION;")?;

        // Insert cards based on the algorithm
        if algorithm == "fsrs" {
            // Use FSRS fields
            use crate::fsrs::{FsrsParameters, sm2_to_fsrs};
            // Create the params properly
            let _params = FsrsParameters::default();

            let mut stmt = self.conn.prepare(
                "INSERT INTO flashcards (
                    deck_id, question, answer, guidance, interval, repetitions, ease_factor, 
                    next_review, created_at, difficulty, stability, retrievability)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )?;

            for card in cards {
                // Convert SM2 parameters to FSRS parameters
                let memory_state = sm2_to_fsrs(card.interval, card.repetitions, card.ease_factor);
                
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
                    memory_state.difficulty,
                    memory_state.stability,
                    memory_state.retrievability
                ])?;
            }
        } else {
            // Use standard SM2 fields (for backward compatibility)
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
    ) -> Result<Vec<(i64, String, i64, String, Flashcard)>> {
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
            let _algorithm: String = row.get::<_, String>(12).unwrap_or_else(|_| "sm2".to_string());
            // since we're just returning SM2 format for display
            // For display purposes, we'll always return the SM2 format since our UI expects this));

            // For display purposes, we'll always return the SM2 format since our UI expects this
            let flashcard = Flashcard {
                question: row.get(5)?,
                answer: row.get(6)?,
                guidance: row.get(7)?,
                interval: row.get(8)?,
                repetitions: row.get(9)?,
                ease_factor: row.get(10)?,
                next_review: row.get(11)?,
            };

            Ok((user_id, username, deck_id, deck_name, flashcard))
        })?;

        let mut results = Vec::new();
        for card in card_iter {
            results.push(card?);
        }

        Ok(results)
    }

    pub fn analyze_review_order(&self, deck_id: i64) -> Result<String> {
        let now = get_current_timestamp()?;
            
        // Fetch due cards ordered by current algorithm (repetitions ASC, next_review ASC)
        let mut stmt = self.conn.prepare(
            "SELECT id, question, repetitions, ease_factor, next_review, 
             (next_review - ?) as overdue_seconds,
             interval
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
            ))
        })?;
        
        let cards = card_iter.collect::<Result<Vec<_>, _>>()?;
        if cards.is_empty() {
            return Ok("No due cards found.".to_string());
        }
        
        // Calculate overdue percentage (relative to interval)
        let mut output = String::new();
        let mut analysis_table = Table::new();
        analysis_table.add_row(row![
            "ID", "Question", "Reps", "Interval", "Overdue By", "Priority"
        ]);
        
        // Calculate priority metrics and potential issues
        let mut ordering_issues = Vec::new();
        let mut _prev_priority = f64::MIN;
        
        // Create a vector to store card data along with calculated priority
        let mut cards_with_priority = Vec::new();
        
        // Generate analysis data
        for (id, question, repetitions, ease_factor, next_review, overdue_seconds, interval) in &cards {
            // Calculate priority metric: combination of repetitions and overdue percentage
            let interval_seconds = (*interval as i64) * 86400; // interval in seconds
            let overdue_percentage = if interval_seconds > 0 {
                (*overdue_seconds as f64 / interval_seconds as f64) * 100.0
            } else {
                100.0 // If interval is 0, consider it 100% overdue
            };
            
            // Normalize repetitions impact (fewer repetitions = higher priority)
            let repetition_factor = 1.0 / (*repetitions as f64 + 1.0);
            
            // Priority score: weighted combination of overdue percentage and repetition factor
            let priority = (overdue_percentage * 0.7) + (repetition_factor * 100.0 * 0.3);
            
            // Store card data with its priority
            cards_with_priority.push((*id, question.clone(), *repetitions, *ease_factor, 
                                  *next_review, *overdue_seconds, *interval, priority));
            
            // For display purposes
            let overdue_display = if *overdue_seconds < 3600 {
                format!("{} minutes", overdue_seconds / 60)
            } else if *overdue_seconds < 86400 {
                format!("{:.1} hours", *overdue_seconds as f64 / 3600.0)
            } else {
                format!("{:.1} days", *overdue_seconds as f64 / 86400.0)
            };
            
            let display_question = if question.len() > 20 {
                format!("{}...", &question[0..17])
            } else {
                question.clone()
            };
            
            analysis_table.add_row(row![
                id,
                display_question,
                repetitions,
                interval,
                overdue_display,
                format!("{:.1}", priority)
            ]);
        }
        
        // Check for ordering issues by comparing adjacent priorities
        for i in 1..cards_with_priority.len() {
            let (id, _, _, _, _, _, _, priority) = cards_with_priority[i];
            let (_, _, _, _, _, _, _, prev_priority) = cards_with_priority[i-1];
            if priority > prev_priority {  // Higher priority should come first
                ordering_issues.push((i, id, priority, prev_priority));
            }
        }
        
        output.push_str("Current Review Order:\n");
        output.push_str(&analysis_table.to_string());
        
        // Sort by our calculated priority for ideal order
        if !ordering_issues.is_empty() {
            // Sort by priority in descending order
            cards_with_priority.sort_by(|a, b| {
                let (_, _, _, _, _, _, _, priority_a) = a;
                let (_, _, _, _, _, _, _, priority_b) = b;
                priority_b.partial_cmp(priority_a).unwrap_or(std::cmp::Ordering::Equal)
            });
            
            output.push_str("\n\nPotential Review Order Improvements:\n");
            output.push_str(&format!("Found {} cards that might benefit from reordering.\n", ordering_issues.len()));
            
            // Display ideal order
            let mut ideal_table = Table::new();
            ideal_table.add_row(row![
                "ID", "Question", "Reps", "Interval", "Overdue By", "Priority"
            ]);
            for (id, question, repetitions, _, _, overdue_seconds, interval, priority) in &cards_with_priority {
                let overdue_display = if *overdue_seconds < 3600 {
                    format!("{} minutes", overdue_seconds / 60)
                } else if *overdue_seconds < 86400 {
                    format!("{:.1} hours", *overdue_seconds as f64 / 3600.0)
                } else {
                    format!("{:.1} days", *overdue_seconds as f64 / 86400.0)
                };
                
                let display_question = if question.len() > 20 {
                    format!("{}...", &question[0..17])
                } else {
                    question.clone()
                };
                
                ideal_table.add_row(row![
                    id,
                    display_question,
                    repetitions,
                    interval,
                    overdue_display,
                    format!("{:.1}", *priority)
                ]);
            }
            
            output.push_str("\nRecommended Review Order:\n");
            output.push_str(&ideal_table.to_string());
            output.push_str("\n\nAnalysis Summary:\n");
            output.push_str("1. Cards with fewer repetitions should generally be prioritized\n");
            output.push_str("2. Among cards with similar repetition counts, prioritize more overdue cards\n");
            output.push_str("3. Consider adjusting your review order based on the recommended sequence\n");
        } else {
            output.push_str("\n\nAnalysis Summary:\n");
            output.push_str("✅ The current review order appears optimal.\n");
            output.push_str("Cards are properly sequenced based on repetition count and overdue status.\n");
        }
        
        Ok(output)
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
}
