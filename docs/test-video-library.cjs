// Run with NODE_PATH pointing to an installed jsdom package. No browser or UI control.
const fs = require('node:fs');
const assert = require('node:assert/strict');
const {JSDOM, VirtualConsole} = require('jsdom');
const source = fs.readFileSync(require('node:path').join(__dirname, 'how-to-video-library.html'), 'utf8');
const flush = () => new Promise(resolve => setImmediate(resolve));

async function check(mode) {
  const errors = [], requests = [], copies = [];
  let pauses = 0, loads = 0;
  const vc = new VirtualConsole();
  vc.on('jsdomError', error => errors.push(error));
  const dom = new JSDOM(source, {
    url: mode === 'file' ? 'file:///tmp/library.html' : 'http://127.0.0.1:8769/#start',
    runScripts: 'dangerously', virtualConsole: vc,
    beforeParse(w) {
      if (mode === 'server') w.CHORO_VIDEO_LIBRARY = {token: 'test-token'};
      w.HTMLElement.prototype.scrollIntoView = () => {};
      w.HTMLElement.prototype.getClientRects = function() { return this.closest('[hidden]') ? [] : [{}]; };
      w.HTMLMediaElement.prototype.pause = () => pauses++;
      w.HTMLMediaElement.prototype.load = () => loads++;
      w.requestAnimationFrame = callback => callback();
      w.fetch = async (url, options) => { requests.push({url, options}); return {ok: true}; };
      w.navigator.clipboard = {writeText: async text => copies.push(text)};
    }
  });
  const w=dom.window, d=w.document, q=s=>d.querySelector(s);
  const manifest=JSON.parse(q('#completed-video-files').textContent);
  const drafts=JSON.parse(q('#draft-video-files').textContent);
  const available=[...manifest,...drafts];
  const rows=[...d.querySelectorAll('[role="tabpanel"] tbody tr')];
  assert.ok(manifest.length > 0);
  assert.equal(new Set(manifest.map(video=>video.id)).size, manifest.length);
  assert.equal(new Set(available.map(video=>video.id)).size, available.length);
  assert.equal(Number(q('#draft-count').textContent), drafts.length);
  assert.equal(rows.filter(row=>row.dataset.status==='completed').length, manifest.length);
  const key=(node,k,more={})=>node.dispatchEvent(new w.KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true,...more}));
  for(const video of available) {
    const row=rows.find(r=>r.querySelector('.video-title').textContent===video.title);
    assert.ok(row); row.click();
    if(video.draft) {
      assert.notEqual(row.dataset.status,'completed');
      assert.match(q('#video-file').textContent,/review draft/);
      assert.match(q('#draft-production-note').textContent,/Draft ready/);
      assert.doesNotMatch(q('.scene-guidance').textContent,/Alex narrates/);
      assert.match(row.getAttribute('aria-label'),/Review draft available/);
    }
    assert.equal(q('#finished-video').hidden,false);
    assert.equal(q('main').inert,true);
    const expected=mode==='server'?'/media/'+video.id:video.file;
    assert.ok(decodeURI(q('#video-link').href).includes(expected));
    assert.equal(q('#video-preview').hasAttribute('src'),mode!=='http');
    assert.ok(q('#recording-scenes').children.length>0);
    q('#video-copy').click(); await flush();
    assert.equal(copies.at(-1),q('#video-link').href);
    q('#video-folder').click(); await flush();
    if(mode==='server') {
      assert.equal(requests.at(-1).url,'/open-folder/'+video.id);
      assert.equal(requests.at(-1).options.method,'POST');
      assert.equal(requests.at(-1).options.headers['X-Choro-Library-Token'],'test-token');
    } else assert.match(q('#video-status').textContent,/launcher/);
    key(q('.modal-close'),'Tab',{shiftKey:true});
    assert.equal(d.activeElement,q('.modal-done'));
    key(q('.modal-done'),'Tab'); assert.equal(d.activeElement,q('.modal-close'));
    key(d,'Escape');
    assert.equal(q('#recording-flow-modal').hidden,true);
    assert.equal(q('#video-preview').hasAttribute('src'),false);
    assert.equal(q('main').inert,false);
    assert.equal(d.activeElement,row);
  }
  for(const row of rows.filter(r=>!available.some(v=>v.title===r.querySelector('.video-title').textContent))) {
    row.click(); assert.equal(q('#finished-video').hidden,true);
    if(!['panel-design','panel-start'].includes(row.closest('[role="tabpanel"]').id)) {
      assert.equal(q('#draft-production-note').hidden,false);
      assert.match(q('#draft-production-note').textContent,/blocked|investigation/i);
      assert.ok(q('#draft-production-note').textContent.length>100);
    }
    q('.modal-done').click();
  }
  rows.find(r=>r.dataset.status==='completed').click();
  if(mode==='server') {
    q('#video-preview').dispatchEvent(new w.Event('error'));
    assert.equal(q('#video-error').hidden,false);
    w.fetch=async()=>{throw Error('offline');};
    q('#video-folder').click(); await flush();
    assert.match(q('#video-status').textContent,/Could not open/);
    w.navigator.clipboard.writeText=async()=>{throw Error('denied');};
    q('#video-copy').click(); await flush();
    assert.match(q('#video-status').textContent,/Copy this local link/);
  }
  q('#recording-flow-modal').click();
  assert.equal(q('#video-error').hidden,true);
  assert.ok(pauses>=manifest.length && loads>=manifest.length);
  assert.deepEqual(errors,[]);
  dom.window.close();
  console.log(`PASS ${mode}: ${manifest.length} video mappings, ${rows.length} dialogs, playback cleanup, links, focus, and failure states`);
}
(async()=>{for(const mode of ['server','file','http'])await check(mode);})().catch(error=>{console.error(error);process.exitCode=1;});
