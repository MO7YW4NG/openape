import fs from "node:fs";
import path from "node:path";
import type { AppConfig } from "./types.ts";

/**
 * Load config from .env file (if it exists).
 */
export function loadConfig(baseDir?: string): AppConfig {
  const envPath = baseDir ? path.resolve(baseDir, ".env") : path.resolve(".env");
  if (fs.existsSync(envPath)) {
    const envContent = fs.readFileSync(envPath, "utf8");
    for (const line of envContent.split("\n")) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith("#")) continue;
      const eqIdx = trimmed.indexOf("=");
      if (eqIdx === -1) continue;
      const key = trimmed.slice(0, eqIdx).trim();
      const value = trimmed.slice(eqIdx + 1).trim();
      if (!process.env[key]) process.env[key] = value;
    }
  }

  return buildConfig();
}

function buildConfig(): AppConfig {
  const moodleBaseUrl = (
    process.env.MOODLE_BASE_URL ?? "https://ilearning.cycu.edu.tw"
  ).replace(/\/$/, "");

  return {
    courseUrl: "",
    moodleBaseUrl,
    headless: process.env.HEADLESS !== "false",
    slowMo: parseInt(process.env.SLOW_MO ?? "0", 10),
    authStatePath: process.env.AUTH_STATE_PATH ?? ".auth/storage-state.json",
    ollamaModel: process.env.MODEL,
    ollamaBaseUrl: (process.env.OLLAMA_BASE_URL ?? "http://localhost:11434").replace(/\/$/, ""),
  };
}
