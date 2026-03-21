import { dirname } from "node:path";

/**
 * Returns the base directory for config/storage resolving.
 * Handles both Deno (raw/compiled) and Node.js (npx).
 */
export function getBaseDir(): string {
  // @ts-ignore - Deno global is available in Deno
  if (typeof Deno !== "undefined" && typeof Deno.execPath === "function") {
    // @ts-ignore
    const exeDir = dirname(Deno.execPath());
    return exeDir.includes("deno") ? process.cwd() : exeDir;
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
 * Example: "{mlang zh-tw}1142爵士樂賞析(遠距)-楊曊恩..." -> "爵士樂賞析"
 */
export function extractCourseName(fullname: string): string {
  if (!fullname) return "";
  // Remove {mlang ...} tags
  let cleaned = fullname.replace(/\{mlang[^}]*\}/g, "");
  // Match: 4+ digits + course name (until (, -, or [)
  const match = cleaned.match(/\d{4,}([^([-]+)/);
  return match ? match[1].trim() : fullname;
}
