import type { Page } from "playwright-core";
import type { AppConfig, Logger } from "./types.ts";
import fs from "node:fs";
import path from "node:path";

/**
 * Get the WS token file path from the auth state path.
 * E.g., .auth/storage-state.json -> .auth/ws-token.json
 */
export function getWsTokenPath(authStatePath: string): string {
  const dir = path.dirname(authStatePath);
  return path.join(dir, "ws-token.json");
}

/**
 * Load WS token from file if it exists.
 */
export function loadWsToken(authStatePath: string): string | null {
  const tokenPath = getWsTokenPath(authStatePath);
  try {
    if (fs.existsSync(tokenPath)) {
      const content = fs.readFileSync(tokenPath, "utf8");
      const data = JSON.parse(content);
      // Check if token is not too old (Moodle tokens typically expire after some time)
      if (data.token && data.timestamp) {
        const age = Date.now() - data.timestamp;
        // Consider token valid if less than 24 hours old
        if (age < 24 * 60 * 60 * 1000) {
          return data.token;
        }
      }
    }
  } catch {
    // Ignore errors, token will be re-acquired
  }
  return null;
}

/**
 * Save WS token to file.
 */
export function saveWsToken(authStatePath: string, token: string): void {
  const tokenPath = getWsTokenPath(authStatePath);
  try {
    const data = {
      token,
      timestamp: Date.now(),
    };
    fs.writeFileSync(tokenPath, JSON.stringify(data, null, 2));
  } catch {
    // Ignore save errors
  }
}

/**
 * Extract and decode the Web Service Token from moodlemobile:// URL
 * Format: moodlemobile://token=BASE64_DATA
 * Decoded: token:::site_url:::other_params
 */
function extractTokenFromCustomScheme(url: string): string | null {
  try {
    const match = url.match(/token=([A-Za-z0-9+/=]+)/);
    if (!match) return null;

    // Base64 decode the token data
    const decoded = atob(match[1]);
    const parts = decoded.split(":::");

    // The second part (index 1) is the actual WS token
    return parts.length >= 2 ? parts[1] : null;
  } catch {
    return null;
  }
}

/**
 * Acquire Moodle Web Service Token via mobile app launch endpoint.
 *
 * Process:
 * 1. Visit admin/tool/mobile/launch.php with service=moodle_mobile_app
 * 2. Server redirects to moodlemobile://token=BASE64_DATA (which causes ERR_ABORTED)
 * 3. We catch the redirect from the response and extract the token
 *
 * @returns The Web Service Token for Moodle API calls
 * @throws Error if token acquisition fails
 */
export async function acquireWsToken(
  page: Page,
  config: AppConfig,
  log: Logger
): Promise<string> {
  log.info("Acquiring Moodle Web Service Token...");

  // Generate random UUID for passport parameter
  const passport = crypto.randomUUID();
  const launchUrl = `${config.moodleBaseUrl}/admin/tool/mobile/launch.php?service=moodle_mobile_app&passport=${passport}`;

  log.debug(`Token acquisition URL: ${launchUrl}`);

  // Set up response listener to catch the redirect
  let tokenFound = false;
  const tokenPromise = new Promise<string>((resolve, reject) => {
    const timeout = setTimeout(() => {
      if (!tokenFound) {
        page.off("response", responseHandler);
        reject(new Error("Token acquisition timed out - no redirect received"));
      }
    }, 15000);

    const responseHandler = async (response: any) => {
      try {
        const status = response.status();
        const headers = response.headers();

        // Check for redirect to custom scheme
        const location = headers["location"] || headers["Location"];
        if (location && location.startsWith("moodlemobile://")) {
          clearTimeout(timeout);
          tokenFound = true;
          page.off("response", responseHandler);

          const token = extractTokenFromCustomScheme(location);
          if (token) {
            resolve(token);
          } else {
            reject(new Error("Failed to extract token from custom scheme URL"));
          }
        }
      } catch (err) {
        // Ignore errors in response handler
      }
    };

    page.on("response", responseHandler);
  });

  try {
    // Navigate to the launch endpoint - expect it to fail with ERR_ABORTED
    // because the browser can't handle moodlemobile:// scheme
    await page.goto(launchUrl, {
      waitUntil: "domcontentloaded",
      timeout: 10000,
    }).catch(() => {
      // Expected: navigation will fail due to custom scheme redirect
      // The token should have been captured by our response handler
      log.debug("Navigation failed as expected (custom scheme redirect)");
    });

    // Wait for the intercepted token
    const token = await tokenPromise;

    log.success("Web Service Token acquired successfully");
    log.debug(`Token (first 10 chars): ${token.substring(0, 10)}...`);

    return token;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    log.warn(`Failed to acquire WS Token: ${message}`);
    throw error;
  }
}
