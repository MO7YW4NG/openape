import { getBaseDir } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, OutputFormat } from "../lib/types.ts";
import { uploadFileApi } from "../lib/moodle.ts";
import { createLogger } from "../lib/logger.ts";
import { loadWsToken } from "../lib/token.ts";
import { formatAndOutput } from "../index.ts";
import path from "node:path";
import fs from "node:fs";

export function registerUploadCommand(program: Command): void {
  const uploadCmd = program.command("upload");
  uploadCmd.description("Upload files to Moodle draft area");

  function getOutputFormat(command: any): OutputFormat {
    const opts = command.optsWithGlobals();
    return (opts.output as OutputFormat) || "json";
  }

  // Pure API context - no browser required (fast!)
  async function createApiContext(options: { verbose?: boolean; headed?: boolean }, command?: any): Promise<{
    log: Logger;
    session: { wsToken: string; moodleBaseUrl: string };
  } | null> {
    const opts = command?.optsWithGlobals ? command.optsWithGlobals() : options;
    const outputFormat = getOutputFormat(command || { optsWithGlobals: () => ({ output: "json" }) });
    const silent = outputFormat === "json" && !opts.verbose;
    const log = createLogger(opts.verbose, silent);

    const baseDir = getBaseDir();
    const sessionPath = path.resolve(baseDir, ".auth", "storage-state.json");

    // Check if session exists
    if (!fs.existsSync(sessionPath)) {
      log.error("未找到登入 session。請先執行 'openape login' 進行登入。");
      log.info(`Session 預期位置: ${sessionPath}`);
      return null;
    }

    // Try to load WS token
    const wsToken = loadWsToken(sessionPath);
    if (!wsToken) {
      log.error("未找到 WS token。請先執行 'openape login' 進行登入。");
      return null;
    }

    return {
      log,
      session: {
        wsToken,
        moodleBaseUrl: "https://ilearning.cycu.edu.tw",
      },
    };
  }

  uploadCmd
    .command("file")
    .description("Upload a file to Moodle draft area")
    .argument("<file-path>", "Path to the file to upload")
    .option("--filename <name>", "Custom filename (default: original filename)")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (filePath, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      // Resolve file path
      const resolvedPath = path.resolve(filePath);

      // Check if file exists
      if (!fs.existsSync(resolvedPath)) {
        apiContext.log.error(`檔案不存在: ${filePath}`);
        process.exitCode = 1;
        return;
      }

      // Get file size
      const stats = fs.statSync(resolvedPath);
      const fileSizeKB = (stats.size / 1024).toFixed(2);
      apiContext.log.info(`上傳檔案: ${path.basename(resolvedPath)} (${fileSizeKB} KB)`);

      // Upload file
      const result = await uploadFileApi(apiContext.session, resolvedPath, {
        filename: options.filename,
      });

      if (!result.success) {
        apiContext.log.error(`上傳失敗: ${result.error}`);
        process.exitCode = 1;
        return;
      }

      apiContext.log.info(`✓ 上傳成功！Draft ID: ${result.draftId}`);

      const uploadResult = {
        success: true,
        draft_id: result.draftId,
        filename: path.basename(resolvedPath),
        filesize: stats.size,
        filesize_kb: fileSizeKB,
        message: "Use this draft ID for assignment submission or forum posts",
      };

      formatAndOutput(uploadResult as unknown as Record<string, unknown>, output, apiContext.log);
    });
}
