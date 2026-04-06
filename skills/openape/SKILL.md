---
name: openape
description: "CYCU iLearning (Moodle): Manage courses, videos, quizzes, materials, grades, forums, announcements, and calendar."
metadata:
  openclaw:
    category: "education"
    requires:
      bins:
        - openape
    cliHelp: "openape --help"
---

# openape

> **PREREQUISITE:** Install and login first:
>
> ```bash
> npm install -g @mo7yw4ng/openape
> openape login
> ```

```bash
openape <command> [subcommand] [args] [flags]
```

## Commands

### courses — Course operations

  - `list` — List enrolled courses. Flags: `--level in_progress|past|future|all`
  - `info <course-id>` — Show detailed course information
  - `progress <course-id>` — Show course completion progress
  - `syllabus <course-id>` — Show course syllabus (from CMAP)

### videos — Video progress operations

  - `list <course-id>` — List videos in a course. Flags: `--incomplete-only`
  - `complete <course-id>` — Complete all videos in a course
  - `complete-all` — Complete all incomplete videos across all courses. Flags: `--dry-run`
  - `download <course-id>` — Download videos from a course. Flags: `--output-dir <path>`

### quizzes — Quiz operations

  - `list <course-id>` — List incomplete quizzes in a course. Flags: `--all`
  - `list-all` — List all incomplete quizzes across courses. Flags: `--level in_progress|all`
  - `start <quiz-id>` — Start a new quiz attempt
  - `info <attempt-id>` — Get quiz attempt data and questions. Flags: `--page <number>`
  - `save <attempt-id> '<answers-json>'` — Save answers for a quiz attempt. Flags: `--submit`. JSON format: `[{"slot":1,"answer":"0"}]`. Multichoice: number, multichoices: `"0,2"`, shortanswer: text

> **NEVER SUBMIT WITHOUT USER'S PERMISSION**, you have to make sure answer is saved before submitting.

### materials — Material/resource operations

  - `list-all` — List all materials across courses. Flags: `--level in_progress|all`
  - `download <course-id>` — Download all materials from a course
  - `download-all` — Download all materials from all courses. Flags: `--output-dir <path>`
  - `complete <course-id>` — Mark all incomplete resources (non-video) as complete
  - `complete-all` — Mark all incomplete resources across all in-progress courses

### assignments — Assignment operations

  - `list <course-id>` — List assignments in a course
  - `list-all` — List all assignments across courses. Flags: `--level in_progress|all`
  - `status <assignment-id>` — Check assignment submission status
  - `submit <assignment-id>` — Submit an assignment. Flags: `--text <content>`, `--file-id <id>`

### grades — Grade operations

  - `summary` — Show grade summary across all courses
  - `course <course-id>` — Show detailed grades for a course

### forums — Forum operations

  - `list` — List forums from in-progress courses
  - `list-all` — List all forums across all courses. Flags: `--level in_progress|all`
  - `discussions <forum-id>` — List discussions in a forum
  - `posts <discussion-id>` — Show posts in a discussion
  - `post <forum-id> <subject> <message>` — Post a new discussion. Flags: `--subscribe`, `--pin`
  - `reply <post-id> <subject> <message>` — Reply to a discussion post. Flags: `--attachment-id <id>`, `--inline-attachment-id <id>`
  - `delete <post-id>` — Delete a forum post or discussion

### announcements — Announcement operations

  - `list-all` — List all announcements across courses. Flags: `--unread-only`
  - `read <announcement-id>` — Read a specific announcement (full content)

### calendar — Calendar operations

  - `events` — List calendar events. Flags: `--course <id>`, `--upcoming`, `--days <n>`
  - `export` — Export calendar events to file. Flags: `--output <path>`, `--days <n>`

### upload — File upload

  - `file <file-path>` — Upload a file to Moodle draft area

### skills — Skill management

  - `install [platform]` — Install OpenApe skill to an agent platform (claude, codex, opencode)
  - `show` — Print the raw SKILL.md content

## Output Formats

Most data commands support `--output`: `json` (default), `csv`, `table`, `silent`

Global flags: `--verbose`, `--headed`, `--session <path>`

## Discovering Commands

```bash
# Browse all commands
openape --help

# Inspect a command's subcommands and options
openape <command> --help
```
