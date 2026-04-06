import { dirname, resolve } from "node:path";
import type { OutputFormat } from "./types.ts";

/**
 * Returns the base directory for config/storage resolving.
 * Handles both Deno (raw/compiled) and Node.js (npx).
 */
export function getBaseDir(): string {
  // @ts-ignore - Deno global is available in Deno
  if (typeof Deno !== "undefined" && typeof Deno.execPath === "function") {
    try {
      // @ts-ignore
      const exeDir = dirname(Deno.execPath());
      return exeDir.includes("deno") ? process.cwd() : exeDir;
    } catch {
      // Deno shim (dnt) or Deno not installed
      return process.cwd();
    }
  }
  // Node.js or dnt runtime
  return process.cwd();
}

/**
 * Strip HTML tags from a string.
 * Preserves text content while removing all HTML markup.
 */
export function stripHtmlTags(html: string): string {
  if (!html) return "";
  // Remove HTML tags
  return html.replace(/<[^>]*>/g, "")
    // Replace HTML entities with their characters
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&#(\d+);/g, (_, dec) => String.fromCharCode(parseInt(dec, 10)))
    // Clean up excessive whitespace
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * Extract clean course name from Moodle fullname.
 * Removes mlang tags, course codes, and instructor info.
 * Example: "{mlang zh-tw}1142Jazz Analysis(Distance)-Instructor..." -> "Jazz Analysis"
 */
export function extractCourseName(fullname: string): string {
  if (!fullname) return "";
  // Remove {mlang ...} tags
  let cleaned = fullname.replace(/\{mlang[^}]*\}/g, "");
  // Match: 4+ digits + course name (until (, -, or [)
  const match = cleaned.match(/\d{4,}([^([-]+)/);
  return match ? match[1].trim() : fullname;
}

/**
 * Get output format from command options (global or local).
 * Defaults to "json" if not specified.
 */
export function getOutputFormat(command: { optsWithGlobals(): { output?: OutputFormat } }): OutputFormat {
  const opts = command.optsWithGlobals();
  return (opts.output as OutputFormat) || "json";
}

/**
 * Sanitize filename by removing invalid characters and limiting length.
 * Replaces invalid characters with underscores and limits to maxLength.
 */
export function sanitizeFilename(name: string, maxLength: number = 200): string {
  return name
    .replace(/[<>:"/\\|?*]/g, "_")
    .replace(/\s+/g, "_")
    .substring(0, maxLength);
}

/**
 * Get the session storage file path.
 */
export function getSessionPath(): string {
  const baseDir = getBaseDir();
  return resolve(baseDir, ".auth", "storage-state.json");
}

/**
 * Format file size to KB with specified decimal places.
 */
export function formatFileSize(bytes: number, decimals: number = 2): string {
  return (bytes / 1024).toFixed(decimals);
}

/**
 * Format Moodle timestamp to localized string.
 */
export function formatMoodleDate(timestamp?: number): string {
  if (!timestamp || timestamp === 0) return "無期限";
  return new Date(timestamp * 1000).toLocaleString("zh-TW");
}

/**
 * Unified timestamp conversion (default: local time string)
 */
export function formatTimestamp(timestamp: number | undefined | null, format: "iso" | "local" | "relative" = "local"): string {
  if (!timestamp || timestamp === 0) return "無期限";

  const date = new Date(timestamp * 1000);

  if (format === "iso") return date.toISOString();
  if (format === "relative") return formatRelativeTime(timestamp);
  return date.toLocaleString("zh-TW");
}

/**
 * Relative time format (e.g., "2 hours ago")
 */
export function formatRelativeTime(timestamp: number): string {
  const seconds = Math.floor(Date.now() / 1000) - timestamp;
  if (seconds < 60) return `${seconds} seconds ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)} minutes ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} hours ago`;
  return `${Math.floor(seconds / 86400)} days ago`;
}
