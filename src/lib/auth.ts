import fs from "node:fs";
import path from "node:path";
import { chromium, type Browser, type BrowserContext, type Page } from "playwright-core";
import type { AppConfig, Logger } from "./types.ts";
import { acquireWsToken, loadWsToken, saveWsToken } from "./token.ts";

/**
 * Find a Chromium-based browser executable on Windows.
 * Priority: Edge → Chrome → Brave
 */
export function findEdgePath(): string {
  const roots = [
    process.env.PROGRAMFILES,
    process.env["PROGRAMFILES(X86)"],
    process.env.LOCALAPPDATA,
  ].filter(Boolean) as string[];

  const browsers = [
    { name: "Edge",  suffix: "Microsoft\\Edge\\Application\\msedge.exe" },
    { name: "Chrome", suffix: "Google\\Chrome\\Application\\chrome.exe" },
    { name: "Brave",  suffix: "BraveSoftware\\Brave-Browser\\Application\\brave.exe" },
  ];

  for (const { suffix } of browsers) {
    for (const root of roots) {
      const candidate = path.join(root, suffix);
      if (fs.existsSync(candidate)) return candidate;
    }
  }

  throw new Error(
    "找不到可用的瀏覽器（Edge / Chrome / Brave）。請確認已安裝其中一種。"
  );
}

/**
 * Launch a browser and return an authenticated context.
 * Tries to restore a saved session first; falls back to fresh OAuth login.
 * Also acquires Moodle Web Service Token for API calls.
 */
export async function launchAuthenticated(
  config: AppConfig,
  log: Logger
): Promise<{ browser: Browser; context: BrowserContext; page: Page; wsToken?: string }> {
  const edgePath = findEdgePath();
  log.debug(`Using Edge: ${edgePath}`);

  // Wait a bit to ensure any previous browser process has fully terminated
  await new Promise(resolve => setTimeout(resolve, 1000));

  const browser = await chromium.launch({
    executablePath: edgePath,
    headless: config.headless,
    slowMo: config.slowMo,
  });

  // Try loading saved WS token first
  let wsToken: string | undefined = loadWsToken(config.authStatePath) ?? undefined;
  if (wsToken) {
    log.info("Loaded saved Web Service Token.");
  }

  // Try restoring a saved session
  const restored = await tryRestoreSession(browser, config, log);
  if (restored) {
    const page = restored.pages()[0] ?? (await restored.newPage());
    // If no saved WS token, try to acquire one
    if (!wsToken) {
      try {
        wsToken = await acquireWsToken(page, config, log);
        saveWsToken(config.authStatePath, wsToken);
      } catch {
        log.warn("Failed to acquire WS Token with restored session, continuing without it.");
      }
    }
    return { browser, context: restored, page, wsToken };
  }

  // Fresh login
  if (config.headless) {
    await browser.close().catch(() => {});
    throw new Error(
      "找不到有效的 Session 或是 Session 已過期。\n" +
      "請先執行 `openape login` 進行手動登入，或是加上 `--headed` 參數執行目前的指令以開啟登入畫面。"
    );
  }

  const context = await browser.newContext();
  const page = await context.newPage();
  await login(page, config, log);
  await saveSession(context, config.authStatePath, log);

  // Acquire WS Token after successful login
  if (!wsToken) {
    try {
      wsToken = await acquireWsToken(page, config, log);
      saveWsToken(config.authStatePath, wsToken);
    } catch {
      log.warn("Failed to acquire WS Token, continuing with sesskey-only auth.");
    }
  }

  return { browser, context, page, wsToken };
}

/**
 * Safely close browser and context with timeout.
 * Designed for AI agent usage - no human interaction needed.
 * If noWait is true, initiates cleanup but doesn't wait for completion.
 */
