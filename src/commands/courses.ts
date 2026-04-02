import { getBaseDir, stripHtmlTags, formatTimestamp } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, OutputFormat } from "../lib/types.ts";
import { getEnrolledCoursesApi } from "../lib/moodle.ts";
import { createLogger } from "../lib/logger.ts";
import { loadWsToken } from "../lib/token.ts";
import { formatAndOutput } from "../index.ts";
import path from "node:path";
import fs from "node:fs";

export function registerCoursesCommand(program: Command): void {
  const coursesCmd = program.command("courses");
  coursesCmd.description("Course operations");

  // Helper to get output format from global or local options
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
    const log = createLogger(opts.verbose, silent, outputFormat);

    const baseDir = getBaseDir();
    const sessionPath = path.resolve(baseDir, ".auth", "storage-state.json");

    // Check if session exists
    if (!fs.existsSync(sessionPath)) {
      console.error("未找到登入 session。請先執行 'openape login' 進行登入。");
      log.info(`Session 預期位置: ${sessionPath}`);
      return null;
    }

    // Try to load WS token
    const wsToken = loadWsToken(sessionPath);
    if (!wsToken) {
      console.error("未找到 WS token。請先執行 'openape login' 進行登入。");
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

  coursesCmd
    .command("list")
    .description("List enrolled courses")
    .option("--incomplete-only", "Show only incomplete courses")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .option("--level <type>", "Course level: in_progress (default) | past | future | all", "in_progress")
    .action(async (options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      // Map level to classification
      const classification = options.level === "all" ? undefined :
        options.level === "past" ? "past" :
          options.level === "future" ? "future" : "inprogress";

      const courses = await getEnrolledCoursesApi(apiContext.session, {
        classification,
      });

      let filteredCourses = courses;
      if (options.incompleteOnly) {
        filteredCourses = courses.filter(c => (c.progress ?? 0) < 100);
      }

      formatAndOutput(filteredCourses as unknown as Record<string, unknown>[], output, apiContext.log);
    });

  coursesCmd
    .command("info")
    .description("Show detailed course information")
    .argument("<course-id>", "Course ID")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (courseId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const courses = await getEnrolledCoursesApi(apiContext.session);
      const course = courses.find(c => c.id === parseInt(courseId, 10));

      if (!course) {
        apiContext.log.error(`Course not found: ${courseId}`);
        process.exitCode = 1;
        return;
      }

      formatAndOutput(course as unknown as Record<string, unknown>, output, apiContext.log);
    });

  coursesCmd
    .command("progress")
    .description("Show course progress")
    .argument("<course-id>", "Course ID")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (courseId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const courses = await getEnrolledCoursesApi(apiContext.session);
      const course = courses.find(c => c.id === parseInt(courseId, 10));

      if (!course) {
        apiContext.log.error(`Course not found: ${courseId}`);
        process.exitCode = 1;
        return;
      }

      const progressData = {
        courseId: course.id,
        courseName: course.fullname,
        progress: course.progress ?? 0,
        startDate: course.startdate ? formatTimestamp(course.startdate) : null,
        endDate: course.enddate ? formatTimestamp(course.enddate) : null,
      };

      formatAndOutput(progressData as unknown as Record<string, unknown>, output, apiContext.log);
    });

  // Helper function to fetch syllabus from CMAP using GWT-RPC API
  async function fetchSyllabus(shortname: string): Promise<Record<string, unknown> | null> {
    try {
      const parts = shortname.split("_");
      if (parts.length < 2) {
        return { error: "Invalid course shortname format" };
      }

      const [yearTerm, opCode] = parts;

      // Build GWT-RPC request body
      // Format: 7|0|8|<base_url>|<permutation>|<service>|<method>|<param_types>|<params>...|1|2|3|4|3|5|5|5|6|7|8|
      const gwtBody = `7|0|8|https://cmap.cycu.edu.tw:8443/Syllabus/syllabus/|339796D6E7B561A6465F5E9B5F4943FA|com.sanfong.syllabus.shared.SyllabusClientService|findClassTargetByYearAndOpCode|java.lang.String/2004016611|${yearTerm}|${opCode}|zh_TW|1|2|3|4|3|5|5|5|6|7|8|`;

      const response = await fetch("https://cmap.cycu.edu.tw:8443/Syllabus/syllabus/syllabusClientService", {
        method: "POST",
        headers: {
          "X-GWT-Permutation": "339796D6E7B561A6465F5E9B5F4943FA",
          "Accept": "text/x-gwt-rpc, */*; q=0.01",
          "Content-Type": "text/x-gwt-rpc; charset=UTF-8",
        },
        body: gwtBody,
      });

      if (!response.ok) {
        return { error: `HTTP ${response.status}`, url: "https://cmap.cycu.edu.tw:8443/Syllabus/syllabus/syllabusClientService" };
      }

      const rawText = await response.text();

      // GWT-RPC response format: //OK[...data...]
      if (!rawText.startsWith("//OK")) {
        return { error: "Invalid GWT-RPC response", rawResponse: rawText.slice(0, 200) };
      }

      // Extract the JSON array part from the GWT response
      // Response format: //OK[data1,data2,...]
      const content = rawText.slice(4); // Remove "//OK"

      // Parse the GWT string table - GWT uses a special format where strings are escaped
      // Format: ["string1","string2",...] or [123,"string2",...]
      const stringTable: string[] = [];

      // Simple parser for GWT string table
      let current = "";
      let inString = false;
      let escaped = false;

      for (let i = 0; i < content.length; i++) {
        const char = content[i];

        if (escaped) {
          // Handle escape sequences
          switch (char) {
            case 'n':
              current += '\n';
              break;
            case 'r':
              current += '\r';
              break;
            case 't':
              current += '\t';
              break;
            case '"':
              current += '"';
              break;
            case '\\':
              current += '\\';
              break;
            case '0':
              current += '\0';
              break;
            default:
              // Unknown escape, just append the char
              current += char;
          }
          escaped = false;
          continue;
        }

        if (char === "\\") {
          escaped = true;
          continue;
        }

        if (char === '"') {
          inString = !inString;
          if (!inString && current.length > 0) {
            stringTable.push(current);
            current = "";
          }
          continue;
        }

        if (inString) {
          current += char;
        }
      }

      // Parse schedule from string table
      // Strategy: Find week numbers (1-18), extract title (previous field) and date
      const schedule: Array<{ week: string; date: string; title: string }> = [];
      const datePattern = /^\d{4}-\d{2}-\d{2}$/;

      // Track processed indices to avoid duplicates
      const processedIndices = new Set<number>();

      for (let i = 0; i < stringTable.length; i++) {
        const s = stringTable[i];

        // Look for week numbers (1-18)
        if (/^[1-9]$|^1[0-8]$/.test(s) && !processedIndices.has(i)) {
          const week = s;
          let date = "";
          let title = "";

          // Previous field is the title
          if (i - 1 >= 0 && !processedIndices.has(i - 1)) {
            title = stringTable[i - 1];
          }

          // For week 1 & 2, find date before the week number
          // For week 18, also look before (last week has no "next week")
          // For other weeks (3-17), next field is next week's date
          if (week === "1" || week === "2" || week === "18") {
            // Look backwards for date pattern (search further back for week 18)
            const maxLookback = week === "18" ? 15 : 6;
            for (let j = i - 1; j >= Math.max(0, i - maxLookback); j--) {
              if (datePattern.test(stringTable[j]) && !processedIndices.has(j)) {
                date = stringTable[j];
                processedIndices.add(j);
                break;
              }
            }
          } else {
            // Week 3-17: look for next week number, then get date before it
            for (let j = i + 1; j < Math.min(i + 10, stringTable.length); j++) {
              if (/^[1-9]$|^1[0-8]$/.test(stringTable[j]) && !processedIndices.has(j)) {
                // Found next week, look before it for date
                for (let k = j - 1; k >= Math.max(0, j - 6); k--) {
                  if (datePattern.test(stringTable[k]) && !processedIndices.has(k)) {
                    date = stringTable[k];
                    processedIndices.add(k);
                    break;
                  }
                }
                break;
              }
            }
          }

          // Clean up title
          title = title.trim()
            .replace(/[\r\n]+/g, ' ')
            .replace(/,+$/, '')
            .trim()
            .slice(0, 200);

          // Only add if we have title (date is optional, will be inferred if missing)
          if (title.length > 1) {
            // If no date found, try to infer from the last added date
            if (date.length === 0 && schedule.length > 0) {
              const lastEntry = schedule[schedule.length - 1];
              const lastDate = new Date(lastEntry.date);
              const nextDate = new Date(lastDate.getTime() + 7 * 24 * 60 * 60 * 1000);
              const year = nextDate.getFullYear();
              const month = String(nextDate.getMonth() + 1).padStart(2, "0");
              const day = String(nextDate.getDate()).padStart(2, "0");
              date = `${year}-${month}-${day}`;
            }

            schedule.push({
              week,
              date,
              title,
            });
          }

          processedIndices.add(i);
        }
      }

      // Sort by date to maintain order
      schedule.sort((a, b) => a.date.localeCompare(b.date));

      // Extract course info from string table
      const result: Record<string, unknown> = {
        yearTerm,
        opCode,
        url: `https://cmap.cycu.edu.tw:8443/Syllabus/CoursePreview.html?yearTerm=${yearTerm}&opCode=${opCode}&locale=zh_TW`,
        schedule,
      };

      // Try to find instructor (look for common patterns)
      for (let i = 0; i < stringTable.length; i++) {
        const s = stringTable[i];
        if (s.includes("教授") || s.includes("老師") || s.includes("教師") || s.includes("Instructor")) {
          result.instructor = s;
          break;
        }
      }

      return result;
    } catch (e) {
      return { error: e instanceof Error ? e.message : String(e) };
    }
  }

  coursesCmd
    .command("syllabus")
    .description("Show course syllabus (from CMAP)")
    .argument("<course-id>", "Course ID")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (courseId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      try {
        const courses = await getEnrolledCoursesApi(apiContext.session);
        const course = courses.find(c => c.id === parseInt(courseId, 10));

        if (!course) {
          apiContext.log.error(`Course not found: ${courseId}`);
          process.exitCode = 1;
          return;
        }

        // Fetch syllabus from CMAP
        const syllabus = await fetchSyllabus(course.shortname);

        if (!syllabus) {
          apiContext.log.warn(`Syllabus not found for course: ${course.shortname}`);
          // Return course info at least
          formatAndOutput({
            courseId: course.id,
            shortname: course.shortname,
            fullname: course.fullname,
            note: "Syllabus not available from CMAP",
          } as unknown as Record<string, unknown>, output, apiContext.log);
          return;
        }

        // Combine course info with syllabus
        const result = {
          courseId: course.id,
          shortname: course.shortname,
          fullname: course.fullname,
          ...syllabus,
        };

        formatAndOutput(result as unknown as Record<string, unknown>, output, apiContext.log);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        apiContext.log.error(`Error fetching syllabus: ${msg}`);
        process.exitCode = 1;
      }
    });
}
