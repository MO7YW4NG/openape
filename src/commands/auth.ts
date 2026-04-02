import { getBaseDir } from "../lib/utils.ts";
import { Command } from "commander";
import { chromium, type Browser, type BrowserContext, type Page } from "playwright-core";
import type { AppConfig } from "../lib/types.ts";
import { createLogger } from "../lib/logger.ts";

import { findEdgePath } from "../lib/auth.ts";
import { saveSesskey, acquireWsToken, saveWsToken, loadWsToken } from "../lib/token.ts";
import { getSiteInfoApi } from "../lib/moodle.ts";
import path from "node:path";
import fs from "node:fs";

export function registerCommand(program: Command): void {
  program
    .command("login")
    .description("Login to iLearning manually and save session")
    .action(async (options) => {
      const log = createLogger(false);

      // Determine session storage path
      const baseDir = getBaseDir();
      const sessionDir = path.resolve(baseDir, ".auth");
      const sessionPath = path.resolve(sessionDir, "storage-state.json");

      // Ensure session directory exists
      if (!fs.existsSync(sessionDir)) {
        fs.mkdirSync(sessionDir, { recursive: true });
      }

      const edgePath = findEdgePath();
      const browser = await chromium.launch({
        executablePath: edgePath,
        headless: false,
        slowMo: 0,
      });

      let context: BrowserContext | undefined;
      let page: Page;

      if (fs.existsSync(sessionPath)) {
        log.info(`找到已有 session: ${sessionPath}`);
        log.info("正在驗證 session...");

        try {
          context = await browser.newContext({ storageState: sessionPath });
          page = await context.newPage();
          await page.goto("https://ilearning.cycu.edu.tw/my/", {
            waitUntil: "domcontentloaded",
            timeout: 15000,
          });

          const url = page.url();
          if (url.includes("login") || url.includes("microsoftonline")) {
            log.warn("Session 已過期，請重新登入。");
            await context.close();
            context = await browser.newContext();
            page = await context.newPage();
            await page.goto("https://ilearning.cycu.edu.tw/login/index.php", {
              waitUntil: "domcontentloaded",
            });
          } else {
            // Session is still valid, close browser and exit
            try {
              if (context) await context.close().catch(() => {});
            } catch {}
            try {
              await browser.close().catch(() => {});
            } catch {}
            // Wait a bit for browser to fully close
            await new Promise(resolve => setTimeout(resolve, 500));
            const result = {
              status: "success",
              message: "Session still valid",
              session_path: sessionPath,
              updated: false
            };
            console.log(JSON.stringify(result));
            return;
          }
        } catch {
          log.warn("無法恢復 session，請重新登入。");
          // context might not have been initialized if the error occurred during newContext
          if (context) {
            await context.close();
          }
          context = await browser.newContext();
          page = await context.newPage();
          await page.goto("https://ilearning.cycu.edu.tw/login/index.php", {
            waitUntil: "domcontentloaded",
          });
        }
      } else {
        log.info("首次登入，請在瀏覽器中完成登入流程。");
        context = await browser.newContext();
        page = await context.newPage();
        await page.goto("https://ilearning.cycu.edu.tw/login/index.php", {
          waitUntil: "domcontentloaded",
        });
      }

      log.info("\n請在瀏覽器中完成登入，登入成功後將自動儲存 session...\n");

      try {
        const startTime = Date.now();
        const timeout = 300000;
        let loggedIn = false;

        while (Date.now() - startTime < timeout) {
          await page.waitForTimeout(1000);
          const currentUrl = page.url();

          if (currentUrl.includes("ilearning.cycu.edu.tw") &&
              !currentUrl.includes("login") &&
              !currentUrl.includes("microsoftonline")) {
            await page.waitForTimeout(2000);
            const finalUrl = page.url();
            if (finalUrl.includes("ilearning.cycu.edu.tw") &&
                !finalUrl.includes("login") &&
                !finalUrl.includes("microsoftonline")) {
              loggedIn = true;
              break;
            }
          }
        }

        if (loggedIn) {
          await context.storageState({ path: sessionPath });

          // Extract and save sesskey for faster API calls
          try {
            // Navigate to a page with M.cfg first
            await page.goto("https://ilearning.cycu.edu.tw/my/", { waitUntil: "domcontentloaded" });
            // Use Function constructor to avoid dnt transforming globalThis
            const sesskey = await page.evaluate(() => (self as any).M?.cfg?.sesskey ?? null);
            if (sesskey) {
              saveSesskey(sessionPath, sesskey);
              log.debug(`Saved sesskey: ${sesskey}`);
            }
          } catch {
            // Ignore sesskey extraction errors
          }

          // Acquire WS token
          let wsToken: string | undefined;
          try {
            wsToken = await acquireWsToken(page, { moodleBaseUrl: "https://ilearning.cycu.edu.tw" } as AppConfig, log);
            saveWsToken(sessionPath, wsToken);
          } catch {
            // WS token is optional, ignore errors
          }

          const stats = fs.statSync(sessionPath);

          // Get user info via WS API
          let user: { userid: number; username: string; fullname: string } | undefined;
          try {
            if (wsToken) {
              const siteInfo = await getSiteInfoApi({
                wsToken,
                moodleBaseUrl: "https://ilearning.cycu.edu.tw",
              });
              user = {
                userid: siteInfo.userid,
                username: siteInfo.username,
                fullname: siteInfo.fullname,
              };
            }
          } catch {
            // Ignore
          }

          const result = {
            status: "success",
            message: "Login successful",
            session_path: sessionPath,
            session_size: stats.size,
            updated: true,
            ...(user ? { user } : {}),
          };

          console.log(JSON.stringify(result, null, 2));
        } else {
          throw new Error("TimeoutError");
        }

      } catch (err) {
        const errorResult = {
          status: "error",
          error: err instanceof Error ? err.message : String(err),
          session_path: sessionPath
        };

        console.log(JSON.stringify(errorResult));
      } finally {
        // Safely close browser with error handling
        try {
          if (context) await context.close().catch(() => {});
        } catch {}
        try {
          await browser.close().catch(() => {});
        } catch {}
        // Wait for browser process to fully terminate
        await new Promise(resolve => setTimeout(resolve, 500));
      }
    });

  program
    .command("status")
    .description("Check session status")
    .option("--session <path>", "Session file path", ".auth/storage-state.json")
    .action(async (options) => {
      const baseDir = getBaseDir();
      const sessionPath = path.resolve(baseDir, options.session);

      if (fs.existsSync(sessionPath)) {
        const stats = fs.statSync(sessionPath);

        // Try to read and validate the session
        try {
          const content = fs.readFileSync(sessionPath, "utf8");
          const state = JSON.parse(content);
          const cookies = state.cookies || [];
          const moodleSession = cookies.find((c: any) => c.name === "MoodleSession");

          const result: Record<string, any> = {
            status: "success",
            session_path: sessionPath,
            exists: true,
            modified: new Date(stats.mtime).toISOString(),
            size: stats.size,
            moodle_session: moodleSession ? {
              exists: true,
              expires: new Date(moodleSession.expires * 1000).toISOString()
            } : {
              exists: false
            }
          };

          // Try to get user info from WS API
          try {
            const wsToken = loadWsToken(sessionPath);
            if (wsToken) {
              const session = {
                wsToken,
                moodleBaseUrl: "https://ilearning.cycu.edu.tw"
              };
              const siteInfo = await getSiteInfoApi(session);
              result.user = {
                userid: siteInfo.userid,
                username: siteInfo.username,
                fullname: siteInfo.fullname
              };
            }
          } catch {
            // WS token might not be available or expired, skip user info
          }

          console.log(JSON.stringify(result, null, 2));
        } catch {
          const result = {
            status: "error",
            error: "Session file is corrupted",
            session_path: sessionPath
          };
          console.log(JSON.stringify(result, null, 2));
        }
      } else {
        const result = {
          status: "error",
          error: "Session not found",
          session_path: sessionPath,
          hint: "Run 'openape login' first"
        };
        console.log(JSON.stringify(result, null, 2));
      }
    });

  program
    .command("logout")
    .description("Remove saved session")
    .option("--session <path>", "Session file path", ".auth/storage-state.json")
    .action(async (options) => {
      const baseDir = getBaseDir();
      const sessionPath = path.resolve(baseDir, options.session);

      if (fs.existsSync(sessionPath)) {
        fs.unlinkSync(sessionPath);
        const result = {
          status: "success",
          message: "Session removed",
          session_path: sessionPath
        };
        console.log(JSON.stringify(result, null, 2));
      } else {
        const result = {
          status: "error",
          error: "Session not found",
          session_path: sessionPath
        };
        console.log(JSON.stringify(result, null, 2));
      }
    });
}
