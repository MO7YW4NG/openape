---
name: openape
description: How to use OpenApe CLI — automate CYCU iLearning (Moodle) tasks including course management, video progress tracking, quizzes, materials, grades, forums, announcements, and calendar
---

# OpenApe CLI

Use the `openape` command to access CYCU iLearning (Moodle) platform. OpenApe provides automation for course management, video progress completion, quiz tracking, material downloads, grade viewing, forum discussions, announcements, and calendar events.

## Setup

If `openape` is not installed:
```bash
npm install -g @mo7yw4ng/openape
```

If not authenticated, run:
```bash
openape login
```
A browser will open for Microsoft OAuth SSO. Complete MFA login manually.

Session is saved to `.auth/storage-state.json` and persists between runs. If session expires, run `openape login` again.

## Discovering Commands

**Every command supports `--help` for full option details:**
```bash
openape --help
openape courses --help
openape videos complete --help
```

Add `--output json` to any command for machine-readable output. Use `--output csv` for spreadsheet format, `--output table` for human-readable tables.

## Course Commands

### Listing courses

```bash
# List in-progress courses (default)
openape courses list

# List all courses including past and future
openape courses list --level all

# List only past courses
openape courses list --level past

# List only future courses
openape courses list --level future
```

Course levels: `in_progress` (default), `past`, `future`, `all`.

### Course information

```bash
# Get detailed course information
openape courses info <course-id>

# Get course progress percentage
openape courses progress <course-id>

# Get course syllabus from CMAP (18-week schedule)
openape courses syllabus <course-id>
```

## Video Commands

### Listing videos

```bash
# List all videos in a course
openape videos list <course-id>

# List only incomplete videos (browser mode only)
openape videos list <course-id> --incomplete-only
```

### Completing videos

```bash
# Complete all incomplete videos in a course
openape videos complete <course-id>

# Complete all incomplete videos across all courses
openape videos complete-all
```

### Downloading videos

```bash
# Download all videos from a course
openape videos download <course-id>

# Download only incomplete videos
openape videos download <course-id> --incomplete-only

# Specify output directory (default: ./downloads/videos)
openape videos download <course-id> --output-dir ./my-videos
```

## Quiz Commands

### Listing quizzes

```bash
# List quizzes in a specific course
openape quizzes list <course-id>

# List all quizzes across all courses
openape quizzes list-all

# List only in-progress course quizzes
openape quizzes list-all --level in_progress
```

### Opening quizzes

```bash
# Open a quiz URL in browser (manual mode)
openape quizzes open <quiz-url>
```

## Material Commands

### Listing materials

```bash
# List all materials/resources across all courses
openape materials list-all

# List materials from in-progress courses only
openape materials list-all --level in_progress
```

Materials include resources (PDFs, documents) and URLs (external links).

### Downloading materials

```bash
# Download all materials from a specific course
openape materials download <course-id>

# Download all materials from all in-progress courses
openape materials download-all

# Download from all courses (including past)
openape materials download-all --level all

# Specify output directory (default: ./downloads)
openape materials download-all --output-dir ./my-materials
```

## Grade Commands

### Viewing grades

```bash
# Show grade summary across all courses
openape grades summary

# Show detailed grades for a specific course
openape grades course <course-id>
```

## Forum Commands

### Listing forums

```bash
# List forums from in-progress courses
openape forums list

# List all forums across all courses
openape forums list-all

# List forums from a specific course level
openape forums list-all --level past
```

### Reading discussions

```bash
# List discussions in a forum (use cmid or instance ID)
openape forums discussions <forum-id>

# Show posts in a discussion
openape forums posts <discussion-id>
```

## Announcement Commands

### Listing announcements

```bash
# List all announcements across all courses
openape announcements list-all

# List only unread announcements
openape announcements list-all --unread-only
```

### Reading announcements

```bash
# Read a specific announcement (shows full content)
openape announcements read <announcement-id>
```

## Calendar Commands

### Listing events

```bash
# List all calendar events
openape calendar events

# List events after a specific date
openape calendar events --events-after 2026-03-01

# List events before a specific date
openape calendar events --events-before 2026-06-30

# List events in a specific course
openape calendar events --course-id <course-id>
```

### Exporting calendar

```bash
# Export calendar events to file
openape calendar export

# Specify output file (default: calendar_events.json)
openape calendar export --output my-calendar.json

# Export as ICS format for calendar apps
openape calendar export --format ics --output my-calendar.ics
```

## Output Formats

All commands support `--output` option:
- `json` (default) - Machine-readable JSON
- `csv` - Comma-separated values for spreadsheets
- `table` - Human-readable table format
- `silent` - Suppress output (useful for automation)

Global options:
- `--verbose` - Enable debug logging
- `--silent` - Suppress all log output (JSON only)
- `--headed` - Run browser in visible mode (for debugging)

## Example Workflows

**Check daily progress:** See what's due and what's incomplete.
```bash
openape courses list --level in_progress
openape videos list <course-id>
openape quizzes list-all
openape announcements list-all --unread-only
```

**Auto-complete videos:** Complete all incomplete videos across courses.
```bash
# First, dry-run to see what will be completed
openape videos complete-all --dry-run

# Then actually complete them
openape videos complete-all
```

**Download all materials:** Get all course materials for offline study.
```bash
openape materials download-all --output-dir ./semester-materials
```

**Check grades and progress:** See how you're doing in all courses.
```bash
openape grades summary
openape courses progress <course-id>
openape courses syllabus <course-id>
```

**Review discussions:** Catch up on forum activity.
```bash
openape forums list --level in_progress
openape forums list-all
openape forums discussions <forum-id>
openape forums posts <discussion-id>
```

**Plan your week:** Check upcoming events and deadlines.
```bash
openape calendar events --events-after 2026-03-21 --events-before 2026-03-28
openape calendar export --format ics --output week.ics
```

**Bulk operations:** Complete videos and download materials across all courses.
```bash
openape videos complete-all
openape materials download-all --level in_progress
```

## Tips

- Use `--level in_progress` (default) to focus on active courses
- Use `--output json` for scripting and automation
- Use `--output table` for human-readable output