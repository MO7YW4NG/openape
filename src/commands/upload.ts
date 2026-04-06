import { getOutputFormat, formatFileSize } from "../lib/utils.ts";
import { Command } from "commander";
import type { OutputFormat } from "../lib/types.ts";
import { uploadFileApi } from "../lib/moodle.ts";
import { createApiContext } from "../lib/auth.ts";
import { formatAndOutput } from "../index.ts";
import path from "node:path";
import fs from "node:fs/promises";

export function registerUploadCommand(program: Command): void {
  const uploadCmd = program.command("upload");
  uploadCmd.description("Upload files to Moodle draft area");

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

      const resolvedPath = path.resolve(filePath);

      let stats;
      try {
        stats = await fs.stat(resolvedPath);
      } catch {
        apiContext.log.error(`檔案不存在: ${filePath}`);
        process.exitCode = 1;
        return;
      }

      const fileSizeKB = formatFileSize(stats.size);
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
        filesize_kb: formatFileSize(stats.size),
        message: "Use this draft ID for assignment submission or forum posts",
      };

      formatAndOutput(uploadResult as unknown as Record<string, unknown>, output, apiContext.log);
    });
}
