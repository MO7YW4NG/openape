import { Command } from "commander";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const SKILL_NAME = "openape";
const GITHUB_RAW_URL = `https://raw.githubusercontent.com/mo7yw4ng/openape/refs/heads/main/skills/${SKILL_NAME}/SKILL.md`;

/**
 * Known agent platforms and their skills directories
 */
const PLATFORMS: Record<string, { name: string; path: string }> = {
  claude: { name: "Claude Code", path: path.join(os.homedir(), ".claude", "skills") },
  codex: { name: "Codex CLI", path: path.join(os.homedir(), ".codex", "skills") },
  opencode: { name: "OpenCode", path: path.join(os.homedir(), ".opencode", "skills") },
};

/**
 * Try to read SKILL.md from local project first (dev mode / bundled build),
 * fallback to fetching from GitHub (when installed globally via npm).
 */
async function readSkillContent(): Promise<string> {
  // Try local path first (relative to this file's location)
  try {
    const base = path.dirname(new URL(import.meta.url).pathname);
    const normalized = process.platform === "win32" ? base.replace(/^\//, "") : base;

    // When running from source: src/commands/ → ../../skills/openape/SKILL.md
    // When bundled by dnt into build/: esm/commands/ or script/ → ../../skills/openape/SKILL.md
    const localPath = path.resolve(normalized, "..", "..", "skills", SKILL_NAME, "SKILL.md");
    return await fs.promises.readFile(localPath, "utf-8");
  } catch {
    // import.meta.url may be unavailable in some environments, or file doesn't exist
  }

  // Fallback: fetch from GitHub
  const res = await fetch(GITHUB_RAW_URL, { headers: { "User-Agent": "openape-cli" } });
  if (!res.ok) {
    throw new Error(`Failed to fetch skill from GitHub: ${res.status} ${res.statusText}`);
  }
  return res.text();
}

export function registerSkillsCommand(program: Command): void {
  const skills = program
    .command("skills")
    .description("Manage OpenApe skills for AI agents");

  skills
    .command("install [platform]")
    .description("Install the OpenApe skill to an agent platform (claude, codex, opencode)")
    .option("--all", "Detect installed agents and install to all")
    .action(async (platform?: string, opts?: { all?: boolean }) => {
      try {
        let targets: { key: string; name: string; path: string }[] = [];

        if (opts?.all) {
          for (const [key, info] of Object.entries(PLATFORMS)) {
            const parentDir = path.dirname(info.path);
            if (fs.existsSync(parentDir)) {
              targets.push({ key, ...info });
            }
          }
          if (targets.length === 0) {
            console.log("No supported agents detected. Supported platforms: " + Object.keys(PLATFORMS).join(", "));
            return;
          }
        } else if (platform) {
          const info = PLATFORMS[platform.toLowerCase()];
          if (!info) {
            console.error(`Unknown platform: ${platform}`);
            console.error(`Supported platforms: ${Object.keys(PLATFORMS).join(", ")}`);
            process.exitCode = 1;
            return;
          }
          targets = [{ key: platform.toLowerCase(), ...info }];
        } else {
          console.error("Specify a platform or use --all.");
          console.error(`Example: openape skills install claude`);
          process.exitCode = 1;
          return;
        }

        console.log(`Fetching ${SKILL_NAME} skill...`);
        const content = await readSkillContent();

        for (const target of targets) {
          console.log(`Installing to ${target.name} (${target.path})...`);
          const destDir = path.join(target.path, SKILL_NAME);

          await fs.promises.mkdir(destDir, { recursive: true });
          await fs.promises.writeFile(path.join(destDir, "SKILL.md"), content, "utf-8");
          console.log(`  \x1b[32m✔\x1b[0m ${SKILL_NAME} installed!`);
        }

        console.log("\nDone!");
      } catch (err) {
        console.error(`\x1b[31mFailed to install skill: ${(err as Error).message}\x1b[0m`);
        process.exitCode = 1;
      }
    });

  skills
    .command("show")
    .description("Print the raw SKILL.md content")
    .action(async () => {
      try {
        const content = await readSkillContent();
        process.stdout.write(content);
      } catch (err) {
        console.error(`\x1b[31mFailed: ${(err as Error).message}\x1b[0m`);
        process.exitCode = 1;
      }
    });
}
