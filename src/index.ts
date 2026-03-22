import { getBaseDir } from "./lib/utils.ts";

import { Command } from "commander";
import { loadConfig } from "./lib/config.ts";
import { launchAuthenticated } from "./lib/auth.ts";
import { extractSessionInfo } from "./lib/session.ts";
import { createLogger } from "./lib/logger.ts";
import type { AppConfig, Logger, SessionInfo, OutputFormat } from "./lib/types.ts";
import denoJson from "../deno.json" with { type: "json" };

// Import command handlers
import { registerCoursesCommand } from "./commands/courses.ts";
import { registerVideosCommand } from "./commands/videos.ts";
import { registerQuizzesCommand } from "./commands/quizzes.ts";
import { registerCommand } from "./commands/auth.ts";
import { registerMaterialsCommand } from "./commands/materials.ts";
import { registerGradesCommand } from "./commands/grades.ts";
import { registerForumsCommand } from "./commands/forums.ts";
import { registerAnnouncementsCommand } from "./commands/announcements.ts";
import { registerCalendarCommand } from "./commands/calendar.ts";
import { registerSkillsCommand } from "./commands/skills.ts";

const program = new Command();

program
  .name("openape")
  .description(denoJson.description)
  .version(denoJson.version);

// Global options
program
  .option("--config <path>", "Custom config file path")
  .option("--session <path>", "Session file path", ".auth/storage-state.json")
  .option("--output <format>", "Output format: json|csv|table|silent", "json")
  .option("--verbose", "Enable debug logging")
  .option("--silent", "Suppress all log output (JSON only)")
  .option("--headed", "Run browser in visible mode");

// Register subcommands
registerCommand(program);
registerCoursesCommand(program);
registerVideosCommand(program);
registerQuizzesCommand(program);
registerMaterialsCommand(program);
registerGradesCommand(program);
registerForumsCommand(program);
registerAnnouncementsCommand(program);
registerCalendarCommand(program);
registerSkillsCommand(program);

/**
 * Load configuration and authenticate, returning the context for commands.
 */
async function createCommandContext(
  options: {
    config?: string;
    session?: string;
    verbose?: boolean;
    silent?: boolean;
    headed?: boolean;
    interactive?: boolean;
  }
): Promise<{ config: AppConfig; log: Logger } | null> {
  const log = createLogger(options.verbose, options.silent);

  const baseDir = getBaseDir();
  const config = loadConfig(baseDir);

  // Apply CLI overrides
  if (options.headed) config.headless = false;
  if (options.session) config.authStatePath = options.session;

  return { config, log };
}

/**
 * Create a session context for commands that need authentication.
 */
export async function createSessionContext(
  options: {
    config?: string;
    session?: string;
    verbose?: boolean;
    silent?: boolean;
    headed?: boolean;
    interactive?: boolean;
  }
): Promise<{ config: AppConfig; log: Logger; page: import("playwright-core").Page; session: SessionInfo } | null> {
  const context = await createCommandContext(options);
  if (!context) return null;

  const { config, log } = context;

  log.info("啟動瀏覽器...");
  const { browser, context: browserContext, page, wsToken } = await launchAuthenticated(config, log);

  try {
    const session = await extractSessionInfo(page, config, log, wsToken);

    // Keep the browser context alive for the duration of the command
    // Note: Caller is responsible for closing the browser
    return { config, log, page, session };
  } catch (err) {
    await browserContext.close();
    await browser.close();
    throw err;
  }
}

/**
 * Helper to output formatted data.
 * For JSON output (agent mode), exits immediately after output.
 */
export function formatAndOutput<T extends Record<string, unknown>>(
  data: T | T[],
  format: OutputFormat,
  log: Logger
): void {
  if (format === "json") {
    console.log(JSON.stringify(data));
    // Exit immediately for AI agent - no need to wait for browser cleanup
    process.exit(0);
  } else if (format === "csv") {
    const arr = Array.isArray(data) ? data : [data];
    if (arr.length === 0) return;
    const fields = Object.keys(arr[0]);
    console.log(formatAsCsv(arr, fields));
  } else if (format === "table") {
    const arr = Array.isArray(data) ? data : [data];
    if (arr.length === 0) {
      console.log("No data");
      return;
    }
    console.log(formatAsTable(arr));
  }
  // "silent" produces no output
}

function formatAsCsv<T extends Record<string, unknown>>(
  data: T[],
  fields: string[]
): string {
  const headers = fields.join(",");
  const rows = data.map((item) => {
    return fields.map((field) => {
      const value = item[field];
      if (value === null || value === undefined) return "";
      if (typeof value === "string") {
        if (value.includes(",") || value.includes('"') || value.includes("\n")) {
          return `"${value.replace(/"/g, '""')}"`;
        }
        return value;
      }
      return String(value);
    }).join(",");
  });
  return [headers, ...rows].join("\n");
}

function formatAsTable<T extends Record<string, unknown>>(data: T[]): string {
  const allFields = Array.from(new Set(data.flatMap((item) => Object.keys(item))));
  const widths: Record<string, number> = {};
  allFields.forEach((field) => {
    widths[field] = Math.max(
      field.length,
      ...data.map((item) => String(item[field] ?? "").length)
    ) + 2;
  });

  const header = allFields.map((f) => f.padEnd(widths[f])).join(" | ");
  const separator = allFields.map((f) => "-".repeat(widths[f] - 1)).join("-+-");
  const rows = data.map((item) => {
    return allFields.map((f) => String(item[f] ?? "").padEnd(widths[f])).join(" | ");
  });

  return [header, separator, ...rows].join("\n");
}

// Export utilities for commands
export { createLogger, type AppConfig, type Logger, type SessionInfo, type OutputFormat };

// Run the program
if (import.meta.main) {
  // If no subcommand provided, show help
  const args = process.argv.slice(2);
  if (args.length === 0) {
    program.help();
  }

  program.parse();
}
