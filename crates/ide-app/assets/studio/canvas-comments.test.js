// Runs in the existing isolated, offscreen WebKit fixture. No live app state.
window.runCanvasCommentsTest = async ({ sleep, messages, showcase = false, progress = () => {} }) => {
  const assert = (value, label) => { if (!value) throw Error(label); };
  const reply = value => window.choroCanvasReply({ session: "fixture", ...value });
  const f = window.choroCanvasTest.flow;
  const source = f.getNode("1");
  await f.setViewport({ x: 60 - source.position.x * .4, y: 80 - source.position.y * .4, zoom: .4 });
  await sleep(350);
  let store = { schema_version: 1, revision: 0, pins: [] };
  let failNext = false, forgeNext = false;
  const realSend = window.ipc.postMessage;
  window.ipc.postMessage = raw => {
    realSend(raw);
    const m = JSON.parse(raw);
    if (m.type !== "comments-read" && m.type !== "comment-edit" && m.type !== "comment-send") return;
    if (forgeNext) {
      forgeNext = false;
      reply({ type: "comments-result", request_id: "foreign", comments: { schema_version: 1, revision: 99, pins: [] } });
      return;
    }
    let error = null;
    if (m.type === "comment-edit") {
      if (failNext) { failNext = false; error = "Fixture save conflict. Try again."; }
      else if (m.revision !== store.revision) error = "Stale comment revision";
      else {
        const op = m.operation;
        if (op.operation === "create") store.pins.push({ ...op, body: op.body.trim(), resolved: false, created_at: 1791140000 });
        if (op.operation === "resolve") store.pins.find(pin => pin.id === op.id).resolved = true;
        store.revision++;
      }
    }
    queueMicrotask(() => reply({ type: "comments-result", request_id: m.request_id, comments: structuredClone(store), error, sent: m.type === "comment-send" && !error }));
  };
  const button = text => [...document.querySelectorAll(".comments-panel button,.comment-popover button")].find(b => b.textContent === text || b.getAttribute("aria-label") === text);
  const clickScreen = () => {
    const board = f.getNode("1"), camera = f.getViewport();
    document.querySelector('.react-flow__node[data-id="1"]').dispatchEvent(new MouseEvent("click", { bubbles: true,
      clientX: camera.x + (board.position.x + board.width * .25) * camera.zoom,
      clientY: camera.y + (board.position.y + board.height * .3) * camera.zoom,
    }));
  };
  const type = text => {
    const textarea = document.querySelector(".comment-popover textarea");
    assert(textarea, "A placed pin opens its comment editor");
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set.call(textarea, text);
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const count = type => messages.filter(m => m.type === type).length;
  const contentKey = f.getNode("1").data.screen.content_key;
  const beforeReads = count("comments-read");
  assert(!document.querySelector(".comments-panel") && beforeReads === 0, "Design mode has no comment UI or storage demand");
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "c", bubbles: true }));
  assert(count("comment-toggle") === 1, "C requests the independent comment tool");
  reply({ type: "comment-mode", enabled: true }); await sleep(100);
  progress("loaded");
  assert(count("comments-read") === 1 && document.querySelector(".comments-panel"), "Comments loads on entry");
  assert(f.getNodes().filter(n => n.type === "artboard").every(n => !n.draggable && n.data.commentMode), "Comments prevents screen dragging");
  assert(f.getNode("1").data.preview?.content_key === contentKey, "Mode entry retains the authored preview content while demand and quality adapt to the narrower canvas");
  clickScreen(); await sleep(60);
  assert(document.querySelector("textarea") && button("Post comment").disabled, "Click places an empty draft with posting disabled");
  assert(document.querySelector('.comment-popover[role="dialog"]') && !document.querySelector('.comments-panel textarea') && !document.querySelector('.comments-panel .comment-actions'), "Writing happens beside the pin; the sidebar only lists notes");
  assert(getComputedStyle(document.querySelector('.artboard')).cursor.includes('data:image/svg+xml'), "Comment mode uses a comment cursor");
  const withinStage = () => {
    const pop = document.querySelector('.comment-popover').getBoundingClientRect(), stage = document.querySelector('.canvas-viewport').getBoundingClientRect();
    assert(pop.left >= stage.left && pop.right <= stage.right + 1 && pop.top >= stage.top && pop.bottom <= stage.bottom + 1, 'The anchored popover stays inside the uncovered stage');
  };
  withinStage();
  type("Make this action easier to find."); await sleep(50);
  assert(!button("Post comment").disabled && !button("Collapse comments").disabled, "Typed drafts can post and the sidebar can collapse");
  const expandedWidth = document.querySelector(".react-flow").getBoundingClientRect().width;
  button("Collapse comments").click(); await sleep(50);
  assert(document.querySelector("textarea").value === "Make this action easier to find." && document.querySelector(".comments-draft-indicator"), "Collapsing the index keeps the draft open on the canvas");
  assert(document.querySelector(".react-flow").getBoundingClientRect().width > expandedWidth + 100, "Collapsing frees canvas space");
  button("Expand comments").click(); await sleep(50);
  assert(document.querySelector("textarea").value === "Make this action easier to find.", "Expanding the index preserves the draft text");
  document.querySelector("textarea").dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); await sleep(40);
  const beforeOpen = count("open"), beforeResize = count("resize"), beforeMove = count("move-screen");
  const node = document.querySelector('.react-flow__node[data-id="1"]');
  node.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, clientX: 220, clientY: 250 }));
  node.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 220, clientY: 250 }));
  await sleep(50);
  assert(count("open") === beforeOpen && count("resize") === beforeResize && count("move-screen") === beforeMove && !document.querySelector(".canvas-menu"), "Comments does not open editors or design menus");
  failNext = true; button("Post comment").click(); await sleep(70);
  assert(document.querySelector("textarea").value === "Make this action easier to find." && document.querySelector('[role="alert"]'), "Failed saves preserve the draft and show recovery");
  button("Post comment").click(); await sleep(70);
  assert(store.pins.length === 1 && document.querySelectorAll(".comment-pin:not(.draft)").length === 1, "Posting saves one pin");
  assert(!document.querySelector("textarea") && button("Resolve") && !messages.filter(m => m.type === "comment-draft").at(-1).dirty, "Successful posting opens the saved note and clears the dirty guard");
  assert(document.activeElement === button('Close comment'), 'Opening a saved note moves keyboard focus into its popover');
  button('Close comment').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); await sleep(50);
  assert(!document.querySelector('.comment-popover') && document.activeElement === document.querySelector('.comment-pin'), 'Escape closes the saved note and returns focus to its pin');
  document.querySelector('.comment-pin').click(); await sleep(50);
  button("Send to agent").click(); await sleep(70);
  const handoff = messages.filter(m => m.type === "comment-send").at(-1);
  assert(handoff.id === store.pins[0].id && handoff.revision === store.revision && !handoff.body && button("Sent to agent").disabled && !store.pins[0].resolved, "Handoff acknowledges the saved pin without resolving or duplicating it");
  progress("posted");
  const marker = document.querySelector(".comment-pin");
  const oldWidth = marker.getBoundingClientRect().width;
  await f.setViewport({ x: 70 - source.position.x * .7, y: 100 - source.position.y * .7, zoom: .7 }); await sleep(100);
  const saved = store.pins[0], board = f.getNode("1"), camera = f.getViewport();
  assert(Math.abs(parseFloat(marker.style.left) - (camera.x + (board.position.x + saved.x * board.width) * camera.zoom)) < .1, "Pin tracks the canvas camera");
  assert(Math.abs(marker.getBoundingClientRect().width - oldWidth) < .1, "Pin remains readable at different zooms");
  button("Collapse comments").click(); await sleep(50);
  assert(document.querySelector(".comment-pin") && button("Resolve") && button("Expand comments"), "Collapsing hides the list while keeping the selected canvas note");
  const beforePinClick = f.getViewport();
  document.querySelector(".comment-pin").click(); await sleep(70);
  assert(button("Expand comments") && button("Resolve") && f.getViewport().x === beforePinClick.x && f.getViewport().y === beforePinClick.y, "A pin opens its popover in place without expanding the list or moving the camera");
  button("Resolve").click(); await sleep(70);
  assert(store.pins[0].resolved && !document.querySelector(".comment-pin") && !document.querySelector("textarea"), "Resolve removes the pin from open comments");
  clickScreen(); await sleep(50); type("Temporary draft"); await sleep(50); button("Cancel").click(); await sleep(50);
  assert(store.pins.length === 1 && !document.querySelector("textarea"), "Cancelling a draft does not save another pin");
  reply({ type: "comment-mode", enabled: false }); await sleep(70);
  assert(!document.querySelector(".comments-panel") && !document.querySelector(".comment-pins") && !f.getNode("1").data.commentMode, "Returning to Design unmounts the comment layer");
  const requestsAfterExit = count("comments-read") + count("comment-edit");
  reply({ type: "comments-result", request_id: "late", comments: store });
  await f.setViewport({ x: 100 - source.position.x * .4, y: 90 - source.position.y * .4, zoom: .4 }); await sleep(100);
  assert(count("comments-read") + count("comment-edit") === requestsAfterExit, "Inactive comments do not poll or react to stale replies");
  reply({ type: "comment-mode", enabled: true }); await sleep(70);
  assert(!document.querySelector(".comment-pin"), "Resolved comments stay resolved when re-entering");
  // A reply from another request must not replace state or unlock the draft.
  clickScreen(); await sleep(50); type("Check the button contrast."); await sleep(50);
  forgeNext = true; button("Post comment").click(); await sleep(50);
  assert(button("Post comment").disabled && document.querySelector("textarea").value === "Check the button contrast.", "Foreign replies cannot acknowledge a save");
  const pending = messages.filter(m => m.type === "comment-edit").at(-1);
  store.pins.push({ ...pending.operation, resolved: false, created_at: 18446744073709551615 }); store.revision++;
  reply({ type: "comments-result", request_id: pending.request_id, comments: structuredClone(store) }); await sleep(60);
  assert(document.querySelector(".comments-panel").textContent.includes("Date unavailable") && button("Resolve"), "Malformed timestamps preserve the note and resolve action");
  button("Resolve").click(); await sleep(60);
  assert(store.pins.at(-1).resolved && !document.querySelector(".comment-pin"), "Comments with malformed timestamps can still be resolved");
  progress("recovery-checked");
  // Multiple screen locations stay in one persistent list when navigating.
  store.pins.push(
    { id: "first-list-note", screen_id: "1", x: .3, y: .3, body: "Make the primary action easier to find.", resolved: false, created_at: 1791140000 },
    { id: "far-list-note", screen_id: "8", x: .6, y: .4, body: "Check this screen's navigation.", resolved: false, created_at: 1791140000 },
    ...Array.from({ length: 10 }, (_, i) => ({ id: `extra-note-${i}`, screen_id: "1", x: .1 + i * .04, y: .7,
      body: `Follow-up ${i + 1}: verify this long note remains available while another comment is selected.`, resolved: false, created_at: 1791140000 })),
  );
  store.revision++;
  reply({ type: "comment-mode", enabled: false }); await sleep(40);
  reply({ type: "comment-mode", enabled: true }); await sleep(100);
  const sidebar = document.querySelector(".comments-panel"), canvas = document.querySelector(".react-flow");
  assert(document.querySelectorAll(".comment-row").length === 12, "Sidebar lists every open note");
  assert(!document.querySelector('.comment-tool'), 'Only the host bottom toolbar owns the Comments control');
  const canvasBounds = canvas.getBoundingClientRect(), sidebarBounds = sidebar.getBoundingClientRect();
  assert(Math.abs(sidebarBounds.right - innerWidth) < 1 && sidebarBounds.top === 0 && Math.abs(sidebarBounds.height - innerHeight) < 1, "Sidebar is docked at full height");
  assert(Math.abs(canvasBounds.right - sidebarBounds.left) < 1, "Sidebar reserves space instead of covering the canvas");
  document.querySelectorAll(".comment-row")[1].click(); await sleep(100);
  assert(document.querySelector('.comment-popover .comment-body').textContent.includes("Check this screen") && !document.querySelector('.comments-panel .comment-actions'), 'Selecting a list row opens its note on the design');
  withinStage();
  assert(document.querySelectorAll(".comment-row").length === 12 && document.querySelector('.comment-list-item.selected').textContent.includes("Check this screen"), "Selecting a comment preserves the full list and highlights its row");
  const at = f.getViewport(), target = f.getNode("8");
  const px = at.x + (target.position.x + .6 * target.width) * at.zoom;
  const py = at.y + (target.position.y + .4 * target.height) * at.zoom;
  assert(Math.abs(px - canvas.clientWidth / 2) < 1 && Math.abs(py - canvas.clientHeight / 2) < 1, "Selecting a distant note centers its pin in the uncovered canvas");
  await f.setViewport({ ...at, x: at.x + 200 }); await sleep(50);
  document.querySelectorAll(".comment-row")[1].click(); await sleep(70);
  assert(Math.abs(f.getViewport().x - at.x) < 1, "Clicking an already-selected note recenters it after panning");
  assert(document.querySelector(".comments-content").scrollHeight > document.querySelector(".comments-content").clientHeight, "Long lists scroll independently");
  const beforeCollapseReads = count("comments-read");
  button("Collapse comments").click(); await sleep(50);
  document.querySelector('.comment-pin[aria-pressed="true"]').click(); await sleep(70);
  assert(button('Expand comments') && document.querySelector('.comment-popover') && count("comments-read") === beforeCollapseReads, "Opening a pin preserves the collapsed index without fetching again");
  button('Expand comments').click(); await sleep(50);
  assert(document.querySelectorAll('.comment-row').length === 12, 'The complete index survives collapse and pin selection');
  // Focus has one visible screen; index navigation must reveal the target
  // before centering its pin, rather than leaving an invisible selection.
  const boot = { ...window.__CHORO_CANVAS__, revision: 1000, fingerprint: 'focus-comment-fixture' };
  reply({ ...boot, type: 'state', layout: { ...boot.layout, overview_mode: 'focus', selected_screen_id: '1' } });
  await sleep(100);
  assert(f.getNode('8').hidden, 'Focus initially hides the distant screen');
  const beforeFocusSelection = count('select');
  document.querySelectorAll('.comment-row')[1].click(); await sleep(60);
  assert(count('select') === beforeFocusSelection + 1 && messages.filter(m => m.type === 'select').at(-1).screen_id === '8', 'Selecting a hidden note requests its screen once');
  reply({ type: 'selection', screen_id: '8', section_id: null }); await sleep(100);
  assert(!f.getNode('8').hidden && document.querySelector('.comment-popover .comment-body').textContent.includes('Check this screen'), 'Focus reveals the target and opens its pinned note on the design');
  withinStage();
  reply({ ...boot, type: 'state', layout: { ...boot.layout, overview_mode: 'canvas' } }); await sleep(70);
  progress("navigation-checked");
  if (showcase && document.querySelector(".comment-pin")) {
    await f.setViewport({ x: 60 - source.position.x * .65, y: 100 - source.position.y * .65, zoom: .65 });
    await sleep(150);
    document.querySelector(".comment-pin").click(); await sleep(50);
  } else { reply({ type: "comment-mode", enabled: false }); await sleep(50); }
  window.ipc.postMessage = realSend;
};
