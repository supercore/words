use crate::flashcard::Flashcard;
use anyhow::{Context, Result};
use prettytable::{row, Table};
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::{SystemTime, UNIX_EPOCH};

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
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs() as i64;

        self.conn
            .execute(
                "INSERT INTO users (username, created_at) VALUES (?1, ?2)",
                params![username, timestamp],
            )
            .context("Failed to create user")?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn create_deck(&self, user_id: i64, name: &str) -> Result<i64> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs() as i64;

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
                        "INSERT INTO decks (user_id, name, created_at) VALUES (?1, ?2, ?3)",
                        params![user_id, name, timestamp],
                    )
                    .context("Failed to create deck")?;

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

                self.conn
                    .execute(
                        "INSERT INTO decks (user_id, name, created_at) VALUES (?1, ?2, ?3)",
                        params![user_id, deck_name, timestamp],
                    )
                    .context("Failed to create deck")?;

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
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs() as i64;

        // Get current flashcard
        let mut stmt = self.conn.prepare(
            "SELECT question, answer, guidance, interval, repetitions, ease_factor, next_review 
             FROM flashcards WHERE id = ?",
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
            params![
                card.interval,
                card.repetitions,
                card.ease_factor,
                card.next_review,
                card_id
            ],
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
             ORDER BY repetitions ASC, next_review ASC",
        )?;

        let cards = stmt.query_map(params![deck_id, now], |row| {
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
                },
            ))
        })?;

        Ok(cards.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn log_operation(
        &self,
        user_id: i64,
        operation: &str,
        entity: &str,
        entity_id: i64,
        details: Option<&str>,
    ) -> Result<()> {
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
             LIMIT 10;",
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
             LIMIT 15;",
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

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs() as i64;

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

        // Use batch operation for actual insertion
        match self.batch_add_flashcards(deck_id, user_id, card_refs) {
            Ok(_) => {
                println!(
                    "Import completed. Total imported: {}, Total skipped: {}",
                    processed_cards.len(),
                    invalid_lines.len()
                );

                // Log the operation
                self.log_operation(
                    user_id,
                    "IMPORT",
                    "FLASHCARDS",
                    deck_id,
                    Some(&format!(
                        "Imported {} cards from CSV",
                        processed_cards.len()
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

    // Instead of individual inserts during CSV/JSON imports:
    pub fn batch_add_flashcards(
        &self,
        deck_id: i64,
        user_id: i64,
        cards: Vec<&Flashcard>,
    ) -> Result<()> {
        // Verify deck belongs to user
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM decks WHERE id = ? AND user_id = ?")?;
        let result = stmt.exists(params![deck_id, user_id])?;

        if !result {
            return Err(anyhow::anyhow!(
                "Deck doesn't belong to user or doesn't exist"
            ));
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs() as i64;

        self.conn.execute_batch("BEGIN TRANSACTION;")?;

        let mut stmt = self.conn.prepare(
            "INSERT INTO flashcards (deck_id, question, answer, guidance, interval, repetitions, ease_factor, next_review, created_at)
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

        self.conn.execute_batch("COMMIT;")?;
        Ok(())
    }
}
