(() => {
  const trusted = (url) => url.protocol === "https:" &&
    (url.hostname === "trainingpeaks.com" || url.hostname.endsWith(".trainingpeaks.com"));
  if (!trusted(new URL(location.href))) return;
  const nativeFetch = window.fetch;
  const nativeOpen = XMLHttpRequest.prototype.open;
  const nativeHeader = XMLHttpRequest.prototype.setRequestHeader;
  const nativeSend = XMLHttpRequest.prototype.send;
  const xhrState = new WeakMap();
  let lastToken = "";
  let athleteId = null;
  let lastReported = "";
  let pending = null;

  const getAthlete = (value, depth = 0) => {
    if (!value || typeof value !== "object" || depth > 3) return null;
    for (const key of ["athleteId", "athleteID", "athletePk", "AthleteId", "AthleteID"]) {
      if (/^\d+$/.test(String(value[key] ?? ""))) return String(value[key]);
    }
    for (const key of ["athlete", "user", "profile", "data", "result"]) {
      const child = value[key];
      if (key === "athlete" && child && /^\d+$/.test(String(child.id ?? ""))) return String(child.id);
      const found = getAthlete(child, depth + 1);
      if (found) return found;
    }
    return null;
  };
  const report = (url, authorization) => {
    try {
      const parsed = new URL(url, location.href);
      if (!trusted(parsed)) return;
      const pathId = parsed.pathname.match(/\/(?:athletes|export)\/(\d+)(?:\/|$)/)?.[1];
      if (pathId) athleteId = pathId;
      const token = /^Bearer\s+(\S+)$/i.exec(authorization ?? "")?.[1];
      if (token) lastToken = token;
      if (!lastToken) return;
      const key = `${lastToken}:${athleteId ?? ""}`;
      if (lastReported === key) return;
      const payload = { requestUrl: parsed.href, token: lastToken, athleteId };
      if (!window.ipc?.postMessage) { pending = payload; return; }
      window.ipc.postMessage(JSON.stringify(payload));
      lastReported = key;
      pending = null;
    } catch { /* Capture must never break a TrainingPeaks request. */ }
  };
  // The browser's bridge may be injected after the request hooks.
  setInterval(() => { if (pending) report(pending.requestUrl, `Bearer ${pending.token}`); }, 500);
  window.fetch = function(input, init) {
    try {
      const request = input instanceof Request ? input : null;
      const url = request?.url ?? String(input);
      const headers = new Headers(init?.headers ?? request?.headers);
      report(url, headers.get("authorization"));
      const promise = nativeFetch.apply(this, arguments);
      if (trusted(new URL(url, location.href))) {
        promise.then((response) => {
          if (!response.headers.get("content-type")?.includes("json")) return;
          // Inspect small account/profile responses, never export bodies.
          if (!/\/(?:user|users|athlete|athletes|profile|account)(?:\/|$)/i.test(new URL(url, location.href).pathname)) return;
          response.clone().json().then((body) => {
            const found = getAthlete(body);
            if (found) { athleteId = found; report(url, headers.get("authorization")); }
          }).catch(() => {});
        }).catch(() => {});
      }
      return promise;
    } catch { return nativeFetch.apply(this, arguments); }
  };
  XMLHttpRequest.prototype.open = function(method, url) {
    xhrState.set(this, { url: String(url), authorization: null });
    return nativeOpen.apply(this, arguments);
  };
  XMLHttpRequest.prototype.setRequestHeader = function(name, value) {
    const state = xhrState.get(this);
    if (state && name.toLowerCase() === "authorization") state.authorization = value;
    return nativeHeader.apply(this, arguments);
  };
  XMLHttpRequest.prototype.send = function() {
    const state = xhrState.get(this);
    if (state) {
      report(state.url, state.authorization);
      this.addEventListener("load", () => {
        try {
          if (!trusted(new URL(state.url, location.href))) return;
          if (!/\/(?:user|users|athlete|athletes|profile|account)(?:\/|$)/i.test(new URL(state.url, location.href).pathname)) return;
          const body = this.responseType === "json" ? this.response : JSON.parse(this.responseText);
          const found = getAthlete(body);
          if (found) { athleteId = found; report(state.url, state.authorization); }
        } catch { /* Binary or non-JSON response. */ }
      }, { once: true });
    }
    return nativeSend.apply(this, arguments);
  };
})();
