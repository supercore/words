import json
import datetime
from prettytable import PrettyTable

# Filepath to the JSON file
filepath = "/Volumes/hynixp41/Users/701/words/flashcards.json"

# Read the JSON file
with open(filepath, 'r') as file:
    flashcards = json.load(file)
flashcards_sorted = sorted(flashcards, key=lambda x: x["next_review"])

# Create a table
table = PrettyTable()
table.field_names = ["Question", "Next Review"]

# Populate the table with data
for flashcard in flashcards_sorted:
    question = flashcard["question"]
    next_review_timestamp = flashcard["next_review"]
    next_review_date = datetime.datetime.fromtimestamp(next_review_timestamp).strftime('%Y-%m-%d %H:%M:%S')
    table.add_row([question, next_review_date])

# Print the table
print(table)