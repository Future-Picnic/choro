(() => {
  if (window.__choroProjectPreviewInspector) return;

  const state = {
    active: false,
    hovered: null,
    selectedElement: null,
    selectedArea: null,
    pointer: null,
    dragging: false,
    suppressClick: false,
    target: null,
  };
  // The inspected page can have any stylesheet or writing direction. Keep
  // Choro's review chrome in a shadow root so page CSS (including `!important`
  // resets and RTL flex direction) cannot restyle or reorder the composer.
  const host = document.createElement("div");
  const shadow = host.attachShadow({ mode: "open" });
  const overlay = document.createElement("div");
  const label = document.createElement("div");
  const composer = document.createElement("div");
  const input = document.createElement("textarea");
  const send = document.createElement("button");
  const chromeStyles = document.createElement("style");
  host.dataset.choroPreviewChrome = "true";
  overlay.dataset.choroPreviewOverlay = "true";
  composer.dataset.choroPreviewComposer = "true";
  input.dataset.choroPreviewComment = "true";
  send.dataset.choroPreviewSend = "true";
  Object.assign(host.style, {
    all: "initial", position: "fixed", inset: "0", zIndex: "2147483644",
    pointerEvents: "none", direction: "ltr", colorScheme: "dark",
  });
  chromeStyles.textContent = `
    [data-choro-preview-comment]::placeholder {
      color: #9291A3;
      opacity: 1;
    }
    [data-choro-preview-comment]::selection {
      background: rgba(202, 201, 238, .28);
    }
    [data-choro-preview-send]:hover:not(:disabled) {
      background: #D7D5F5 !important;
    }
    [data-choro-preview-send]:active:not(:disabled) {
      transform: translateY(1px);
    }
    [data-choro-preview-send]:focus-visible {
      outline: 2px solid rgba(202, 201, 238, .72) !important;
      outline-offset: 2px;
    }
  `;
  Object.assign(overlay.style, {
    all: "initial", position: "fixed", pointerEvents: "none", zIndex: "1",
    display: "none", boxSizing: "border-box", border: "2px solid #168CFF",
    borderRadius: "4px", background: "rgba(22,140,255,.16)",
    boxShadow: "0 0 0 1px rgba(7,23,34,.32)",
  });
  Object.assign(label.style, {
    all: "initial", boxSizing: "border-box",
    position: "absolute", left: "-2px", top: "-28px", maxWidth: "360px",
    height: "24px", padding: "0 8px", display: "flex", alignItems: "center",
    overflow: "hidden", whiteSpace: "nowrap", textOverflow: "ellipsis",
    borderRadius: "6px", background: "#272633", color: "#F0EDF1",
    border: "1px solid rgba(133,184,223,.55)",
    font: "600 11px -apple-system, BlinkMacSystemFont, sans-serif",
    boxShadow: "0 5px 18px rgba(0,0,0,.32)",
  });
  overlay.appendChild(label);
  Object.assign(composer.style, {
    all: "initial", position: "fixed", zIndex: "3", display: "none",
    width: "340px", minHeight: "48px", boxSizing: "border-box",
    alignItems: "center", gap: "8px", padding: "7px 7px 7px 10px",
    borderRadius: "13px", background: "#272633", color: "#F0EDF1",
    border: "1px solid rgba(202,201,238,.24)", direction: "ltr",
    pointerEvents: "auto", isolation: "isolate",
    boxShadow: "0 12px 30px -16px rgba(0,0,0,.72), 0 5px 16px rgba(0,0,0,.24)",
    font: "500 13.5px -apple-system, BlinkMacSystemFont, sans-serif",
  });
  Object.assign(input.style, {
    all: "unset", boxSizing: "border-box", display: "block",
    flex: "1", minWidth: "0", height: "34px", maxHeight: "96px", resize: "none",
    padding: "7px 8px", margin: "0", border: "0", outline: "0",
    background: "transparent", color: "#F0EDF1", caretColor: "#CAC9EE",
    font: "500 13.5px -apple-system, BlinkMacSystemFont, sans-serif",
    lineHeight: "20px", overflow: "auto", textAlign: "start",
  });
  input.dir = "auto";
  input.setAttribute("aria-label", "Preview review comment");
  input.placeholder = "Tell the active agent what to change…";
  Object.assign(send.style, {
    all: "unset", boxSizing: "border-box", display: "flex",
    flex: "none", height: "28px", padding: "0 10px", border: "0",
    alignItems: "center", justifyContent: "center",
    borderRadius: "7px", background: "#CAC9EE", color: "#303049",
    font: "650 12.5px -apple-system, BlinkMacSystemFont, sans-serif",
    lineHeight: "1", cursor: "pointer", userSelect: "none",
  });
  send.type = "button";
  send.setAttribute("aria-label", "Send review to the active agent");
  send.textContent = "Send";
  composer.append(input, send);
  shadow.append(chromeStyles, overlay, composer);

  const mount = () => {
    const root = document.documentElement || document.body;
    if (!root) return;
    if (!host.isConnected) root.appendChild(host);
  };
  const post = payload => {
    try {
      window.webkit.messageHandlers.choroPreview.postMessage(JSON.stringify(payload));
      return true;
    } catch (_) {
      return false;
    }
  };
  const clean = value => String(value || "").replace(/\s+/g, " ").trim();
  const bounded = (value, limit) => String(value || "").slice(0, limit);
  const cssEscape = value => window.CSS && CSS.escape
    ? CSS.escape(value)
    : String(value).replace(/[^a-zA-Z0-9_-]/g, character => `\\${character}`);
  const shortText = (value, limit = 240) => {
    const result = clean(value);
    return result.length > limit ? `${result.slice(0, limit - 1)}…` : result;
  };
  const stableSelector = element => {
    if (element.id) return `#${cssEscape(element.id)}`;
    for (const attribute of ["data-testid", "data-test", "data-cy"]) {
      const value = element.getAttribute(attribute);
      if (value) return `[${attribute}="${cssEscape(value)}"]`;
    }
    const name = element.getAttribute("name");
    if (name) return `${element.tagName.toLowerCase()}[name="${cssEscape(name)}"]`;
    const path = [];
    let node = element;
    while (node && node.nodeType === Node.ELEMENT_NODE && path.length < 6) {
      let part = node.tagName.toLowerCase();
      const useful = Array.from(node.classList || [])
        .filter(value => value.length < 48 && !/[0-9a-f]{8,}/i.test(value)).slice(0, 2);
      if (useful.length) part += `.${useful.map(cssEscape).join(".")}`;
      const parent = node.parentElement;
      if (parent) {
        const siblings = Array.from(parent.children).filter(child => child.tagName === node.tagName);
        if (siblings.length > 1) part += `:nth-of-type(${siblings.indexOf(node) + 1})`;
      }
      path.unshift(part);
      const candidate = path.join(" > ");
      try { if (document.querySelectorAll(candidate).length === 1) return candidate; } catch (_) {}
      node = parent;
    }
    return path.join(" > ");
  };
  const implicitRole = element => {
    const tag = element.tagName.toLowerCase();
    if (tag === "button") return "button";
    if (tag === "a" && element.hasAttribute("href")) return "link";
    if (tag === "input") return element.type === "checkbox" ? "checkbox" : element.type === "radio" ? "radio" : "textbox";
    if (tag === "select") return "combobox";
    if (tag === "textarea") return "textbox";
    if (tag === "img") return "img";
    if (/^h[1-6]$/.test(tag)) return "heading";
    return "";
  };
  const describe = element => {
    const rect = element.getBoundingClientRect();
    const styles = getComputedStyle(element);
    const text = shortText(element.innerText || element.textContent || "");
    const accessibleName = shortText(
      element.getAttribute("aria-label") || element.getAttribute("alt") ||
      element.getAttribute("title") || element.getAttribute("placeholder") || text, 120,
    );
    return {
      selector: bounded(stableSelector(element), 4096), tagName: element.tagName.toLowerCase(),
      id: bounded(element.id, 512),
      classes: Array.from(element.classList || []).slice(0, 12).map(value => bounded(value, 120)),
      text, role: bounded(element.getAttribute("role") || implicitRole(element), 120), accessibleName,
      pageURL: bounded(location.href, 4096), pageTitle: bounded(document.title, 512),
      rect: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
      styles: {
        display: bounded(styles.display, 512), color: bounded(styles.color, 512),
        backgroundColor: bounded(styles.backgroundColor, 512),
        fontFamily: bounded(styles.fontFamily, 512), fontSize: bounded(styles.fontSize, 512),
        fontWeight: bounded(styles.fontWeight, 512),
        borderRadius: bounded(styles.borderRadius, 512), padding: bounded(styles.padding, 512),
        margin: bounded(styles.margin, 512),
      },
    };
  };
  const normalizeRect = (start, end) => ({
    x: Math.max(0, Math.min(start.x, end.x)),
    y: Math.max(0, Math.min(start.y, end.y)),
    width: Math.min(innerWidth, Math.max(start.x, end.x)) - Math.max(0, Math.min(start.x, end.x)),
    height: Math.min(innerHeight, Math.max(start.y, end.y)) - Math.max(0, Math.min(start.y, end.y)),
  });
  const setOverlayRect = (rect, locked, area) => {
    mount();
    Object.assign(overlay.style, {
      display: "block", left: `${rect.x}px`, top: `${rect.y}px`,
      width: `${rect.width}px`, height: `${rect.height}px`,
      borderStyle: area ? "dashed" : "solid",
      borderColor: locked ? "#168CFF" : "#85B8DF",
      background: locked ? "rgba(22,140,255,.16)" : "rgba(133,184,223,.12)",
    });
    label.style.top = rect.y < 34 ? `${rect.height + 4}px` : "-28px";
  };
  const drawElement = (element, locked = false) => {
    if (!element || !element.isConnected) { overlay.style.display = "none"; return; }
    const info = describe(element);
    setOverlayRect(info.rect, locked, false);
    label.textContent = `${info.tagName}${info.role ? ` · ${info.role}` : ""}${info.accessibleName ? ` · ${info.accessibleName}` : ""}`;
  };
  const drawArea = (rect, locked = false) => {
    setOverlayRect(rect, locked, true);
    label.textContent = `Image area · ${Math.round(rect.width)} × ${Math.round(rect.height)}`;
  };
  const positionComposer = (rect, reset = true) => {
    mount();
    const width = Math.max(1, Math.min(340, innerWidth - 24));
    const left = Math.max(12, Math.min(innerWidth - width - 12, rect.x + rect.width / 2 - width / 2));
    let top = rect.y + rect.height - 24;
    if (top + 60 > innerHeight) top = Math.max(12, rect.y - 56);
    Object.assign(composer.style, {
      display: "flex", width: `${width}px`, left: `${left}px`, top: `${top}px`,
      borderColor: "rgba(202,201,238,.24)",
    });
    if (reset) {
      input.value = "";
      send.disabled = false;
      send.style.opacity = "1";
      setTimeout(() => input.focus(), 0);
    }
  };
  const targetAt = event => {
    const target = event.composedPath ? event.composedPath()[0] : event.target;
    if (!(target instanceof Element) || target === host || shadow.contains(target)) return null;
    return target;
  };
  const lockElement = element => {
    const info = describe(element);
    state.selectedElement = element; state.selectedArea = null;
    state.target = { kind: "element", element: info };
    drawElement(element, true); positionComposer(info.rect);
    post({ kind: "selectionReady", targetKind: "element" });
  };
  const lockArea = rect => {
    const area = {
      pageURL: bounded(location.href, 4096), pageTitle: bounded(document.title, 512), rect,
    };
    state.selectedElement = null; state.selectedArea = rect;
    state.target = { kind: "area", area };
    drawArea(rect, true); positionComposer(rect);
    post({ kind: "selectionReady", targetKind: "area" });
  };
  const submitReview = () => {
    const comment = clean(input.value);
    if (!comment || !state.target) {
      composer.style.borderColor = "#DE6E6E"; input.focus(); return;
    }
    composer.style.borderColor = "rgba(202,201,238,.24)";
    send.disabled = true; send.style.opacity = ".55";
    if (!post({
      kind: "submitReview", comment, targetKind: state.target.kind,
      element: state.target.element || null, area: state.target.area || null,
    })) {
      send.disabled = false; send.style.opacity = "1";
      composer.style.borderColor = "#DE6E6E"; input.focus();
    }
  };
  send.addEventListener("click", event => { event.preventDefault(); submitReview(); });
  input.addEventListener("input", () => {
    if (clean(input.value)) composer.style.borderColor = "rgba(202,201,238,.48)";
  });
  input.addEventListener("focus", () => {
    if (composer.style.borderColor !== "rgb(222, 110, 110)") {
      composer.style.borderColor = "rgba(202,201,238,.48)";
    }
  });
  input.addEventListener("blur", () => {
    if (composer.style.borderColor !== "rgb(222, 110, 110)") {
      composer.style.borderColor = "rgba(202,201,238,.24)";
    }
  });
  input.addEventListener("keydown", event => {
    if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); submitReview(); }
  });
  const onPointerDown = event => {
    if (!state.active || event.button !== 0) return;
    const target = targetAt(event);
    if (!target) return;
    state.pointer = { id: event.pointerId, start: { x: event.clientX, y: event.clientY }, target };
    state.dragging = false; composer.style.display = "none";
    event.preventDefault(); event.stopPropagation(); event.stopImmediatePropagation();
  };
  const onPointerMove = event => {
    if (!state.active) return;
    if (state.pointer && state.pointer.id === event.pointerId) {
      const end = { x: event.clientX, y: event.clientY };
      const dx = end.x - state.pointer.start.x, dy = end.y - state.pointer.start.y;
      if (state.dragging || Math.hypot(dx, dy) >= 7) {
        state.dragging = true; drawArea(normalizeRect(state.pointer.start, end));
      }
      event.preventDefault(); event.stopPropagation(); return;
    }
    state.hovered = targetAt(event); drawElement(state.hovered);
  };
  const onPointerUp = event => {
    if (!state.active || !state.pointer || state.pointer.id !== event.pointerId) return;
    const pointer = state.pointer;
    state.pointer = null; state.active = false; state.suppressClick = true;
    document.documentElement.style.cursor = "";
    if (state.dragging) {
      const rect = normalizeRect(pointer.start, { x: event.clientX, y: event.clientY });
      state.dragging = false;
      if (rect.width >= 4 && rect.height >= 4) lockArea(rect); else lockElement(pointer.target);
    } else lockElement(pointer.target);
    event.preventDefault(); event.stopPropagation(); event.stopImmediatePropagation();
  };
  const onClick = event => {
    if (!state.suppressClick) return;
    state.suppressClick = false;
    event.preventDefault(); event.stopPropagation(); event.stopImmediatePropagation();
  };
  const clear = () => {
    state.active = false; state.hovered = null; state.selectedElement = null;
    state.selectedArea = null; state.pointer = null; state.dragging = false; state.target = null;
    document.documentElement.style.cursor = "";
    overlay.style.display = "none"; composer.style.display = "none";
  };
  const onKey = event => {
    if (
      event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey
      && String(event.key || "").toLowerCase() === "f"
      && post({ kind: "toggleFocusMode" })
    ) {
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
      return;
    }
    if (event.key !== "Escape" || (!state.active && composer.style.display === "none")) return;
    clear(); post({ kind: "cancelled" });
    event.preventDefault(); event.stopPropagation();
  };
  const refresh = () => {
    if (state.active && state.hovered && !state.pointer) drawElement(state.hovered);
    else if (state.selectedElement) {
      const info = describe(state.selectedElement); drawElement(state.selectedElement, true); positionComposer(info.rect, false);
    } else if (state.selectedArea) { drawArea(state.selectedArea, true); positionComposer(state.selectedArea, false); }
  };
  document.addEventListener("pointerdown", onPointerDown, true);
  document.addEventListener("pointermove", onPointerMove, true);
  document.addEventListener("pointerup", onPointerUp, true);
  document.addEventListener("click", onClick, true);
  document.addEventListener("keydown", onKey, true);
  window.addEventListener("scroll", refresh, true);
  window.addEventListener("resize", refresh, true);

  window.__choroProjectPreviewInspector = {
    activate() {
      state.active = true; state.hovered = null; state.pointer = null;
      mount(); composer.style.display = "none"; document.documentElement.style.cursor = "crosshair";
    },
    deactivate() { state.active = false; state.pointer = null; document.documentElement.style.cursor = ""; refresh(); },
    clear,
    prepareSnapshot() { overlay.style.display = "none"; composer.style.display = "none"; },
    submitted() { clear(); },
    submissionFailed() { send.disabled = false; send.style.opacity = "1"; refresh(); input.focus(); },
    refresh,
  };
})();
