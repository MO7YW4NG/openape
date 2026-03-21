import type { Logger } from "./types.ts";

const NO_COLOR = !!process.env.NO_COLOR;

const c = (code: string, text: string) =>
  NO_COLOR ? text : `\x1b[${code}m${text}\x1b[0m`;

export function createLogger(verbose = false, silent = false): Logger {
  if (silent) {
    // Silent logger - no output at all
    return {
      info:    (_msg: string) => {},
      success: (_msg: string) => {},
      warn:    (_msg: string) => {},
      error:   (_msg: string) => {},
      debug:   (_msg: string) => {},
    };
  }
  return {
    info:    (msg: string) => console.error(c("36", "[INFO]") + `  ${msg}`),
    success: (msg: string) => console.error(c("32", "[OK]")   + `    ${msg}`),
    warn:    (msg: string) => console.error(c("33", "[WARN]") + `  ${msg}`),
    error:   (msg: string) => console.error(c("31", "[ERR]") + `   ${msg}`),
    debug:   (msg: string) => {
      if (verbose) console.error(c("90", "[DBG]") + `   ${msg}`);
    },
  };
}
