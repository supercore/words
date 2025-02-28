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