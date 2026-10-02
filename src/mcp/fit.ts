import { gunzipSync } from "node:zlib";
import { XMLParser } from "fast-xml-parser";
// Garmin's published NodeNext declarations omit `.js` from their internal
// re-exports, so TypeScript cannot see these runtime exports under NodeNext.
// @ts-expect-error The named exports are present in the package's ESM entry.
import { Decoder, Stream } from "@garmin/fitsdk";
import type { DatabaseSync } from "node:sqlite";

export type FitRecord = Record<string, unknown>;

export interface DecodedActivity {
  fileId: number;
  relativePath: string;
  format: "FIT" | "TCX" | "GPX" | "UNKNOWN";
  fitValid: boolean;
  crcValid: boolean;
  errors: string[];
  sessions: FitRecord[];
  laps: FitRecord[];
  records: FitRecord[];
  fileIds: FitRecord[];
}

function asArray<T>(value: T | T[] | undefined | null): T[] {
  if (value === undefined || value === null) return [];
  return Array.isArray(value) ? value : [value];
}

function nested(object: unknown, ...keys: string[]): unknown {
  let value = object;
  for (const key of keys) {
    if (!value || typeof value !== "object") return undefined;
    value = (value as Record<string, unknown>)[key];
  }
  return value;
}

function numeric(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim() && Number.isFinite(Number(value))) return Number(value);
  return undefined;
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value ? value : undefined;
}

function recursiveValue(object: unknown, wanted: Set<string>): unknown {
  if (!object || typeof object !== "object") return undefined;
  for (const [key, value] of Object.entries(object as Record<string, unknown>)) {
    if (wanted.has(key.toLowerCase())) return value;
    const child = recursiveValue(value, wanted);
    if (child !== undefined) return child;
  }
  return undefined;
}

function normalizeSport(value: unknown): string | undefined {
  const sport = String(value ?? "").toLowerCase();
  if (sport.includes("run")) return "running";
  if (sport.includes("bik") || sport.includes("cycl")) return "cycling";
  if (sport.includes("swim")) return "swimming";
  if (sport.includes("walk") || sport.includes("hik")) return "walking";
  if (sport.includes("row")) return "rowing";
  return sport || undefined;
}

function haversineMeters(left: FitRecord, right: FitRecord): number {
  const lat1 = numeric(left.latitude);
  const lon1 = numeric(left.longitude);
  const lat2 = numeric(right.latitude);
  const lon2 = numeric(right.longitude);
  if ([lat1, lon1, lat2, lon2].some((value) => value === undefined)) return 0;
  const radians = Math.PI / 180;
  const dLat = (lat2! - lat1!) * radians;
  const dLon = (lon2! - lon1!) * radians;
  const a = Math.sin(dLat / 2) ** 2 + Math.cos(lat1! * radians) * Math.cos(lat2! * radians) * Math.sin(dLon / 2) ** 2;
  return 6_371_000 * 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
}

