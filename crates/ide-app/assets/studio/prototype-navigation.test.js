// Offscreen WebKit: the trusted player stays mounted while screens are prepared.
window.runPrototypeNavigationTest = async ({ wait, assert, messages, originalSend }) => {
  const boot = window.__CHORO_STUDIO__, initial = document.getElementById('screen');
  const session = boot.session, before = initial.getBoundingClientRect(), results = new Map(), probes = new Map();
  addEventListener('message', event => {
    if (event.data?.type === 'transition-result') results.set(event.data.name, event.data);
    if (event.data?.type === 'transition-probed') probes.set(event.data.name, event.data);
  });
  const next = (name, width = 390, height = 844) => ({ ...structuredClone(boot), screen_id: name, width, height, authored_height: 2200,
    document: { html: '<html><body><h1>' + name + '</h1></body></html>', css: 'body{margin:0;background:#234567;color:white;min-height:2200px}',
      js: 'const instance=Math.random(),name=' + JSON.stringify(name) + ';addEventListener("load",()=>parent.postMessage({type:"transition-result",name,instance,width:innerWidth,height:innerHeight},"*"));addEventListener("message",event=>{if(event.source===parent&&event.data?.type==="transition-probe")parent.postMessage({type:"transition-probed",name,instance,color:getComputedStyle(document.body).backgroundColor},"*")});' } });
  const show = incoming => window.choroStudioReply({ session, type: 'prototype-screen', bootstrap: incoming });
  const started = performance.now();
  show(next('second'));
  assert(document.getElementById('screen') === initial && initial.getBoundingClientRect().width === before.width && getComputedStyle(initial).visibility === 'visible', 'Navigation retains the visible screen and geometry while preparing its destination');
  assert(document.querySelectorAll('iframe').length === 2 && getComputedStyle(document.getElementById('screen-pending')).visibility === 'hidden', 'Only one hidden destination is prepared, with no visible blank iframe');
  await wait(() => boot.screen_id === 'second' && !document.getElementById('screen-pending'), 'prepared destination committed');
  const firstSwapMs = performance.now() - started;
  assert(results.get('second')?.width === 390 && results.get('second')?.height === 844, 'The destination loads at its authored device viewport before it is revealed');
  assert(window.__CHORO_STUDIO__ === boot && boot.session === session && document.querySelectorAll('iframe').length === 1, 'Navigation reuses the player shell and disposes its old iframe');
  const second = document.getElementById('screen');
  const preparedInstance = results.get('second').instance;
  second.contentWindow.postMessage({ type: 'transition-probe' }, '*');
  await wait(() => probes.has('second'), 'revealed destination remains loaded');
  assert(probes.get('second').instance === preparedInstance && probes.get('second').color === 'rgb(35, 69, 103)', 'Revealing a prepared iframe preserves its loaded document and script state');
  window.choroStudioReply({ session: 'foreign', type: 'prototype-screen', bootstrap: next('forged') });
  assert(document.getElementById('screen') === second && !document.getElementById('screen-pending'), 'Foreign sessions cannot replace the player');
  show(next('superseded')); show(next('latest', 1440, 960));
  assert(document.querySelectorAll('iframe').length === 2, 'Rapid navigation replaces the pending screen without accumulating iframes');
  await wait(() => boot.screen_id === 'latest', 'latest navigation wins');
  assert(results.get('latest')?.width === 1440 && results.get('latest')?.height === 960 && document.getElementById('screen').getAttribute('sandbox') === 'allow-scripts', 'The latest screen preserves desktop viewport and opaque prototype isolation');
  show(next('second'));
  await wait(() => boot.screen_id === 'second', 'return to previous screen');
  assert(boot.session === session && results.get('second')?.width === 390 && document.querySelectorAll('iframe').length === 1, 'Returning to a previous screen keeps the player and restores its device viewport');
  const retained = document.getElementById('screen');
  show(next('failed'));
  document.getElementById('screen-pending').dispatchEvent(new Event('error'));
  assert(boot.screen_id === 'second' && document.getElementById('screen') === retained && document.querySelectorAll('iframe').length === 1 && retained.style.pointerEvents === '', 'A failed destination keeps the previous screen visible and interactive');
  assert(messages.some(message => message.type === 'prototype-failed' && message.screen_id === 'failed' && message.previous_screen_id === 'second'), 'Failure reports both screens so native selection can roll back');
  show(next('retry'));
  await wait(() => boot.screen_id === 'retry', 'navigation recovers after failure');
  assert(!document.getElementById('error').textContent, 'Successful navigation clears the failure message');
  // The native ready reply reattaches the comments adapter to the new screen.
  boot.screens.push({ id: 'with-note', name: 'With note', width: 390, height: 2200, archived: false });
  const note = { id: 'saved-note', screen_id: 'with-note', x: .3, y: .2, body: 'Pinned feedback', resolved: false, created_at: 1791140000 };
  const real = window.ipc.postMessage;
  window.ipc.postMessage = raw => {
    real(raw); const message = JSON.parse(raw);
    if (message.type === 'comments-read') queueMicrotask(() => window.choroStudioReply({ session, type: 'comments-result', request_id: message.request_id, comments: { schema_version: 1, revision: 1, pins: [note] } }));
    if (message.type === 'ready') queueMicrotask(() => window.choroStudioReply({ session, type: 'comment-mode', enabled: true, selected: note.id }));
  };
  window.choroStudioReply({ session, type: 'comment-mode', enabled: true });
  await wait(() => document.querySelector('.comment-row'), 'comments in retained shell');
  show(next('with-note'));
  await wait(() => document.querySelector('.comment-popover .comment-body')?.textContent === note.body && document.querySelector('.comment-pin'), 'selected comment attaches to new prototype screen');
  assert(document.querySelectorAll('iframe').length === 1 && messages.filter(message => message.type === 'comments-read').length === 2, 'Comments remount once for the destination without rebuilding the player');
  window.choroStudioReply({ session, type: 'comment-mode', enabled: false });
  window.ipc.postMessage = real;
  show(next('Prototype destination', boot.testMobilePrototype ? 390 : 1440, boot.testMobilePrototype ? 844 : 960));
  await wait(() => boot.screen_id === 'Prototype destination', 'final fitted destination');
  assert(window.testErrors.length === 0, 'No player errors: ' + JSON.stringify(window.testErrors));
  originalSend(JSON.stringify({ type: 'phase', first_swap_ms: firstSwapMs, retained_player: true, max_iframes: 2 }));
  originalSend(JSON.stringify({ type: 'thumbnail-ready' }));
};
