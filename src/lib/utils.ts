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