export async function closeBrowserSafely(
  browser: Browser,
  context?: BrowserContext,
  timeoutMs: number = 5000,
  noWait: boolean = false
): Promise<void> {
  const closePromises: Promise<void>[] = [];

  // Close context with error handling
  if (context) {
    closePromises.push(
      Promise.race([
        context.close().catch(() => {}),
        new Promise<void>(resolve => setTimeout(() => resolve(), timeoutMs))
      ])
    );
  }

  // Close browser with error handling
  closePromises.push(
    Promise.race([
      browser.close().catch(() => {}),
      new Promise<void>(resolve => setTimeout(() => resolve(), timeoutMs))
    ])
  );

  if (noWait) {
    // Fire and forget - don't wait for cleanup
    Promise.allSettled(closePromises);
    return;
  }

  await Promise.allSettled(closePromises);
}

/**
 * Attempt to restore a session from stored state.
 * Returns null if the stored state doesn't exist or the session is expired.
 */
async function tryRestoreSession(
  browser: Browser,
  config: AppConfig,
  log: Logger
): Promise<BrowserContext | null> {
  const statePath = path.resolve(config.authStatePath);
  if (!fs.existsSync(statePath)) {
    log.debug("No saved session found, will perform fresh login.");
    return null;
  }

  log.info("Restoring saved session...");
  const context = await browser.newContext({ storageState: statePath });
  const page = await context.newPage();

  try {
    await page.goto(`${config.moodleBaseUrl}/my/`, {
      waitUntil: "domcontentloaded",
      timeout: 15000,
    });

    // If we got redirected to a login page, the session is expired
    const url = page.url();
    if (url.includes("login") || url.includes("microsoftonline")) {
      log.warn("Saved session expired, will re-authenticate.");
      await context.close();
      return null;
    }

    log.success("Session restored successfully.");
    return context;
  } catch {
    log.warn("Failed to restore session, will re-authenticate.");
    await context.close();
    return null;
  }
}

/**
 * Save the current session state to disk for future reuse.
 */
async function saveSession(
  context: BrowserContext,
  statePath: string,
  log: Logger
): Promise<void> {
  try {
    await context.storageState({ path: statePath });
    log.debug("Session saved for future reuse.");
  } catch (err) {
    log.warn(`Failed to save session: ${err}`);
  }
}

/**
 * Perform Microsoft OAuth login flow.
 */
async function login(
  page: Page,
  config: AppConfig,
  log: Logger
): Promise<void> {
  log.info("Starting Microsoft OAuth login...");

  await page.goto(`${config.moodleBaseUrl}/auth/oauth2/login.php`, {
    waitUntil: "domcontentloaded",
    timeout: 30000,
  });

  // Wait for Microsoft login page or redirect back to Moodle
  try {
    await page.waitForURL(
      (url) =>
        url.toString().includes("microsoftonline") ||
        url.toString().includes("login.microsoftonline") ||
        (url.toString().includes("ilearning.cycu.edu.tw") &&
          !url.toString().includes("auth/oauth2/login")),
      { timeout: 10000 }
    );

    const url = page.url().toString();
    if (url.includes("microsoftonline") || url.includes("login.microsoftonline")) {
      log.info("Microsoft login page detected. Please complete login in the browser.");
      log.info("Waiting for redirect back to Moodle...");

      await page.waitForURL(
        (u) =>
          u.toString().includes("ilearning.cycu.edu.tw") &&
          !u.toString().includes("microsoftonline") &&
          !u.toString().includes("login.microsoftonline"),
        { timeout: 300000 }
      );
    }
  } catch {
    // Already logged in or redirected
  }

  // Verify we're logged in
  const finalUrl = page.url().toString();
  if (
    finalUrl.includes("login") ||
    finalUrl.includes("microsoftonline") ||
    finalUrl === config.moodleBaseUrl + "/auth/oauth2/login.php"
  ) {
    throw new Error(`登入後未重新導向回 Moodle。目前 URL: ${finalUrl}`);
  }

  log.success("Login completed successfully.");
}
