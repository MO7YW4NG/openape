import { formatTimestamp } from "../lib/utils.ts";
import { Command } from "commander";
import { getEnrolledCoursesApi, getCalendarEventsApi } from "../lib/moodle.ts";
import { createApiContext } from "../lib/auth.ts";
import fs from "node:fs";

export function registerCalendarCommand(program: Command): void {
  const calendarCmd = program.command("calendar");
  calendarCmd.description("Calendar operations");

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

      const now = Math.floor(Date.now() / 1000);
      const endTime = now + (days * 24 * 60 * 60);

      let allEvents = [];

      if (options.course) {
        const courseId = parseInt(options.course, 10);
        const events = await getCalendarEventsApi(apiContext.session, {
          startTime: now,
          endTime: endTime,
        });
        allEvents = events.filter(e => e.courseid === courseId);
      } else {
        const results = await Promise.allSettled(
          courses.map(course =>
            getCalendarEventsApi(apiContext.session, {
              courseId: course.id,
              startTime: now,
              endTime: endTime,
            })
          )
        );
        for (const result of results) {
          if (result.status === "fulfilled") allEvents.push(...result.value);
        }
      }

      allEvents.sort((a, b) => a.timestart - b.timestart);

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

      const now = Math.floor(Date.now() / 1000);
      const days = parseInt(options.days, 10);
      const endTime = now + (days * 24 * 60 * 60);

      const allEvents = [];

      const results = await Promise.allSettled(
        courses.map(course =>
          getCalendarEventsApi(apiContext.session, {
            courseId: course.id,
            startTime: now,
            endTime: endTime,
          })
        )
      );
      for (const result of results) {
        if (result.status === "fulfilled") allEvents.push(...result.value);
      }

      allEvents.sort((a, b) => a.timestart - b.timestart);

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

      fs.writeFileSync(options.output, JSON.stringify(exportData));

      apiContext.log.success(`Exported ${allEvents.length} events to ${options.output}`);
    });
}