function decodeTcx(xml: string, fileId: number, relativePath: string): DecodedActivity {
  const parsed = new XMLParser({ ignoreAttributes: false, attributeNamePrefix: "@_", removeNSPrefix: true }).parse(xml) as Record<string, unknown>;
  const activities = asArray(nested(parsed, "TrainingCenterDatabase", "Activities", "Activity") as FitRecord | FitRecord[] | undefined);
  const sessions: FitRecord[] = [];
  const laps: FitRecord[] = [];
  const records: FitRecord[] = [];
  for (const activity of activities) {
    const activityLaps = asArray(activity.Lap as FitRecord | FitRecord[] | undefined);
    const activityRecords: FitRecord[] = [];
    for (const lap of activityLaps) {
      const lapRecords: FitRecord[] = [];
      for (const track of asArray(lap.Track as FitRecord | FitRecord[] | undefined)) {
        for (const point of asArray(track.Trackpoint as FitRecord | FitRecord[] | undefined)) {
          const extension = point.Extensions;
          const record: FitRecord = {
            timestamp: text(point.Time),
            latitude: numeric(nested(point, "Position", "LatitudeDegrees")),
            longitude: numeric(nested(point, "Position", "LongitudeDegrees")),
            altitude: numeric(point.AltitudeMeters),
            distance: numeric(point.DistanceMeters),
            heartRate: numeric(nested(point, "HeartRateBpm", "Value")),
            cadence: numeric(point.Cadence) ?? numeric(recursiveValue(extension, new Set(["runcadence", "cadence"]))),
            speed: numeric(recursiveValue(extension, new Set(["speed"]))),
            power: numeric(recursiveValue(extension, new Set(["watts", "power"]))),
          };
          lapRecords.push(record);
          activityRecords.push(record);
          records.push(record);
        }
      }
      const fallback = summarizeRecords(lapRecords);
      laps.push({
        startTime: text(lap["@_StartTime"]) ?? stringField(lapRecords[0], "timestamp"),
        timestamp: stringField(lapRecords.at(-1), "timestamp"),
        totalElapsedTime: numeric(lap.TotalTimeSeconds),
        totalTimerTime: numeric(lap.TotalTimeSeconds),
        totalDistance: numeric(lap.DistanceMeters) ?? numberField(fallback, "totalDistance"),
        totalCalories: numeric(lap.Calories),
        avgHeartRate: numeric(nested(lap, "AverageHeartRateBpm", "Value")) ?? numberField(fallback, "avgHeartRate"),
        maxHeartRate: numeric(nested(lap, "MaximumHeartRateBpm", "Value")) ?? numberField(fallback, "maxHeartRate"),
        avgCadence: numeric(lap.Cadence) ?? numberField(fallback, "avgCadence"),
        maxCadence: numberField(fallback, "maxCadence"),
        avgPower: numberField(fallback, "avgPower"),
        maxPower: numberField(fallback, "maxPower"),
        avgSpeed: numeric(recursiveValue(lap.Extensions, new Set(["avgspeed"]))) ?? numberField(fallback, "avgSpeed"),
        maxSpeed: numberField(fallback, "maxSpeed"),
        sport: normalizeSport(activity["@_Sport"]),
      });
    }
    const fallback = summarizeRecords(activityRecords);
    sessions.push({
      startTime: stringField(laps.at(-activityLaps.length), "startTime") ?? stringField(activityRecords[0], "timestamp"),
      timestamp: stringField(activityRecords.at(-1), "timestamp"),
      sport: normalizeSport(activity["@_Sport"]),
      totalElapsedTime: activityLaps.reduce((sum, lap) => sum + (numeric(lap.TotalTimeSeconds) ?? 0), 0),
      totalTimerTime: activityLaps.reduce((sum, lap) => sum + (numeric(lap.TotalTimeSeconds) ?? 0), 0),
      totalDistance: activityLaps.reduce((sum, lap) => sum + (numeric(lap.DistanceMeters) ?? 0), 0),
      totalCalories: activityLaps.reduce((sum, lap) => sum + (numeric(lap.Calories) ?? 0), 0),
      avgHeartRate: numberField(fallback, "avgHeartRate"),
      maxHeartRate: numberField(fallback, "maxHeartRate"),
      avgPower: numberField(fallback, "avgPower"),
      maxPower: numberField(fallback, "maxPower"),
      avgCadence: numberField(fallback, "avgCadence"),
      maxCadence: numberField(fallback, "maxCadence"),
      avgSpeed: numberField(fallback, "avgSpeed"),
      maxSpeed: numberField(fallback, "maxSpeed"),
    });
  }
  return { fileId, relativePath, format: "TCX", fitValid: sessions.length > 0 || records.length > 0, crcValid: true, errors: [], sessions, laps, records, fileIds: [] };
}

