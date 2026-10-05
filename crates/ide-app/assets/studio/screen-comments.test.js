// Isolated screen/player fixture. Agent handoffs are mocked, never live turns.
window.runScreenCommentsTest = async ({ wait, assert, messages, originalSend }) => {
  const boot = window.__CHORO_STUDIO__, frame = document.getElementById('screen');
  const button = label => [...document.querySelectorAll('.screen-comments-root button,.comment-popover button')].find(b => b.textContent === label || b.getAttribute('aria-label') === label);
  const otherScreen = '20000000-0000-4000-8000-000000000001';
  boot.screens = [...(boot.screens ?? []), { id: boot.screen_id, name: 'Current screen', width: boot.width, height: boot.height, archived: false }, { id: otherScreen, name: 'Another screen', width: boot.width, height: boot.height, archived: false }];
  let enabled = false, dirty = false, failSend = true, failFocus = true, focusRequest = null;
  let comments = { schema_version: 1, revision: 0, pins: [] };
  const real = window.ipc.postMessage;
  window.ipc.postMessage = raw => {
    real(raw);
    const m = JSON.parse(raw);
    if (m.type === 'comment-toggle') {
      if (dirty) return;
      enabled = !enabled;
      window.choroStudioReply({ session: boot.session, type: 'comment-mode', enabled });
    }
    if (m.type === 'comment-draft') dirty = m.dirty;
    if (m.type === 'comment-focus') {
      assert(m.id && !m.body && !m.screen_id, 'Cross-screen focus sends only the saved comment identity');
      focusRequest = m;
      if (failFocus) {
        failFocus = false;
        queueMicrotask(() => window.choroStudioReply({ session: boot.session, type: 'comments-result', request_id: m.request_id, error: 'Fixture screen unavailable. Retry.' }));
      }
    }
    if (m.type === 'comments-read' || m.type === 'comment-edit' || m.type === 'comment-send') {
      let error = null, sent = false;
      if (m.type === 'comment-edit') {
        if (m.operation.operation === 'create') comments.pins.push({ ...m.operation, resolved: false, created_at: 1791140000 });
        else comments.pins.find(pin => pin.id === m.operation.id).resolved = true;
        comments.revision++;
      }
      if (m.type === 'comment-send') {
        assert(m.id === comments.pins[0].id && m.revision === comments.revision && !m.body && !m.screen_id, 'Handoff sends only the saved pin identity and revision');
        if (failSend) { failSend = false; error = 'Fixture assistant unavailable. Retry.'; } else sent = true;
      }
      queueMicrotask(() => window.choroStudioReply({ session: boot.session, type: 'comments-result', request_id: m.request_id, comments: structuredClone(comments), error, sent }));
    }
  };
  const count = type => messages.filter(m => m.type === type).length;
  assert(!window.choroCommentsLoaded && !document.querySelector('.comments-panel'), 'Inactive screen comments do not evaluate their React bundle');
  const originalRect = frame.getBoundingClientRect();
  const input = document.createElement('input'); document.body.append(input);
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', bubbles: true }));
  assert(count('comment-toggle') === 0, 'Typing C in a field does not activate comments'); input.remove();
  if (boot.prototype) frame.contentWindow.postMessage('test-comment-key', '*');
  else { await wait(() => frame.contentDocument?.readyState === 'complete', 'screen loaded'); frame.contentDocument.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', bubbles: true })); }
  await wait(() => document.querySelector('.comments-panel')?.textContent.includes('No open comments.'), 'comments from authored frame shortcut');
  assert(frame === document.getElementById('screen') && frame.getBoundingClientRect().width === originalRect.width, 'Entering Comments preserves the live iframe and camera');
  const cover = document.querySelector('.screen-comment-capture'), r = cover.getBoundingClientRect();
  cover.dispatchEvent(new MouseEvent('click', { bubbles: true, clientX: r.left + r.width * .3, clientY: r.top + r.height * .3 }));
  await wait(() => document.querySelector('.comment-popover textarea'), 'draft input');
  assert(!document.querySelector('.comments-panel textarea') && getComputedStyle(cover).cursor.includes('data:image/svg+xml'), 'Comments compose on the screen with a comment cursor');
  const textarea = document.querySelector('.comment-popover textarea');
  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(textarea, 'Fix the action on this screen.');
  textarea.dispatchEvent(new Event('input', { bubbles: true }));
  await wait(() => dirty && !button('Post comment').disabled, 'draft protected');
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', bubbles: true }));
  assert(enabled, 'An unposted draft prevents leaving the comment tool');
  button('Post comment').click();
  await wait(() => button('Send to agent') && !dirty, 'posted note');
  assert(comments.pins[0].screen_id === boot.screen_id && Math.abs(comments.pins[0].x - .3) < .004, 'Pin uses the active screen and local fractions');
  button('Send to agent').click(); await wait(() => document.querySelector('.comment-popover [role="alert"]'), 'failed handoff');
  assert(!button('Send to agent').disabled && !comments.pins[0].resolved, 'A failed handoff stays retryable and keeps the note open');
  button('Send to agent').click(); await wait(() => button('Sent to agent') && !button('Resolve').disabled && document.querySelector('.comment-popover [role="status"]')?.textContent.includes('Sent to the design assistant'), 'successful handoff');
  assert(!document.querySelector('.comments-panel .comment-actions') && document.querySelector('.comment-popover .comment-body'), 'The index has no composer or actions; the pinned note owns them');
  const bounds = document.querySelector('.comment-popover').getBoundingClientRect(), sidebar = document.querySelector('.comments-panel').getBoundingClientRect();
  assert(bounds.left >= 0 && bounds.top >= 0 && bounds.right <= sidebar.left + 1 && bounds.bottom <= innerHeight, 'The popover avoids the sidebar and viewport edges');
  assert(button('Sent to agent').disabled && !comments.pins[0].resolved && count('comment-send') === 2, 'Accepted handoff is acknowledged once without resolving the note');
  if (boot.testCommentsShowcase) { originalSend(JSON.stringify({ type: 'thumbnail-ready' })); return; }
  if (boot.prototype) {
    cover.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: 400 }));
    await wait(() => window.choroCommentGeometry().scrollY > 100, 'scroll while commenting');
    assert(frame === document.getElementById('screen'), 'Scrolling in Comments preserves the prototype');
    document.querySelector('.comment-row').click();
    await wait(() => window.choroCommentGeometry().scrollY < 400, 'selected pin remains reachable');
    const stage = document.getElementById('canvas');
    for (let index = 0; index < 8; index++) window.choroStudioReply({ session: boot.session, type: 'camera-command', command: 'zoom-in' });
    stage.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaX: 2500, deltaY: 2500, clientX: 100, clientY: 100 }));
    const visiblePin = () => {
      const pin = document.querySelector('.comment-pin');
      if (!pin) return false;
      const bounds = pin.getBoundingClientRect(), area = stage.getBoundingClientRect(), sidebar = document.querySelector('.comments-panel').getBoundingClientRect();
      return bounds.left >= area.left && bounds.right <= Math.min(area.right, sidebar.left) && bounds.top >= area.top && bounds.bottom <= area.bottom;
    };
    await wait(() => !visiblePin(), 'zoomed and panned pin is outside the stage');
    const zoom = window.choroCommentGeometry().zoom;
    document.querySelector('.comment-row').click();
    await wait(visiblePin, 'sidebar selection reveals the pin through stage zoom and pan');
    assert(window.choroCommentGeometry().zoom === zoom && frame === document.getElementById('screen'), 'Revealing a zoomed pin keeps its zoom and the live prototype');
    window.choroStudioReply({ session: boot.session, type: 'camera-command', command: 'fit' });
  }
  button('Resolve').click(); await wait(() => comments.pins[0].resolved && !document.querySelector('.comment-pin'), 'resolved pin');
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', bubbles: true }));
  await wait(() => !document.querySelector('.comments-panel'), 'exit Comments');
  assert(frame === document.getElementById('screen') && document.body.classList.contains('preview') === !!boot.prototype, 'Exiting keeps Design/Prototype and the same live screen');
  const reads = count('comments-read');
  window.choroStudioReply({ session: 'foreign', type: 'comment-mode', enabled: true });
  assert(!document.querySelector('.comments-panel') && count('comments-read') === reads, 'Foreign replies cannot activate comments');
  const remote = { id: '30000000-0000-4000-8000-000000000001', screen_id: otherScreen, x: .5, y: .2, body: 'Feedback on another screen', resolved: false, created_at: 1791140000 };
  comments.pins.push({ ...comments.pins[0], id: '30000000-0000-4000-8000-000000000002', body: 'Current screen feedback', resolved: false }, remote);
  comments.revision++;
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', bubbles: true }));
  await wait(() => document.querySelectorAll('.comment-row').length === 2, 'all-screen comment index');
  assert(document.querySelectorAll('.comment-pin').length === 1, 'Other screens appear in the index without placing pins on this screen');
  const remoteRow = () => [...document.querySelectorAll('.comment-row')].find(row => row.textContent.includes(remote.body));
  remoteRow().click();
  await wait(() => document.querySelector('.comments-panel [role="alert"]')?.textContent.includes('Fixture screen unavailable'), 'navigation failure');
  assert(!document.querySelector('.comment-popover') && !remoteRow().disabled, 'Failed screen navigation stays retryable in the index');
  remoteRow().click();
  await wait(() => count('comment-focus') === 2 && remoteRow().disabled, 'retry cross-screen selection');
  assert(focusRequest.id === remote.id, 'The chosen row requests its saved pin');
  // Simulate the native host replacing the screen and carrying the selection
  // into its ready reply. This is isolated; no project or live app is opened.
  window.choroStudioReply({ session: boot.session, type: 'comment-mode', enabled: false });
  await wait(() => !document.querySelector('.comments-panel'), 'old screen comments unmounted');
  const previousGeometry = window.choroCommentGeometry;
  window.choroCommentGeometry = () => ({ ...previousGeometry(), screen_id: otherScreen, name: 'Another screen' });
  window.choroStudioReply({ session: boot.session, type: 'comment-mode', enabled: true, selected: remote.id });
  await wait(() => document.querySelector('.comment-popover .comment-body')?.textContent === remote.body, 'selected note opened on its screen');
  assert(document.querySelectorAll('.comment-row').length === 2, 'The new screen keeps the full comment index');
  assert(document.querySelector('.comment-location').textContent === 'Another screen', 'The selected note belongs to the newly opened screen');
  await wait(() => document.querySelectorAll('.comment-pin').length === 1, 'selected screen pin follows prototype scroll');
  window.choroStudioReply({ session: boot.session, type: 'comment-mode', enabled: false });
  await wait(() => !document.querySelector('.comments-panel'), 'finish cross-screen selection');
  originalSend(JSON.stringify({ type: 'thumbnail-ready' }));
};
