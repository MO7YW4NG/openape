#!/usr/bin/env node

import { spawnSync } from "child_process";
import { join } from "path";

function getExePath(): string {
  const arch = process.arch;
  let os = process.platform as string;
  let ext = "";

  if (os === "win32" || os === "cygwin") {
    os = "win32";
    ext = ".exe";
  }

  const pkg = `@mo7yw4ng/openape-${os}-${arch}`;
  try {
    // Resolve the package directory instead of a specific file
    const pkgJson = require.resolve(`${pkg}/package.json`);
    return join(pkgJson, "..", "bin", `openape${ext}`);
  } catch (e) {
    throw new Error(
      `Unsupported platform: ${process.platform} ${arch}\n` +
      `Try installing the platform package manually: npm install ${pkg}`
    );
  }
}

const result = spawnSync(getExePath(), process.argv.slice(2), {
  stdio: "inherit",
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 0);
