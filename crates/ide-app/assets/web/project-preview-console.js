(() => {
  if (window.__choroProjectPreviewConsole) return;

  const MAX_PART_LENGTH = 8_000;
  const MAX_MESSAGE_LENGTH = 16_000;
  const handlerName = "choroPreviewConsole";

  const shorten = (value, limit = MAX_PART_LENGTH) => {
    const text = String(value);
    return text.length > limit ? `${text.slice(0, limit)}\u2026` : text;
  };

  const formatValue = value => {
    if (typeof value === "string") return shorten(value);
    if (value instanceof Error) return shorten(value.stack || value.message || value.name);
    if (typeof value === "undefined") return "undefined";
    if (typeof value === "function") return shorten(value.toString());
    if (typeof value === "bigint") return `${value}n`;

    const seen = new WeakSet();
    try {
      const json = JSON.stringify(value, (_key, nested) => {
        if (typeof nested === "bigint") return `${nested}n`;
        if (typeof nested === "object" && nested !== null) {
          if (seen.has(nested)) return "[Circular]";
          seen.add(nested);
        }
        return nested;
      });
      return shorten(json === undefined ? String(value) : json);
    } catch (_error) {
      try {
        return shorten(String(value));
      } catch (_stringError) {
        return "[Unprintable value]";
      }
    }
  };

  const post = (level, values, source = null, line = null, column = null) => {
    try {
      const handler = window.webkit?.messageHandlers?.[handlerName];
      if (!handler) return;
      const message = values.map(formatValue).join(" ").slice(0, MAX_MESSAGE_LENGTH);
      handler.postMessage(JSON.stringify({ level, message, source, line, column }));
    } catch (_error) {
      // Console forwarding must never break the page being previewed.
    }
  };

  const originalConsole = {};
  for (const level of ["debug", "log", "info", "warn", "error"]) {
    const original = typeof console[level] === "function" ? console[level].bind(console) : null;
    originalConsole[level] = original;
    console[level] = (...values) => {
      post(level, values);
      if (original) original(...values);
    };
  }

  window.addEventListener("error", event => {
    if (event.error) {
      post("error", [event.error], event.filename || null, event.lineno || null, event.colno || null);
      return;
    }
    const target = event.target;
    const resource = target?.currentSrc || target?.src || target?.href;
    post("error", [resource ? `Failed to load resource: ${resource}` : event.message || "Unknown page error"]);
  }, true);

  window.addEventListener("unhandledrejection", event => {
    post("error", ["Unhandled promise rejection:", event.reason]);
  });

  Object.defineProperty(window, "__choroProjectPreviewConsole", {
    value: Object.freeze({ originalConsole }),
    configurable: false,
    enumerable: false,
    writable: false,
  });
})();
