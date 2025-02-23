use anyhow::{Result, Context};
use serde_json::Value;
use std::fs;
use rusqlite::{Connection, params};
use chrono;

pub struct Migrator {
    db: Connection,
}

impl Migrator {
    pub fn new() -> Result<Self> {
        let conn = Connection::open("flashcards.db")
            .context("Failed to open database")?;
        Ok(Migrator { db: conn })
    }

    pub fn migrate_from_json(&mut self, json_path: &str, username: &str, deck_name: &str) -> Result<()> {
        let json_content = fs::read_to_string(json_path)
            .context("Failed to read flashcards.json")?;

        let json: Value = serde_json::from_str(&json_content)
            .context("Failed to parse JSON")?;

        if let Value::Array(array) = &json {
            if let Some(card) = array.first() {
                println!("First record in source JSON: {:?}", card);
            }
        } else {
            println!("JSON is not an array: {:?}", json);
            return Err(anyhow::anyhow!("JSON is not an array"));
        }

        let tx = self.db.transaction()
            .context("Failed to start transaction")?;

        // Create user for existing cards
        tx.execute(
            "INSERT INTO users (username, created_at) VALUES (?1, ?2)",
            params![
                username,
                chrono::Utc::now().timestamp()
            ],
        )?;
        let user_id = tx.last_insert_rowid();

        // Create deck
        tx.execute(
            "INSERT INTO decks (user_id, name, created_at) VALUES (?1, ?2, ?3)",
            params![
                user_id,
                deck_name,
                chrono::Utc::now().timestamp()
            ],
        )?;
        let deck_id = tx.last_insert_rowid();

        // Migrate flashcards
        let mut migrated_count = 0;
        if let Value::Array(array) = json {
            for card in array {
                let result = tx.execute(
                    "INSERT INTO flashcards (
                        deck_id, question, answer, guidance,
                        interval, repetitions, ease_factor, next_review, created_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        deck_id,
                        card["question"].as_str().unwrap_or(""),
                        card["answer"].as_str().unwrap_or(""),
                        card["guidance"].as_str().unwrap_or(""),
                        card["interval"].as_u64().unwrap_or(0),
                        card["repetitions"].as_u64().unwrap_or(0),
                        card["ease_factor"].as_f64().unwrap_or(2.5),
                        card["next_review"].as_u64().unwrap_or(0),
                        chrono::Utc::now().timestamp(),
                    ],
                );
                if result.is_ok() {
                    migrated_count += 1;
                }
            }
        } else {
            println!("JSON is not an array");
        }

        // Commit transaction
        tx.commit().context("Failed to commit transaction")?;

        println!("Total flashcards migrated: {}", migrated_count);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    // use std::fs::File;
    // use std::io::Write;

    #[test]
    fn test_migration_from_json() -> Result<()> {
        // Create an in-memory database
        let conn = Connection::open_in_memory().context("Failed to open in-memory database")?;
        
        // Create the necessary tables
        conn.execute_batch("
            CREATE TABLE users (
                id INTEGER PRIMARY KEY,
                username TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE decks (
                id INTEGER PRIMARY KEY,
                user_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(user_id) REFERENCES users(id)
            );
            CREATE TABLE flashcards (
                id INTEGER PRIMARY KEY,
                deck_id INTEGER NOT NULL,
                question TEXT NOT NULL,
                answer TEXT NOT NULL,
                guidance TEXT,
                interval INTEGER NOT NULL,
                repetitions INTEGER NOT NULL,
                ease_factor REAL NOT NULL,
                next_review INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(deck_id) REFERENCES decks(id)
            );
        ").context("Failed to create schema")?;

        // Create a Migrator instance with the in-memory database
        let mut migrator = Migrator { db: conn };

        // Path to the flashcards.json file in the current directory
        let json_path = "flashcards.json";

        // Check if the flashcards.json file exists
        if !std::path::Path::new(json_path).exists() {
            panic!("flashcards.json file does not exist in the current directory");
        }

        // Perform the migration
        let username = "Alice";
        let deck_name = "Spanish Vocabulary";
        println!("Starting migration from JSON file: {}", json_path);
        migrator.migrate_from_json(json_path, username, deck_name)?;
        println!("Migration completed successfully");

        // Verify the migration results
        let mut stmt = migrator.db.prepare("SELECT question, answer, guidance FROM flashcards")?;
        let flashcards: Vec<(String, String, String)> = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?.collect::<Result<_, _>>()?;

        // Check if the flashcards have been correctly migrated
        assert!(!flashcards.is_empty(), "No flashcards were migrated");

        // Display the migrated flashcards
        // println!("Migrated flashcards:");
        // for (question, answer, guidance) in &flashcards {
        //     println!("Question: {}, Answer: {}, Guidance: {}", question, answer, guidance);
        // }

        // Display the first migrated flashcard
        if let Some((question, answer, guidance)) = flashcards.first() {
            println!("First migrated flashcard: Question: {}, Answer: {}, Guidance: {}", question, answer, guidance);
        }

        Ok(())
    }
}