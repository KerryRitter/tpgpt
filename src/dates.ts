import type { DateWindow } from "./types.js";

const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;

export function assertIsoDate(value: string, label = "date"): void {
  if (!ISO_DATE.test(value)) {
    throw new Error(`${label} must use YYYY-MM-DD format (received ${JSON.stringify(value)})`);
  }

  const parsed = new Date(`${value}T00:00:00Z`);
  if (Number.isNaN(parsed.valueOf()) || formatIsoDate(parsed) !== value) {
    throw new Error(`${label} is not a valid calendar date (received ${JSON.stringify(value)})`);
  }
}

export function formatIsoDate(date: Date): string {
  return date.toISOString().slice(0, 10);
}

export function shiftUtcYears(isoDate: string, years: number): string {
  assertIsoDate(isoDate);
  const [year, month, day] = isoDate.split("-").map(Number) as [number, number, number];
  const targetYear = year + years;

  // Match common calendar arithmetic: Feb 29 becomes Feb 28 in a non-leap year.
  const lastDay = new Date(Date.UTC(targetYear, month, 0)).getUTCDate();
  return formatIsoDate(new Date(Date.UTC(targetYear, month - 1, Math.min(day, lastDay))));
}

export function shiftUtcMonths(isoDate: string, months: number): string {
  assertIsoDate(isoDate);
  const [year, month, day] = isoDate.split("-").map(Number) as [number, number, number];
  const zeroBasedTarget = year * 12 + (month - 1) + months;
  const targetYear = Math.floor(zeroBasedTarget / 12);
  const targetMonth = ((zeroBasedTarget % 12) + 12) % 12;
  const lastDay = new Date(Date.UTC(targetYear, targetMonth + 1, 0)).getUTCDate();
  return formatIsoDate(new Date(Date.UTC(targetYear, targetMonth, Math.min(day, lastDay))));
}

export function dateWindows(endDate: string, years: number, monthsPerRequest = 3): DateWindow[] {
  assertIsoDate(endDate, "--end");
  if (!Number.isInteger(years) || years < 1 || years > 50) {
    throw new Error("--years must be an integer between 1 and 50");
  }
  if (!Number.isInteger(monthsPerRequest) || monthsPerRequest < 1 || monthsPerRequest > 12) {
    throw new Error("--months-per-request must be an integer between 1 and 12");
  }

  const windows: DateWindow[] = [];
  const totalMonths = years * 12;
  for (let monthsAgo = totalMonths; monthsAgo > 0; monthsAgo -= monthsPerRequest) {
    windows.push({
      start: shiftUtcMonths(endDate, -monthsAgo),
      end: shiftUtcMonths(endDate, -Math.max(0, monthsAgo - monthsPerRequest)),
    });
  }
  return windows;
}
