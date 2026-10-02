export const EXPORT_KINDS = ["workouts", "files"] as const;

export type ExportKind = (typeof EXPORT_KINDS)[number];

export interface DateWindow {
  start: string;
  end: string;
}

export interface Config {
  athleteId: string;
  token: string;
  databasePath: string;
  workDirectory: string;
  endDate: string;
  years: number;
  monthsPerRequest: number;
  forceDownload: boolean;
  keepExtracted: boolean;
  apiBaseUrl: string;
}

export type WorkoutRow = Record<string, unknown>;

export interface ImportContext {
  athleteId: string;
  exportId: number;
  exportKind: ExportKind;
  window: DateWindow;
  extractionRoot: string;
}
