import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { dateWindows, shiftUtcMonths, shiftUtcYears } from "../src/dates.js";

describe("date windows", () => {
  it("builds twenty chronological three-month windows for five years", () => {
    const windows = dateWindows("2026-09-17", 5, 3);
    assert.equal(windows.length, 20);
    assert.deepEqual(windows[0], { start: "2021-09-17", end: "2021-12-17" });
    assert.deepEqual(windows.at(-1), { start: "2026-06-17", end: "2026-09-17" });
    for (let index = 1; index < windows.length; index += 1) {
      assert.equal(windows[index]?.start, windows[index - 1]?.end);
    }
  });

  it("clamps month and leap-year arithmetic to valid calendar dates", () => {
    assert.equal(shiftUtcMonths("2024-05-31", -3), "2024-02-29");
    assert.equal(shiftUtcYears("2024-02-29", 1), "2025-02-28");
  });
});
