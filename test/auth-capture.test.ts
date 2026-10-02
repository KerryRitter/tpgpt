import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, it } from "node:test";

const script = readFileSync(new URL("../native/src/auth-capture.js", import.meta.url), "utf8");

function browser(origin = "https://app.trainingpeaks.com/") {
  const captures: Array<{ requestUrl: string; token: string; athleteId: string | null }> = [];
  let fetches = 0;
  class Xhr {
    responseType = "";
    responseText = '{"athleteId":42}';
    callbacks: Array<() => void> = [];
    open(_method: string, _url: string) {}
    setRequestHeader(_name: string, _value: string) {}
    send() { this.callbacks.forEach((callback) => callback()); }
    addEventListener(_name: string, callback: () => void) { this.callbacks.push(callback); }
  }
  const window = {
    ipc: { postMessage: (payload: string) => captures.push(JSON.parse(payload)) },
    fetch: async (_input: string | Request, _init?: RequestInit) => {
      fetches++;
      return new Response('{"athleteId":42}', { headers: { "content-type": "application/json" } });
    },
  };
  runInNewContext(script, { window, XMLHttpRequest: Xhr, location: { href: origin }, URL, Headers, Request, setInterval: () => 0 });
  return { window, captures, Xhr, fetches: () => fetches };
}

describe("embedded browser auth capture", () => {
  it("captures fetch headers, detects athlete IDs, and preserves the response", async () => {
    const context = browser();
    const response = await context.window.fetch("https://tpapi.trainingpeaks.com/athletes/42/profile", { headers: { authorization: "Bearer test-token" } });
    assert.equal(response.status, 200);
    assert.equal(context.fetches(), 1);
    assert.deepEqual(context.captures[0], { requestUrl: "https://tpapi.trainingpeaks.com/athletes/42/profile", token: "test-token", athleteId: "42" });
    await context.window.fetch("https://tpapi.trainingpeaks.com/athletes/42/profile", { headers: { authorization: "Bearer test-token" } });
    assert.equal(context.captures.length, 1);
  });

  it("captures XHR and ignores unrelated hosts and origins", async () => {
    const context = browser();
    const xhr = new context.Xhr();
    xhr.open("GET", "https://tpapi.trainingpeaks.com/athletes/42/profile");
    xhr.setRequestHeader("Authorization", "Bearer xhr-token");
    xhr.send();
    assert.equal(context.captures[0]?.token, "xhr-token");
    await context.window.fetch("https://trainingpeaks.com.evil.example/athletes/99", { headers: { authorization: "Bearer should-not-capture" } });
    assert.equal(context.captures.length, 1);
    const external = browser("https://example.com/");
    await external.window.fetch("https://tpapi.trainingpeaks.com/athletes/42/profile", { headers: { authorization: "Bearer should-not-capture" } });
    assert.equal(external.captures.length, 0);
  });
});
