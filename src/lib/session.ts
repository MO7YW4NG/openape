import type { Page } from "playwright-core";
import type { AppConfig, Logger, SessionInfo } from "./types.ts";

/**
 * Extract Moodle sesskey from the current page.
 * The sesskey is required for all AJAX calls.
 */
export async function extractSessionInfo(
  page: Page,
  config: AppConfig,
  log: Logger,
  wsToken?: string
): Promise<SessionInfo> {
  // Ensure we're on a Moodle page
  const url = page.url();
  if (!url.includes(config.moodleBaseUrl.replace("https://", ""))) {
    await page.goto(`${config.moodleBaseUrl}/my/`, {
      waitUntil: "domcontentloaded",
    });
  }

  // Try extracting sesskey from M.cfg (Moodle's JS config object)
  // Use string to avoid dnt transforming globalThis/window to dntShim
  let sesskey: string | null = await page.evaluate("() => self.M?.cfg?.sesskey ?? null");

  // Fallback: extract from a hidden input
  if (!sesskey) {
    sesskey = await page.evaluate(() => {
      const el = document.querySelector<HTMLInputElement>('input[name="sesskey"]');
      return el?.value ?? null;
    });
  }

  // Fallback: regex on page source
  if (!sesskey) {
    const content = await page.content();
    const match = content.match(/"sesskey"\s*:\s*"([a-zA-Z0-9]+)"/);
    sesskey = match?.[1] ?? null;
  }

  if (!sesskey) {
    throw new Error("Failed to extract sesskey from Moodle page.");
  }

  log.debug(`Extracted sesskey: ${sesskey}`);

  const sessionInfo: SessionInfo = {
    sesskey,
    moodleBaseUrl: config.moodleBaseUrl,
  };

  if (wsToken) {
    sessionInfo.wsToken = wsToken;
    log.debug("Web Service Token included in session");
  }

  return sessionInfo;
}
