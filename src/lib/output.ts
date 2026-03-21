import type { OutputFormat, OutputOptions } from "./types.ts";

// ── Output Formatter ─────────────────────────────────────────────────

/**
 * Format data for output based on the specified format.
 */
export function formatOutput<T extends Record<string, unknown>>(
  data: T | T[],
  options: OutputOptions = { format: "json" }
): string {
  switch (options.format) {
    case "json":
      return formatJson(data, options.pretty);
    case "csv":
      return formatCsv(Array.isArray(data) ? data : [data], options.fields);
    case "table":
      return formatTable(Array.isArray(data) ? data : [data]);
    case "silent":
      return "";
    default:
      return formatJson(data, options.pretty);
  }
}

/**
 * Format data as JSON string.
 */
export function formatJson<T>(
  data: T,
  pretty: boolean = true
): string {
  return JSON.stringify(data, null, pretty ? 2 : 0);
}

/**
 * Format data as CSV string.
 */
export function formatCsv<T extends Record<string, unknown>>(
  data: T[],
  fields?: string[]
): string {
  if (data.length === 0) return "";

  const allFields = fields || Object.keys(data[0]);
  const headers = allFields.join(",");

  const rows = data.map((item) => {
    return allFields.map((field) => {
      const value = item[field];
      if (value === null || value === undefined) return "";
      if (typeof value === "string") {
        // Escape quotes and wrap in quotes if contains comma or quote
        if (value.includes(",") || value.includes('"') || value.includes("\n")) {
          return `"${value.replace(/"/g, '""')}"`;
        }
        return value;
      }
      return String(value);
    }).join(",");
  });

  return [headers, ...rows].join("\n");
}

/**
 * Format data as a simple table string.
 */
export function formatTable<T extends Record<string, unknown>>(
  data: T[]
): string {
  if (data.length === 0) return "No data";

  // Get all unique fields from all items
  const allFields = Array.from(
    new Set(data.flatMap((item) => Object.keys(item)))
  );

  // Calculate column widths
  const widths: Record<string, number> = {};
  allFields.forEach((field) => {
    const maxFieldLength = Math.max(
      field.length,
      ...data.map((item) => String(item[field] ?? "").length)
    );
    widths[field] = maxFieldLength + 2; // padding
  });

  // Build header row
  const header = allFields.map((f) => f.padEnd(widths[f])).join(" | ");

  // Build separator row
  const separator = allFields.map((f) => "-".repeat(widths[f] - 1)).join("-+-");

  // Build data rows
  const rows = data.map((item) => {
    return allFields.map((f) => {
      const value = item[f] ?? "";
      return String(value).padEnd(widths[f]);
    }).join(" | ");
  });

  return [header, separator, ...rows].join("\n");
}

/**
 * Print output to stdout.
 */
export function printOutput<T extends Record<string, unknown>>(
  data: T | T[],
  options: OutputOptions = { format: "json" }
): void {
  const output = formatOutput(data, options);
  if (output) {
    console.log(output);
  }
}

/**
 * Print error output.
 */
export function printError(error: Error | string, options: OutputOptions = { format: "json" }): void {
  const errorObj = {
    error: true,
    message: typeof error === "string" ? error : error.message,
    timestamp: Date.now(),
  };

  if (options.format === "json") {
    console.error(JSON.stringify(errorObj, null, 2));
  } else {
    console.error(`[ERR]   ${errorObj.message}`);
  }
}
