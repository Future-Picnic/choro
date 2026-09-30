(() => {
  if (window.__choroProjectPreviewAgent) return;

  const MAX_ELEMENTS = 250;
  const state = {
    refs: new Map(),
    point: { x: 28, y: 28 },
    visible: false,
    running: false,
    commandId: null,
    epoch: 0,
    nativePending: null,
    snapshotSequence: 0,
    snapshotToken: null,
    snapshotViewport: null,
    pendingNavigation: null,
  };

  const host = document.createElement("div");
  const shadow = host.attachShadow({ mode: "closed" });
  const cursor = document.createElement("div");
  const arrow = document.createElement("div");
  const badge = document.createElement("div");
  const status = document.createElement("div");
  host.dataset.choroPreviewAgent = "true";
  Object.assign(host.style, {
    all: "initial",
    position: "fixed",
    inset: "0",
    zIndex: "2147483646",
    pointerEvents: "none",
    direction: "ltr",
    colorScheme: "dark",
  });
  Object.assign(cursor.style, {
    all: "initial",
    position: "fixed",
    left: "0",
    top: "0",
    width: "1px",
    height: "1px",
    display: "none",
    pointerEvents: "none",
    transform: "translate3d(28px, 28px, 0)",
    transition: "transform 220ms cubic-bezier(.2,.8,.2,1)",
    filter: "drop-shadow(0 2px 3px rgba(0,0,0,.42))",
    willChange: "transform",
  });
  Object.assign(arrow.style, {
    all: "initial",
    position: "absolute",
    left: "-2px",
    top: "-2px",
    width: "18px",
    height: "22px",
    background: "#CAC9EE",
    clipPath: "polygon(0 0, 0 100%, 5px 15px, 10px 22px, 14px 19px, 9px 12px, 18px 11px)",
  });
  Object.assign(badge.style, {
    all: "initial",
    position: "absolute",
    left: "15px",
    top: "17px",
    height: "22px",
    padding: "0 7px",
    display: "flex",
    alignItems: "center",
    borderRadius: "6px",
    background: "#CAC9EE",
    color: "#303049",
    border: "1px solid rgba(255,255,255,.34)",
    boxShadow: "0 5px 14px rgba(0,0,0,.24)",
    font: "650 11px -apple-system, BlinkMacSystemFont, sans-serif",
    whiteSpace: "nowrap",
  });
  Object.assign(status.style, {
    all: "initial",
    position: "absolute",
    left: "15px",
    top: "41px",
    maxWidth: "220px",
    padding: "4px 7px",
    display: "none",
    borderRadius: "6px",
    background: "#272633",
    color: "#F0EDF1",
    border: "1px solid rgba(202,201,238,.24)",
    boxShadow: "0 5px 14px rgba(0,0,0,.24)",
    font: "500 10.5px -apple-system, BlinkMacSystemFont, sans-serif",
    whiteSpace: "nowrap",
  });
  badge.textContent = "Agent";
  cursor.append(arrow, badge, status);
  shadow.append(cursor);

  const mount = () => {
    const root = document.documentElement || document.body;
    if (root && !host.isConnected) root.appendChild(host);
  };
  const post = payload => new Promise((resolve, reject) => {
    const body = JSON.stringify(payload);
    let attempts = 0;
    const send = () => {
      try {
        const handler = window.webkit?.messageHandlers?.choroPreview;
        if (!handler) throw new Error("Preview message bridge is not ready");
        handler.postMessage(body);
        resolve();
      } catch (error) {
        attempts += 1;
        if (attempts < 100) {
          setTimeout(send, 50);
        } else {
          reject(error);
        }
      }
    };
    send();
  });
  const clean = (value, limit = 240) => {
    const text = String(value || "").replace(/\s+/g, " ").trim();
    return text.length > limit ? `${text.slice(0, limit - 1)}…` : text;
  };
  const loopbackHost = hostname => {
    const host = String(hostname || "").toLowerCase().replace(/^\[|\]$/g, "");
    if (host === "localhost" || host === "::1") return true;
    const parts = host.split(".");
    return parts.length === 4 &&
      parts.every(part => /^\d{1,3}$/.test(part) && Number(part) <= 255) &&
      Number(parts[0]) === 127;
  };
  const ensureAllowedLocation = policy => {
    const current = new URL(location.href);
    if (
      policy?.allowLoopback === true &&
      (current.protocol === "http:" || current.protocol === "https:") &&
      loopbackHost(current.hostname)
    ) {
      return;
    }
    if (current.protocol === "file:" && typeof policy?.fileRoot === "string") {
      const root = new URL(policy.fileRoot);
      if (root.protocol === "file:" && current.href.startsWith(root.href)) return;
    }
    throw new Error("Preview control stopped because the page left its approved local source");
  };
  const visibleRect = element => {
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    if (
      rect.width < 2 ||
      rect.height < 2 ||
      rect.bottom <= 0 ||
      rect.right <= 0 ||
      rect.top >= innerHeight ||
      rect.left >= innerWidth ||
      style.display === "none" ||
      style.visibility === "hidden" ||
      Number(style.opacity) === 0
    ) return null;
    return rect;
  };
  const implicitRole = element => {
    const tag = element.tagName.toLowerCase();
    if (tag === "button") return "button";
    if (tag === "a" && element.hasAttribute("href")) return "link";
    if (tag === "textarea") return "textbox";
    if (tag === "select") return "combobox";
    if (tag === "input") {
      if (element.type === "checkbox") return "checkbox";
      if (element.type === "radio") return "radio";
      if (element.type === "submit" || element.type === "button") return "button";
      return "textbox";
    }
    return "";
  };
  const nameFor = element => clean(
    element.getAttribute("aria-label") ||
      element.getAttribute("alt") ||
      element.getAttribute("title") ||
      element.getAttribute("placeholder") ||
      element.innerText ||
      element.textContent,
    160,
  );
  const observe = () => {
    state.refs.clear();
    state.snapshotSequence += 1;
    state.snapshotToken = `s${state.snapshotSequence}-${Date.now().toString(36)}`;
    state.snapshotViewport = {
      width: innerWidth,
      height: innerHeight,
      scrollX,
      scrollY,
    };
    const selector = [
      "button",
      "a[href]",
      "input",
      "textarea",
      "select",
      "[role]",
      "[contenteditable='true']",
      "[tabindex]:not([tabindex='-1'])",
    ].join(",");
    const elements = [];
    for (const element of document.querySelectorAll(selector)) {
      if (!(element instanceof HTMLElement) || element.closest("[data-choro-preview-agent]")) continue;
      const rect = visibleRect(element);
      if (!rect) continue;
      const ref = `${state.snapshotToken}:e${elements.length + 1}`;
      state.refs.set(ref, element);
      elements.push({
        ref,
        tag: element.tagName.toLowerCase(),
        role: clean(element.getAttribute("role") || implicitRole(element), 80),
        name: nameFor(element),
        text: clean(element.innerText || element.textContent, 160),
        disabled: Boolean(element.disabled || element.getAttribute("aria-disabled") === "true"),
        rect: {
          x: Math.round(rect.x),
          y: Math.round(rect.y),
          width: Math.round(rect.width),
          height: Math.round(rect.height),
        },
      });
      if (elements.length >= MAX_ELEMENTS) break;
    }
    return {
      url: String(location.href).slice(0, 4096),
      title: clean(document.title, 512),
      snapshot: state.snapshotToken,
      viewport: { width: innerWidth, height: innerHeight },
      scroll: { x: scrollX, y: scrollY },
      elements,
      truncated: elements.length >= MAX_ELEMENTS,
    };
  };
  const show = message => {
    mount();
    state.visible = true;
    cursor.style.display = "block";
    status.textContent = message || "";
    status.style.display = message ? "block" : "none";
  };
  const hide = () => {
    state.epoch += 1;
    state.visible = false;
    state.running = false;
    state.commandId = null;
    state.pendingNavigation = null;
    if (state.nativePending) {
      state.nativePending.reject(new Error("Preview control was interrupted by the user"));
      state.nativePending = null;
    }
    cursor.style.display = "none";
    status.style.display = "none";
  };
  const frame = () => new Promise(resolve => requestAnimationFrame(() => resolve()));
  const delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
  const ensureActive = token => {
    if (state.epoch !== token) throw new Error("Preview control was interrupted by the user");
  };
  const moveTo = async (x, y, message, token) => {
    show(message);
    const safeX = Math.max(0, Math.min(innerWidth - 20, Number(x) || 0));
    const safeY = Math.max(0, Math.min(innerHeight - 42, Number(y) || 0));
    await frame();
    cursor.style.transform = `translate3d(${safeX}px, ${safeY}px, 0)`;
    state.point = { x: safeX, y: safeY };
    await delay(230);
    ensureActive(token);
  };
  const ripple = (x, y) => {
    const ring = document.createElement("div");
    Object.assign(ring.style, {
      all: "initial",
      position: "fixed",
      left: `${x - 11}px`,
      top: `${y - 11}px`,
      width: "22px",
      height: "22px",
      borderRadius: "999px",
      border: "2px solid #CAC9EE",
      boxSizing: "border-box",
      opacity: "1",
      transform: "scale(.45)",
      transition: "transform 260ms ease-out, opacity 260ms ease-out",
    });
    shadow.append(ring);
    requestAnimationFrame(() => {
      ring.style.transform = "scale(1.6)";
      ring.style.opacity = "0";
    });
    setTimeout(() => ring.remove(), 300);
  };
  const requestNativeInput = (command, input, expectedTarget = null) => new Promise((resolve, reject) => {
    if (state.nativePending) {
      reject(new Error("Another native Preview input is still running"));
      return;
    }
    state.nativePending = {
      commandId: command.id,
      action: input.action,
      expectedTarget,
      delivered: false,
      resolve,
      reject,
    };
    post({
      kind: "agentNativeInputRequest",
      commandId: command.id,
      ...input,
    }).catch(error => {
      if (state.nativePending?.commandId === command.id) {
        state.nativePending = null;
        reject(error);
      }
    });
  });
  const targetFor = payload => {
    if (payload && typeof payload.ref === "string") {
      const element = state.refs.get(payload.ref);
      if (element && element.isConnected) return element;
      throw new Error(`Element ${payload.ref} is stale; take another snapshot`);
    }
    if (Number.isFinite(payload?.x) && Number.isFinite(payload?.y)) {
      const viewport = state.snapshotViewport;
      if (
        typeof payload.snapshot !== "string" ||
        payload.snapshot !== state.snapshotToken ||
        !viewport ||
        viewport.width !== innerWidth ||
        viewport.height !== innerHeight ||
        viewport.scrollX !== scrollX ||
        viewport.scrollY !== scrollY
      ) {
        throw new Error("Coordinates are stale; take another snapshot and include its snapshot token");
      }
      const element = document.elementFromPoint(payload.x, payload.y);
      if (element instanceof HTMLElement && !host.contains(element)) return element;
    }
    throw new Error("Provide an element ref or visible x/y coordinates");
  };
  const centerOf = element => {
    const rect = visibleRect(element);
    if (!rect) throw new Error("Target is not visible");
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  };
  const bringIntoView = async (element, token) => {
    element.scrollIntoView({ block: "center", inline: "center", behavior: "smooth" });
    await delay(260);
    ensureActive(token);
    return centerOf(element);
  };
  const setNativeValue = (element, value) => {
    const prototype = element instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(prototype, "value")?.set;
    if (setter) setter.call(element, value);
    else element.value = value;
  };
  const executeAction = async (command, token, policy) => {
    ensureAllowedLocation(policy);
    const payload = command.payload && typeof command.payload === "object" ? command.payload : {};
    switch (command.action) {
      case "snapshot":
        show("Observing Preview…");
        return { result: observe(), capture: true };
      case "click": {
        const element = targetFor(payload);
        const point = await bringIntoView(element, token);
        await moveTo(point.x, point.y, `Clicking ${nameFor(element) || element.tagName.toLowerCase()}`, token);
        ensureActive(token);
        ripple(point.x, point.y);
        const submitsForm = (
          element instanceof HTMLButtonElement ||
          element instanceof HTMLInputElement
        ) && String(element.type || "").toLowerCase() === "submit";
        if (element instanceof HTMLAnchorElement || submitsForm) {
          // Navigation can destroy this isolated world synchronously. The
          // outer executor activates this only after the host acknowledges
          // receipt of the completion message.
          return {
            result: {
              ok: true,
              url: location.href,
              target: nameFor(element),
              input: "dom-navigation",
            },
            capture: false,
            navigationElement: element,
          };
        }
        const native = await requestNativeInput(
          command,
          { action: "click", x: point.x, y: point.y },
          element,
        );
        if (!native.delivered) {
          element.focus({ preventScroll: true });
          element.click();
        }
        await delay(120);
        return {
          result: {
            ok: true,
            url: location.href,
            target: nameFor(element),
            input: native.delivered ? "native" : "dom-fallback",
          },
          capture: false,
        };
      }
      case "type": {
        const element = targetFor(payload);
        const point = await bringIntoView(element, token);
        await moveTo(point.x, point.y, `Typing in ${nameFor(element) || "field"}`, token);
        ensureActive(token);
        const native = await requestNativeInput(
          command,
          { action: "click", x: point.x, y: point.y },
          element,
        );
        if (!native.delivered) element.focus({ preventScroll: true });
        const text = String(payload.text || "").slice(0, 20000);
        if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
          const next = payload.clear === false ? `${element.value}${text}` : text;
          setNativeValue(element, next);
          element.dispatchEvent(new InputEvent("input", {
            bubbles: true,
            inputType: "insertText",
            data: text,
          }));
          element.dispatchEvent(new Event("change", { bubbles: true }));
        } else if (element.isContentEditable) {
          if (payload.clear !== false) element.textContent = "";
          element.textContent = `${element.textContent || ""}${text}`;
          element.dispatchEvent(new InputEvent("input", {
            bubbles: true,
            inputType: "insertText",
            data: text,
          }));
        } else {
          throw new Error("Target is not an editable field");
        }
        await delay(100);
        return {
          result: {
            ok: true,
            url: location.href,
            target: nameFor(element),
            focus: native.delivered ? "native" : "dom-fallback",
          },
          capture: false,
        };
      }
      case "scroll": {
        show("Scrolling Preview…");
        const x = Math.max(-5000, Math.min(5000, Number(payload.x) || 0));
        const y = Math.max(-5000, Math.min(5000, Number(payload.y) || 0));
        window.scrollBy({ left: x, top: y, behavior: "smooth" });
        await delay(320);
        ensureActive(token);
        return { result: { ok: true, scroll: { x: scrollX, y: scrollY } }, capture: false };
      }
      case "key": {
        const key = clean(payload.key, 40);
        if (!key) throw new Error("Provide a key");
        show(`Pressing ${key}…`);
        const target = document.activeElement || document.body;
        const native = await requestNativeInput(command, {
          action: "key",
          key,
          code: clean(payload.code || key, 40),
          meta: Boolean(payload.meta),
          control: Boolean(payload.control),
          alt: Boolean(payload.alt),
          shift: Boolean(payload.shift),
        });
        if (!native.delivered) {
          for (const type of ["keydown", "keyup"]) {
            target.dispatchEvent(new KeyboardEvent(type, {
              key,
              code: clean(payload.code || key, 40),
              bubbles: true,
              cancelable: true,
              metaKey: Boolean(payload.meta),
              ctrlKey: Boolean(payload.control),
              altKey: Boolean(payload.alt),
              shiftKey: Boolean(payload.shift),
            }));
          }
          if (key === "Enter" && target instanceof HTMLElement) target.click();
        }
        await delay(80);
        ensureActive(token);
        return {
          result: { ok: true, key, input: native.delivered ? "native" : "dom-fallback" },
          capture: false,
        };
      }
      case "wait": {
        const milliseconds = Math.max(0, Math.min(5000, Number(payload.milliseconds) || 500));
        show(`Waiting ${milliseconds} ms…`);
        await delay(milliseconds);
        ensureActive(token);
        return { result: { ok: true, milliseconds, url: location.href }, capture: false };
      }
      case "stop":
        hide();
        state.refs.clear();
        return { result: { ok: true, stopped: true }, capture: false };
      default:
        throw new Error(`Unsupported Preview action: ${command.action}`);
    }
  };

  window.__choroProjectPreviewAgent = {
    async execute(command, policy) {
      if (!command || typeof command.id !== "string") return;
      if (state.running) {
        if (state.commandId === command.id) return;
        hide();
      }
      const token = ++state.epoch;
      state.running = true;
      state.commandId = command.id;
      try {
        const completed = await executeAction(command, token, policy);
        if (completed.navigationElement) {
          state.pendingNavigation = {
            commandId: command.id,
            element: completed.navigationElement,
          };
          await post({
            kind: "agentNavigationReady",
            commandId: command.id,
            result: completed.result,
          });
          return;
        }
        await post({
          kind: "agentActionResult",
          commandId: command.id,
          success: true,
          capture: completed.capture,
          result: completed.result,
        });
      } catch (error) {
        try {
          await post({
            kind: "agentActionResult",
            commandId: command.id,
            success: false,
            capture: false,
            error: clean(error?.message || error, 1000),
          });
        } catch (_) {}
      } finally {
        if (
          state.commandId === command.id &&
          (!state.pendingNavigation || state.pendingNavigation.commandId !== command.id)
        ) {
          state.running = false;
          state.commandId = null;
          if (command.action !== "stop") status.style.display = "none";
        }
      }
    },
    activateNavigation(commandId) {
      const pending = state.pendingNavigation;
      if (!pending || pending.commandId !== commandId) {
        throw new Error("Preview navigation is no longer pending");
      }
      state.pendingNavigation = null;
      state.running = false;
      state.commandId = null;
      status.style.display = "none";
      if (!pending.element.isConnected) {
        throw new Error("Preview navigation target became stale");
      }
      let settlesInCurrentDocument = false;
      if (pending.element instanceof HTMLAnchorElement) {
        const target = String(pending.element.target || "").toLowerCase();
        const destination = new URL(pending.element.href, location.href);
        const current = new URL(location.href);
        settlesInCurrentDocument = (
          (target && target !== "_self") ||
          (
            destination.origin === current.origin &&
            destination.pathname === current.pathname &&
            destination.search === current.search
          )
        );
      }
      pending.element.click();
      if (settlesInCurrentDocument) {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            post({
              kind: "agentNavigationSettled",
              commandId,
              url: String(location.href).slice(0, 4096),
            }).catch(() => {});
          });
        });
      }
    },
    cancel(commandId, reason) {
      if (state.commandId && commandId && state.commandId !== commandId) return;
      const message = clean(reason || "Preview control was cancelled", 1000);
      state.epoch += 1;
      state.running = false;
      state.commandId = null;
      state.pendingNavigation = null;
      if (state.nativePending) {
        state.nativePending.reject(new Error(message));
        state.nativePending = null;
      }
      cursor.style.display = "none";
      status.style.display = "none";
    },
    nativeCompleted(commandId, success, error) {
      const pending = state.nativePending;
      if (!pending || pending.commandId !== commandId) return;
      state.nativePending = null;
      if (success) pending.resolve({ delivered: pending.delivered });
      else pending.reject(new Error(clean(error || "Native Preview input failed", 1000)));
    },
    hide,
  };

  document.addEventListener("pointerdown", event => {
    // AppKit-delivered agent clicks are trusted too. While one exact native
    // request is pending, it belongs to this command rather than the user.
    if (state.nativePending && state.nativePending.action === "click" && event.isTrusted) {
      return;
    }
    if (state.running && event.isTrusted) hide();
  }, true);
  document.addEventListener("click", event => {
    if (!state.nativePending || state.nativePending.action !== "click" || !event.isTrusted) return;
    const expected = state.nativePending.expectedTarget;
    const target = event.target;
    state.nativePending.delivered = !expected || (
      target instanceof Node &&
      (target === expected || expected.contains(target) || target.contains(expected))
    );
  }, true);
  document.addEventListener("keydown", event => {
    if (state.nativePending && state.nativePending.action === "key" && event.isTrusted) {
      state.nativePending.delivered = true;
    }
  }, true);
})();
