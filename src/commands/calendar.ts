import { getBaseDir, formatTimestamp } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, OutputFormat } from "../lib/types.ts";
import { getEnrolledCoursesApi, getCalendarEventsApi } from "../lib/moodle.ts";
import { createLogger } from "../lib/logger.ts";
import { loadWsToken } from "../lib/token.ts";
import path from "node:path";
import fs from "node:fs";

export function registerCalendarCommand(program: Command): void {
  const calendarCmd = program.command("calendar");
  calendarCmd.description("Calendar operations");

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
    const log = createLogger(opts.verbose, silent);

    const baseDir = getBaseDir();
    const sessionPath = path.resolve(baseDir, ".auth", "storage-state.json");

    // Check if session exists
    if (!fs.existsSync(sessionPath)) {
      log.error("未找到登入 session。請先執行 'openape auth login' 進行登入。");
      log.info(`Session 預期位置: ${sessionPath}`);
      return null;
    }

    // Try to load WS token
    const wsToken = loadWsToken(sessionPath);
    if (!wsToken) {
      log.error("未找到 WS token。請先執行 'openape auth login' 進行登入。");
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

  calendarCmd
    .command("events")
    .description("List calendar events")
    .option("--upcoming", "Show only upcoming events")
    .option("--days <n>", "Number of days ahead to look", "30")
    .option("--course <id>", "Filter by course ID")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const days = parseInt(options.days, 10);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const courses = await getEnrolledCoursesApi(apiContext.session);

      // Calculate time range
      const now = Math.floor(Date.now() / 1000);
      const endTime = now + (days * 24 * 60 * 60);

      let allEvents = [];

      if (options.course) {
        // Get events for specific course
        const courseId = parseInt(options.course, 10);
        const events = await getCalendarEventsApi(apiContext.session, {
          startTime: now,
          endTime: endTime,
        });
        allEvents = events.filter(e => e.courseid === courseId);
      } else {
        // Get events for all courses
        for (const course of courses) {
          try {
            const events = await getCalendarEventsApi(apiContext.session, {
              courseId: course.id,
              startTime: now,
              endTime: endTime,
            });
            allEvents.push(...events);
          } catch (err) {
            apiContext.log.debug(`Failed to fetch calendar events for ${course.fullname}: ${err}`);
          }
        }
      }

      // Sort by start time
      allEvents.sort((a, b) => a.timestart - b.timestart);

      // Filter upcoming only if requested
      let filteredEvents = allEvents;
      if (options.upcoming) {
        filteredEvents = allEvents.filter(e => e.timestart > now);
      }

      console.log(JSON.stringify({
        status: "success",
        timestamp: new Date().toISOString(),
        total_events: allEvents.length,
        upcoming: allEvents.filter(e => e.timestart > now).length,
        by_type: allEvents.reduce((acc, e) => {
          acc[e.eventtype] = (acc[e.eventtype] || 0) + 1;
          return acc;
        }, {} as Record<string, number>),
      }));
      for (const e of filteredEvents) {
        console.log(JSON.stringify({
          id: e.id,
          name: e.name,
          description: e.description,
          course_id: e.courseid,
          event_type: e.eventtype,
          start_time: formatTimestamp(e.timestart),
          end_time: e.timeduration ? formatTimestamp(e.timestart + Math.floor(e.timeduration / 1000)) : null,
          location: e.location,
        }));
      }
    });

  calendarCmd
    .command("export")
    .description("Export calendar events to file")
    .option("--output <path>", "Output file path", "./calendar.json")
    .option("--days <n>", "Number of days ahead to include", "30")
    .action(async (options, command) => {
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const courses = await getEnrolledCoursesApi(apiContext.session);

      // Calculate time range
      const now = Math.floor(Date.now() / 1000);
      const days = parseInt(options.days, 10);
      const endTime = now + (days * 24 * 60 * 60);

      const allEvents = [];

      for (const course of courses) {
        try {
          const events = await getCalendarEventsApi(apiContext.session, {
            courseId: course.id,
            startTime: now,
            endTime: endTime,
          });
          allEvents.push(...events);
        } catch (err) {
          apiContext.log.debug(`Failed to fetch calendar events for ${course.fullname}: ${err}`);
        }
      }

      // Sort by start time
      allEvents.sort((a, b) => a.timestart - b.timestart);

      // Export data
      const exportData = {
        exported_at: new Date().toISOString(),
        time_range: {
          start: new Date(now * 1000).toISOString(),
          end: new Date(endTime * 1000).toISOString(),
          days: days,
        },
        events: allEvents.map(e => ({
          id: e.id,
          name: e.name,
          description: e.description,
          course_id: e.courseid,
          event_type: e.eventtype,
          start_time: formatTimestamp(e.timestart),
          end_time: e.timeduration ? formatTimestamp(e.timestart + Math.floor(e.timeduration / 1000)) : null,
          location: e.location,
        })),
        summary: {
          total_events: allEvents.length,
          by_type: allEvents.reduce((acc, e) => {
            acc[e.eventtype] = (acc[e.eventtype] || 0) + 1;
            return acc;
          }, {} as Record<string, number>),
        },
      };

      // Write to file
      fs.writeFileSync(options.output, JSON.stringify(exportData));

      apiContext.log.success(`Exported ${allEvents.length} events to ${options.output}`);
    });
}
