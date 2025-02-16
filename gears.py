import json
import datetime
from prettytable import PrettyTable

# Filepath to the JSON file
filepath = "/Volumes/hynixp41/Users/701/words/flashcards.json"

# Read the JSON file
with open(filepath, 'r') as file:
    flashcards = json.load(file)

# Sort flashcards by repetitions first, then by next_review
flashcards_sorted = sorted(flashcards, key=lambda x: (x["repetitions"], x["next_review"]))

# Create a table
table = PrettyTable()
table.field_names = ["Order", "Question", "Next Review", "Repetitions"]

# Populate the table with data
for index, flashcard in enumerate(flashcards_sorted, start=1):
    question = flashcard["question"]
    next_review_timestamp = flashcard["next_review"]
    next_review_date = datetime.datetime.fromtimestamp(next_review_timestamp).strftime('%Y-%m-%d %H:%M:%S')
    repetitions = flashcard["repetitions"]
    table.add_row([index, question, next_review_date, repetitions])

# Print the table
print(table)