function decodeGpx(xml: string, fileId: number, relativePath: string): DecodedActivity {
  const parsed = new XMLParser({ ignoreAttributes: false, attributeNamePrefix: "@_", removeNSPrefix: true }).parse(xml) as Record<string, unknown>;
  const metadataTime = text(nested(parsed, "gpx", "metadata", "time"));
  const tracks = asArray(nested(parsed, "gpx", "trk") as FitRecord | FitRecord[] | undefined);
  const sessions: FitRecord[] = [];
  const laps: FitRecord[] = [];
  const records: FitRecord[] = [];
  for (const track of tracks) {
    const trackRecords: FitRecord[] = [];
    for (const segment of asArray(track.trkseg as FitRecord | FitRecord[] | undefined)) {
      let distance = 0;
      let previous: FitRecord | undefined;
      const segmentRecords: FitRecord[] = [];
      for (const point of asArray(segment.trkpt as FitRecord | FitRecord[] | undefined)) {
        const extension = point.extensions;
        const record: FitRecord = {
          timestamp: text(point.time),
          latitude: numeric(point["@_lat"]),
          longitude: numeric(point["@_lon"]),
          altitude: numeric(point.ele),
          heartRate: numeric(recursiveValue(extension, new Set(["hr", "heartrate"]))),
          cadence: numeric(recursiveValue(extension, new Set(["cad", "cadence"]))),
          temperature: numeric(recursiveValue(extension, new Set(["atemp", "temperature"]))),
          power: numeric(recursiveValue(extension, new Set(["power", "watts"]))),
        };
        if (previous) distance += haversineMeters(previous, record);
        record.distance = distance;
        const priorTime = previous ? Date.parse(String(previous.timestamp ?? "")) : Number.NaN;
        const currentTime = Date.parse(String(record.timestamp ?? ""));
        const elapsed = (currentTime - priorTime) / 1000;
        if (previous && elapsed > 0) record.speed = haversineMeters(previous, record) / elapsed;
        previous = record;
        segmentRecords.push(record);
        trackRecords.push(record);
        records.push(record);
      }
      const summary = summarizeRecords(segmentRecords);
      const start = stringField(segmentRecords[0], "timestamp") ?? metadataTime ?? null;
      const end = stringField(segmentRecords.at(-1), "timestamp") ?? start;
      const elapsed = start && end ? (Date.parse(end) - Date.parse(start)) / 1000 : null;
      laps.push({ ...summary, startTime: start, timestamp: end, totalElapsedTime: elapsed, totalTimerTime: elapsed });
    }
    const summary = summarizeRecords(trackRecords);
    const start = stringField(trackRecords[0], "timestamp") ?? metadataTime ?? null;
    const end = stringField(trackRecords.at(-1), "timestamp") ?? start;
    const elapsed = start && end ? (Date.parse(end) - Date.parse(start)) / 1000 : null;
    sessions.push({
      ...summary,
      startTime: start,
      timestamp: end,
      sport: normalizeSport(track.type ?? track.name),
      totalElapsedTime: elapsed,
      totalTimerTime: elapsed,
    });
  }
  return { fileId, relativePath, format: "GPX", fitValid: sessions.length > 0 || records.length > 0, crcValid: true, errors: [], sessions, laps, records, fileIds: [] };
}

function plainValue(value: unknown): unknown {
  if (value instanceof Date) return value.toISOString();
  if (Array.isArray(value)) return value.map(plainValue);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([key, child]) => [key, plainValue(child)]),
    );
  }
  return value;
}

function asRecords(value: unknown): FitRecord[] {
  if (!Array.isArray(value)) return [];
  return value.map((record) => plainValue(record) as FitRecord);
}

export function decodeActivityFile(database: DatabaseSync, fileId: number): DecodedActivity {
  const row = database.prepare(`
    SELECT f.relative_path, b.content
    FROM files AS f JOIN file_blobs AS b ON b.id = f.blob_id
    WHERE f.id = ?
  `).get(fileId) as { relative_path: string; content: Uint8Array } | undefined;
  if (!row) throw new Error(`No stored file has ID ${fileId}`);

  let bytes = Buffer.from(row.content);
  if (bytes[0] === 0x1f && bytes[1] === 0x8b) bytes = gunzipSync(bytes);
  const lowerPath = row.relative_path.toLowerCase();
  if (lowerPath.includes(".tcx")) return decodeTcx(bytes.toString("utf8"), fileId, row.relative_path);
  if (lowerPath.includes(".gpx")) return decodeGpx(bytes.toString("utf8"), fileId, row.relative_path);
  const fitValid = Decoder.isFIT(Stream.fromBuffer(bytes));
  if (!fitValid) {
    return {
      fileId,
      relativePath: row.relative_path,
      format: "UNKNOWN",
      fitValid: false,
      crcValid: false,
      errors: ["Not a FIT file"],
      sessions: [],
      laps: [],
      records: [],
      fileIds: [],
    };
  }

  const crcValid = new Decoder(Stream.fromBuffer(bytes)).checkIntegrity();
  const decoderOptions = {
    applyScaleAndOffset: true,
    expandSubFields: true,
    expandComponents: true,
    convertTypesToStrings: true,
    convertDateTimesToDates: true,
    includeUnknownData: false,
    mergeHeartRates: true,
  } as const;
  let decoded = new Decoder(Stream.fromBuffer(bytes)).read(decoderOptions);
  let recoveryNote: string | null = null;
  const firstMessages = decoded.messages as unknown as Record<string, unknown>;
  const firstRecordCount = Array.isArray(firstMessages.recordMesgs) ? firstMessages.recordMesgs.length : 0;
  // A few source files have a valid FIT signature but a zero data-size field.
  // Garmin's decoder can recover their messages when the size is inferred.
  if (firstRecordCount === 0 && bytes.length > 14 && bytes[0] === 12 && bytes.readUInt32LE(4) === 0) {
    const repaired = Buffer.from(bytes);
    repaired.writeUInt32LE(repaired.length - repaired[0]! - 2, 4);
    const recovered = new Decoder(Stream.fromBuffer(repaired)).read(decoderOptions);
    const recoveredMessages = recovered.messages as unknown as Record<string, unknown>;
    if (Array.isArray(recoveredMessages.recordMesgs) && recoveredMessages.recordMesgs.length > 0) {
      decoded = recovered;
      recoveryNote = "Recovered records from a FIT file whose original header declared a zero data size; CRC remains invalid.";
    }
  }
  const messages = decoded.messages as unknown as Record<string, unknown>;
  return {
    fileId,
    relativePath: row.relative_path,
    format: "FIT",
    fitValid,
    crcValid,
    errors: [
      ...(recoveryNote ? [recoveryNote] : []),
      ...decoded.errors.map((error: Error) => error.message),
    ],
    sessions: asRecords(messages.sessionMesgs),
    laps: asRecords(messages.lapMesgs),
    records: asRecords(messages.recordMesgs),
    fileIds: asRecords(messages.fileIdMesgs),
  };
}

