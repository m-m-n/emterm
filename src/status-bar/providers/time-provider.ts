/**
 * Time Provider
 *
 * Provides {time} variable with configurable format string.
 * Format tokens: HH (24h), hh (12h), mm, ss, A (AM/PM), YYYY, MM, DD.
 */

import type { VariableProvider } from "./types";

/**
 * Format a date according to a format string.
 * Supported tokens: HH, hh, mm, ss, A, YYYY, MM, DD.
 */
export function formatTime(date: Date, format: string): string {
  if (!format) format = "HH:mm:ss";

  const hours24 = date.getHours();
  const hours12 = hours24 % 12 || 12;
  const minutes = date.getMinutes();
  const seconds = date.getSeconds();
  const ampm = hours24 < 12 ? "AM" : "PM";
  const year = date.getFullYear();
  const month = date.getMonth() + 1;
  const day = date.getDate();

  const pad = (n: number): string => n.toString().padStart(2, "0");

  // Replace tokens from longest to shortest to avoid partial matches
  let result = format;
  result = result.replace(/YYYY/g, year.toString());
  result = result.replace(/MM/g, pad(month));
  result = result.replace(/DD/g, pad(day));
  result = result.replace(/HH/g, pad(hours24));
  result = result.replace(/hh/g, pad(hours12));
  result = result.replace(/mm/g, pad(minutes));
  result = result.replace(/ss/g, pad(seconds));
  result = result.replace(/A/g, ampm);

  return result;
}

/**
 * TimeProvider implements VariableProvider for the {time} variable.
 */
export class TimeProvider implements VariableProvider {
  private format: string;

  constructor(format: string = "HH:mm:ss") {
    this.format = format;
  }

  getValue(): string {
    return formatTime(new Date(), this.format);
  }

  getColor(): string | null {
    return null;
  }

  setFormat(format: string): void {
    this.format = format;
  }

  dispose(): void {
    // No resources to clean up
  }
}
