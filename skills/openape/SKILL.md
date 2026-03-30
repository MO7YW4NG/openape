---
name: openape
description: How to use OpenApe CLI — automate CYCU iLearning (Moodle) tasks including course management, video progress tracking, quizzes, materials, grades, forums, announcements, and calendar
---

# OpenApe CLI

Use `openape` command to access CYCU iLearning (Moodle) platform.

## Setup

```bash
# Install globally via npm
npm install -g @mo7yw4ng/openape

# Login (opens browser for Microsoft OAuth SSO)
openape login

# Check session status and user info
openape status
```

## Commands

### Courses
```bash
openape courses list [--level in_progress|past|future|all]
openape courses info <course-id>
openape courses progress <course-id>
openape courses syllabus <course-id>
```

### Videos
```bash
openape videos list <course-id> [--incomplete-only]
openape videos complete <course-id>
openape videos complete-all [--dry-run]
openape videos download <course-id> [--output-dir ./downloads/videos]
```

### Quizzes
```bash
openape quizzes list <course-id>
openape quizzes list-all [--level in_progress|all]
openape quizzes start <quiz-id>
openape quizzes info <attempt-id> [--page <number>]
openape quizzes save <attempt-id> '<answers-json>'
# answers-json example: '[{"slot":1,"answer":"0"}]'
# multichoice: number, multichoices: "0,2", shortanswer: "text"
```

### Materials
```bash
openape materials list-all [--level in_progress|all]
openape materials download <course-id>
openape materials download-all [--output-dir ./downloads]
openape materials complete <course-id>
openape materials complete-all
```

### Assignments
```bash
openape assignments list <course-id>
openape assignments list-all [--level in_progress|all]
openape assignments status <assignment-id>
openape assignments submit <assignment-id> [--text <content>] [--file-id <id>]
```

### Grades
```bash
openape grades summary
openape grades course <course-id>
```

### Forums
```bash
openape forums list [--level in_progress|all]
openape forums list-all
openape forums discussions <forum-id>
openape forums posts <discussion-id>
openape forums post <forum-id> <subject> <message> [--subscribe] [--pin]
openape forums reply <discussion-id> <subject> <message> [--parent-id <id>]
openape forums delete <post-id>
```

### Announcements
```bash
openape announcements list-all [--unread-only]
openape announcements read <announcement-id>
```

### Calendar
```bash
openape calendar events [--course-id <id>] [--events-after <date>] [--events-before <date>]
openape calendar export [--format json|ics] [--output <file>]
```

### Upload
```bash
openape upload file <file-path>
```

### Skills
```bash
openape skills install [claude|codex|opencode]
openape skills show
```

## Output Formats

All commands support `--output` option:
- `json` (default) - Machine-readable JSON
- `csv` - Spreadsheet format
- `table` - Human-readable tables
- `silent` - Suppress output

Global options: `--verbose`, `--headed`, `--session <path>`

## Quick Examples

```bash
# Check daily progress
openape courses list
openape announcements list-all --unread-only

# Auto-complete videos
openape videos complete-all --dry-run  # preview
openape videos complete-all            # execute

# Auto-complete materials
openape materials complete-all

# Download materials
openape materials download-all --output-dir ./semester

# Check grades
openape grades summary

# Post to forum
openape forums post 26617 "簽到" "11144238 王敏權"

# Take a quiz
openape quizzes start 12345
openape quizzes info 234210
openape quizzes save 234210 '[{"slot":1,"answer":"0"}]'
```