export function numberField(record: FitRecord | undefined, ...names: string[]): number | null {
  if (!record) return null;
  for (const name of names) {
    const value = record[name];
    if (typeof value === "number" && Number.isFinite(value)) return value;
  }
  return null;
}

export function stringField(record: FitRecord | undefined, ...names: string[]): string | null {
  if (!record) return null;
  for (const name of names) {
    const value = record[name];
    if (typeof value === "string" && value) return value;
  }
  return null;
}

export function isoField(record: FitRecord | undefined, ...names: string[]): string | null {
  const value = stringField(record, ...names);
  return value && !Number.isNaN(Date.parse(value)) ? new Date(value).toISOString() : null;
}

export function summarizeRecords(records: FitRecord[]): FitRecord {
  if (records.length === 0) return {};
  const numeric = (name: string): number[] => records
    .map((record) => record[name])
    .filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  const average = (values: number[]): number | null => values.length
    ? values.reduce((sum, value) => sum + value, 0) / values.length
    : null;
  const maximum = (values: number[]): number | null => values.length ? Math.max(...values) : null;
  const startTime = stringField(records[0], "timestamp");
  const endTime = stringField(records.at(-1), "timestamp");
  const elapsed = startTime && endTime ? (Date.parse(endTime) - Date.parse(startTime)) / 1000 : null;
  return {
    startTime,
    endTime,
    totalElapsedTime: elapsed,
    totalTimerTime: elapsed,
    totalDistance: numberField(records.at(-1), "distance"),
    avgHeartRate: average(numeric("heartRate")),
    maxHeartRate: maximum(numeric("heartRate")),
    avgPower: average(numeric("power")),
    maxPower: maximum(numeric("power")),
    avgCadence: average(numeric("cadence")),
    maxCadence: maximum(numeric("cadence")),
    avgSpeed: average(numeric("speed")),
    maxSpeed: maximum(numeric("speed")),
  };
}

function semicirclesToDegrees(value: unknown): number | undefined {
  return typeof value === "number" ? value * (180 / 2 ** 31) : undefined;
}

export function sampleRecords(records: FitRecord[], maximum: number): FitRecord[] {
  if (records.length === 0 || maximum <= 0) return [];
  const count = Math.min(maximum, records.length);
  const indices = count === 1
    ? [0]
    : Array.from({ length: count }, (_, index) => Math.round(index * (records.length - 1) / (count - 1)));
  return indices.map((index) => {
    const record = records[index] ?? {};
    return {
      timestamp: record.timestamp,
      heartRate: record.heartRate,
      power: record.power,
      cadence: record.cadence,
      speedMps: record.enhancedSpeed ?? record.speed,
      distanceMeters: record.distance,
      altitudeMeters: record.enhancedAltitude ?? record.altitude,
      temperatureC: record.temperature,
      latitude: semicirclesToDegrees(record.positionLat),
      longitude: semicirclesToDegrees(record.positionLong),
    };
  });
}
