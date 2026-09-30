// Run with NODE_PATH pointing to an installed jsdom package. No browser or UI control.
const fs = require('node:fs');
const assert = require('node:assert/strict');
const {JSDOM, VirtualConsole} = require('jsdom');
const originalSource = fs.readFileSync(require('node:path').join(__dirname, 'how-to-video-library.html'), 'utf8');
const socialFixture = {id:'social-test-1',title:'One useful vertical idea',sourceTitle:'Create Specialists on Demand',takeaway:'Focused help without a saved profile.',folder:'test-social',file:'vertical.mp4',poster:'poster.png',duration:27,social:true};
const existingSocial = JSON.parse(originalSource.match(/id="social-video-files">([\s\S]*?)<\/script>/)[1]);
const source = originalSource.replace(/(<script type="application\/json" id="social-video-files">)[\s\S]*?(<\/script>)/,(_,a,b)=>a+JSON.stringify([...existingSocial,socialFixture])+b);
const flush = () => new Promise(resolve => setImmediate(resolve));

async function check(mode, initialHash = 'start') {
  const errors = [], requests = [], copies = [];
  let pauses = 0, loads = 0;
  const vc = new VirtualConsole();
  vc.on('jsdomError', error => errors.push(error));
  const dom = new JSDOM(source, {
    url: (mode === 'file' ? 'file:///tmp/library.html' : 'http://127.0.0.1:8769/') + '#' + initialHash,
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
  assert.equal(q('#short-video-area').hidden,initialHash!=='shorts');
  assert.equal(q('#panel-tiktok').hidden,initialHash!=='tiktok');
  assert.equal(q('.tabs-shell').hidden,['shorts','tiktok','social-plan'].includes(initialHash));
  assert.equal(q('#social-plan-area').hidden,initialHash!=='social-plan');
  if(initialHash==='social-plan') assert.equal(q('#social-plan-frame').getAttribute('src'),'social-media-plan.html');
  assert.match(q('h1').textContent,/content marketing/i);
  q('#nav-social-plan').click();
  assert.equal(w.location.hash,'#social-plan');
  assert.equal(q('#social-plan-area').hidden,false);
  assert.equal(q('#short-video-area').hidden,true);
  assert.equal(q('.tabs-shell').hidden,true);
  assert.equal(q('.summary').hidden,true);
  assert.equal(q('#nav-social-plan').getAttribute('aria-current'),'page');
  assert.equal(d.querySelectorAll('.library-nav [aria-current="page"]').length,1);
  assert.equal(d.querySelectorAll('[role="tabpanel"]:not([hidden])').length,0);
  assert.equal(q('#social-plan-frame').getAttribute('src'),'social-media-plan.html');
  assert.ok(q('#social-plan-frame').title.includes('social media plan'));
  q('#nav-shorts').click();
  assert.equal(q('#social-plan-area').hidden,true);
  assert.equal(q('#short-video-area').hidden,false);
  q('#nav-tutorials').click();
  const manifest=JSON.parse(q('#completed-video-files').textContent);
  const drafts=JSON.parse(q('#draft-video-files').textContent);
  const planned=JSON.parse(q('#planned-video-scripts').textContent);
  const agentQuestionTitle='Let Your Agents Ask Each Other';
  assert.equal(planned[agentQuestionTitle],undefined);
  const agentQuestionScript=JSON.parse(q('#draft-video-scripts').textContent)[agentQuestionTitle];
  assert.equal(agentQuestionScript.playlist,'productivity');
  assert.equal(agentQuestionScript.scenes.length,5);
  assert.match(agentQuestionScript.review.outcome,/same-project/);
  assert.ok([...d.querySelectorAll('#panel-productivity .video-title')].some(el=>el.textContent===agentQuestionTitle));
  const agentQuestionVideo=manifest.find(video=>video.title===agentQuestionTitle);
  assert.equal(agentQuestionVideo.avatar,true);
  assert.equal(agentQuestionVideo.duration,39.7);
  const socials=JSON.parse(q('#social-video-files').textContent);
  const available=[...manifest,...drafts];
  const studioTitles=['Design, Build, and Preview with Studio','Meet Choro Studio','Create a Design from Scratch','Build and Use a Design System','Create a Design from a Doc or Task','Refine a Design with the Assistant','Implement a Design with an Agent','Use Visual References with an Agent'];
  const tutorialScripts=JSON.parse(q('#draft-video-scripts').textContent);
  for(const title of studioTitles){
    const video=manifest.find(v=>v.title===title);
    assert.ok(video,`${title} must have a finished native Studio export`);
    assert.equal(video.avatar,true);
    assert.equal(planned[title],undefined);
    assert.ok(tutorialScripts[title]?.scenes.length>=3);
  }
  assert.equal(manifest.some(v=>v.title==='Compare the Design with the Live Result'),false,'Blank live Compare pane must not be published as success');
  const companionTitles=['Meet Choro Companion','Set the Mood with Companion Music','Show or Hide Choro Companion'];
  for(const title of companionTitles){
    const video=manifest.find(v=>v.title===title);
    assert.ok(video,`${title} must have a completed Alex export`);
    assert.equal(video.avatar,true);
    assert.equal(video.draft,false);
    assert.equal(drafts.some(v=>v.title===title),false);
    assert.equal(planned[title],undefined,'Completed Companion videos must not retain planned-only state');
  }
  const rows=[...d.querySelectorAll('[role="tabpanel"]:not([data-format="social"]) tbody tr')];
  assert.equal(new Set(rows.map(row=>row.querySelector('.video-title').textContent)).size,rows.length);
  assert.equal(q('#panel-companion').querySelectorAll('tbody tr').length,3);
  assert.equal(rows.find(row=>row.querySelector('.video-title').textContent==='Meet Choro Companion').closest('[role="tabpanel"]').id,'panel-start');
  assert.equal(rows.find(row=>row.querySelector('.video-title').textContent==='Follow Agents with Desktop Companion').closest('[role="tabpanel"]').id,'panel-companion');
  q('#tab-companion').click();
  assert.equal(q('#panel-companion').hidden,false);
  assert.equal(w.location.hash,'#companion');
  assert.equal(Number(q('#playlist-count').textContent),d.querySelectorAll('[role="tabpanel"]:not([data-format="social"])').length);
  assert.equal(Number(q('#video-count').textContent),rows.length);
  const concepts=JSON.parse(q('#short-video-concepts').textContent);
  assert.equal(new Set(concepts.map(item=>item.id)).size,concepts.length);
  assert.equal(concepts.length,24);
  assert.equal(concepts.filter(item=>item.stage==='idea').length,6);
  const reviewBatch=concepts.filter(item=>item.reviewBatch==='five-promos-20260922');
  assert.deepEqual(reviewBatch.map(item=>item.id).sort(),['doc-context','on-demand','rough-idea','safe-experiment','studio']);
  assert.equal(concepts.find(item=>item.id==='phone-decision').mediaId,undefined,'Choro Mobile stays outside this batch');
  assert.equal(q('#tab-tiktok'),null,'Archive must not occupy a tutorial category tab');
  q('#nav-shorts').click();
  assert.equal(w.location.hash,'#shorts');
  assert.equal(q('#short-video-area').hidden,false);
  assert.equal(q('.tabs-shell').hidden,true);
  assert.equal(q('.summary').hidden,true);
  assert.equal(q('#nav-shorts').getAttribute('aria-current'),'page');
  assert.equal(q('#nav-tutorials').hasAttribute('aria-current'),false);
  assert.equal(d.querySelectorAll('[role="tabpanel"]:not([hidden])').length,0);
  assert.equal(d.querySelectorAll('.short-open').length,concepts.length);
  assert.equal(Number(q('#video-count').textContent),rows.length,'Concepts must not inflate tutorial counts');
  q('#watch-short-example').click();
  assert.equal(q('#finished-video').hidden,false);
  assert.equal(q('#video-preview').dataset.format,'social');
  assert.match(q('#video-link').href,/social-point-dont-explain-upbeat|upbeat-v2/);
  assert.match(q('#video-file').textContent,/Fresh upbeat ElevenLabs/);
  q('.modal-done').click();
  assert.equal(d.activeElement,q('#watch-short-example'));
  for(const concept of concepts) {
    const button=q(`[data-concept-id="${concept.id}"]`);
    button.click();
    assert.equal(q('#recording-title').textContent,concept.title);
    assert.equal(q('#scene-review').hidden,false);
    assert.equal(q('#recording-scenes').children.length,concept.stage==='idea'?concept.beats.length:concept.scenes.length+(concept.mediaId?0:1));
    assert.equal(q('#recording-outcome').textContent,concept.proof);
    assert.match(q('.scene-guidance').textContent,/Social-first/);
    if(concept.stage==='idea') {
      assert.match(button.textContent,/Idea only/);
      assert.match(q('#recording-scenes').textContent,/Narration not drafted/);
      assert.match(q('#recording-intro').textContent,/not scripted or filmed/);
      assert.equal(Boolean(concept.mediaId),false);
    } else assert.match(q('#recording-scenes').textContent,/Your home for building products/);
    assert.equal(q('#finished-video').hidden,!concept.mediaId);
    if(concept.mediaId) {
      const media=socials.find(video=>video.id===concept.mediaId);
      assert.ok(media);
      if(concept.reviewBatch==='five-promos-20260922') {
        assert.match(button.textContent,/Review new video/);
        assert.match(q('#recording-intro').textContent,/ready for review/);
        assert.ok(media.duration>20&&media.duration<30);
        assert.match(media.folder,/^social-five-review-20260922\//);
        assert.match(q('#video-link').href,/social-five-|social-five-review-20260922/);
        assert.equal(concept.scenes[0][0],'hook');
        assert.equal(concept.scenes.at(-1)[0],'outro');
      }
    }
    else {
      assert.equal(q('#video-preview').hasAttribute('src'),false);
      assert.equal(q('#video-link').hasAttribute('href'),false);
      assert.match(q('#recording-intro').textContent,/not (scripted or )?filmed/);
      assert.ok(concept.requirement.length>20);
    }
    q('.modal-done').click();
    assert.equal(d.activeElement,button);
  }
  q('#short-search').value='zz nonexistent subject';
  q('#short-search').dispatchEvent(new w.Event('input'));
  assert.equal(q('#short-empty').hidden,false);
  assert.equal([...d.querySelectorAll('.short-group')].every(group=>group.hidden),true);
  q('#short-search').value='worktree';
  q('#short-search').dispatchEvent(new w.Event('input'));
  assert.equal([...d.querySelectorAll('.short-list li')].filter(item=>!item.hidden).length,1);
  q('#short-search').value=''; q('#short-search').dispatchEvent(new w.Event('input'));
  assert.equal(q('#short-empty').hidden,true);
  q('#short-archive-link').click();
  assert.equal(q('#panel-tiktok').hidden,false);
  assert.equal(w.location.hash,'#tiktok');
  assert.equal(q('#short-video-area').hidden,true);
  assert.equal(q('.tabs-shell').hidden,true);
  assert.equal(q('#nav-shorts').getAttribute('aria-current'),'page');
  assert.equal(q('#social-rows').children.length,socials.length);
  q('#social-search').value='no matching subject xyz';
  q('#social-search').dispatchEvent(new w.Event('input'));
  assert.equal(q('#social-table').hidden,true);
  assert.match(q('#social-empty').textContent,/No clips match/);
  q('#social-search').value=''; q('#social-search').dispatchEvent(new w.Event('input'));
  for(const video of socials) {
    const row=[...q('#social-rows').children].find(r=>r.dataset.socialId===video.id);
    row.click();
    assert.equal(q('#video-preview').dataset.format,'social');
    assert.equal(q('#scene-review').hidden,true);
    assert.match(q('#recording-outcome').textContent,/AI presenter/);
    assert.equal(q('#video-preview').hasAttribute('src'),mode!=='http');
    q('#video-folder').click(); await flush();
    if(mode==='server')assert.equal(requests.at(-1).url,'/open-folder/'+video.id);
    q('#video-copy').click(); await flush(); assert.equal(copies.at(-1),q('#video-link').href);
    q('.modal-done').click();
    assert.equal(q('#video-preview').hasAttribute('src'),false);
    assert.equal(d.activeElement,row);
  }
  q('.archive-back').click();
  assert.equal(w.location.hash,'#shorts');
  assert.equal(q('#panel-tiktok').hidden,true);
  q('#nav-tutorials').click();
  assert.equal(w.location.hash,'#start');
  assert.equal(q('.tabs-shell').hidden,false);
  assert.equal(q('.summary').hidden,false);
  assert.equal(q('#short-video-area').hidden,true);
  assert.equal(q('#nav-tutorials').getAttribute('aria-current'),'page');
  assert.ok(manifest.length > 0);
  assert.equal(new Set(manifest.map(video=>video.id)).size, manifest.length);
  assert.equal(new Set(available.map(video=>video.id)).size, available.length);
  assert.equal(Number(q('#draft-count').textContent), drafts.length);
  assert.equal(rows.filter(row=>row.dataset.status==='completed').length, manifest.length);
  for(const row of rows.filter(row=>row.dataset.status==='completed')) {
    assert.equal(row.querySelector('.completion-badge')?.textContent,'Completed','Every completed video must show a visible badge');
  }
  const key=(node,k,more={})=>node.dispatchEvent(new w.KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true,...more}));
  for(const video of available) {
    const row=rows.find(r=>r.querySelector('.video-title').textContent===video.title);
    assert.ok(row); row.click();
    if(companionTitles.includes(video.title)||['Start a Chat with a Bandmate','Save a Git Workflow: Dev to Main'].includes(video.title)) {
      const script=JSON.parse(q('#draft-video-scripts').textContent)[video.title];
      assert.equal(row.dataset.status,'completed');
      assert.equal(q('#recording-scenes').children.length,script.scenes.length+1);
      assert.match(q('#recording-scenes').textContent,/Hey, I’m Alex from Choro/);
      assert.match(q('#video-file').textContent,/Alex D narration \+ HeyGen/);
      assert.equal(q('#draft-production-note').hidden,true);
      q('.modal-done').click(); row.click();
      assert.equal(q('#recording-scenes').children.length,script.scenes.length+1,'Reopening must not duplicate greeting');
    }
    if(video.draft) {
      assert.notEqual(row.dataset.status,'completed');
      assert.match(q('#video-file').textContent,/review draft/);
      assert.match(q('#draft-production-note').textContent,video.avatarPending ? /Avatar blocked/ : planned[video.title] ? /Revision needed/ : /Draft ready/);
      if (video.localVoiceOnly) {
        assert.equal(video.avatar, false);
        assert.ok(!video.avatarPending);
        assert.match(q('#video-file').textContent, /Free local AI voice.*No avatar/);
        assert.match(q('.scene-guidance').textContent, /Free local AI narration, no avatar/);
        assert.doesNotMatch(q('#draft-production-note').textContent, /Avatar blocked/);
        if(companionTitles.includes(video.title)){
          const script=JSON.parse(q('#draft-video-scripts').textContent)[video.title];
          assert.equal(q('#recording-scenes').children.length,script.scenes.length);
          assert.match(row.querySelector('.completion-badge').textContent,/Draft ready/);
          assert.doesNotMatch(q('.scene-guidance').textContent,/to be selected|not yet recorded/);
        }
      }
      if (video.avatarPending) {
        assert.equal(planned[video.title], undefined);
        assert.match(q('#video-file').textContent,/4K review draft.*Alex D.*Avatar render blocked/);
        assert.doesNotMatch(q('#video-file').textContent,/Free local|Previous version/);
        assert.match(q('.scene-guidance').textContent,/fresh footage recorded/);
        if(video.title==='Start a Chat with a Bandmate') assert.match(q('#recording-scenes').textContent,/beneath the conversation title/);
        if(video.title==='Save a Git Workflow: Dev to Main') {
          assert.match(q('#recording-scenes').textContent,/Confirm merge in Demo repo/);
          assert.match(q('#draft-production-note').textContent,/merged dev-to-main result/);
          assert.doesNotMatch(q('#draft-production-note').textContent,/awaiting.*confirmation/i);
        }
      }
      assert.doesNotMatch(q('.scene-guidance').textContent,/Alex narrates/);
      assert.match(row.getAttribute('aria-label'),/review draft available/i);
      if(planned[video.title]) {
        assert.match(q('#video-file').textContent,/Previous version/);
        assert.match(q('.scene-guidance').textContent,/not yet recorded/);
        assert.equal(q('#recording-scenes').children.length,planned[video.title].scenes.length);
        assert.match(row.querySelector('.completion-badge').textContent,/Revision needed/);
        if(video.title==='Follow and Manage Your Agents') {
          assert.match(q('#recording-scenes').textContent,/Working/);
          assert.match(q('#recording-scenes').textContent,/Needs attention/);
          assert.match(q('#recording-scenes').textContent,/completion/);
        }
        if(video.title==='Start a Chat with a Bandmate') {
          assert.match(q('#recording-scenes').textContent,/profile chip in the composer/);
          assert.match(q('#recording-scenes').textContent,/beneath the conversation title/);
        }
        if(video.title==='Dictate Prompts and Messages') {
          const scenes=planned[video.title].scenes;
          assert.equal(scenes[1].id,'hold-shortcut');
          assert.equal(scenes[2].id,'hands-off');
          assert.equal(scenes[3].id,'click-alternative');
          assert.match(scenes[1].shown,/centered ⌘L/);
          assert.match(scenes[1].text,/release the keys/);
          assert.match(scenes[2].shown,/centered ⌘⇧L/);
          assert.match(scenes[2].text,/again.*stop/);
        }
      }
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
    const rowTitle=row.querySelector('.video-title').textContent;
    if(!['panel-design','panel-start'].includes(row.closest('[role="tabpanel"]').id) || planned[rowTitle]) {
      assert.equal(q('#draft-production-note').hidden,false);
      assert.match(q('#draft-production-note').textContent,/blocked|blocker|investigation|awaiting confirmation|recording needed|in production/i);
      assert.ok(q('#draft-production-note').textContent.length>100);
      const title=row.querySelector('.video-title').textContent;
      if(planned[title]) {
        assert.equal(q('#recording-scenes').children.length,planned[title].scenes.length);
        assert.match(q('.scene-guidance').textContent,/not yet recorded/);
        for (const scene of planned[title].scenes) assert.ok(q('#recording-scenes').textContent.includes(scene.text));
      }
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
  w.location.hash='shorts';
  await new Promise(resolve=>w.setTimeout(resolve,10));
  assert.equal(q('#short-video-area').hidden,false,'Hash navigation reaches the main Shorts section');
  q('#watch-short-example').click();
  w.location.hash='agents';
  await new Promise(resolve=>w.setTimeout(resolve,10));
  assert.equal(q('#recording-flow-modal').hidden,true,'Navigation closes the modal and clears playback');
  assert.equal(q('#video-preview').hasAttribute('src'),false);
  assert.equal(q('main').inert,false);
  assert.equal(q('#panel-agents').hidden,false);
  assert.equal(q('.tabs-shell').hidden,false);
  assert.ok(pauses>=manifest.length && loads>=manifest.length);
  assert.deepEqual(errors,[]);
  dom.window.close();
  console.log(`PASS ${mode} #${initialHash}: ${manifest.length} video mappings, ${rows.length} tutorial dialogs, ${concepts.length} short concepts, navigation, cleanup, focus, and failure states`);
}
(async()=>{
  for(const mode of ['server','file','http'])await check(mode);
  await check('server','shorts');
  await check('server','tiktok');
  await check('file','shorts');
  await check('server','social-plan');
  await check('file','social-plan');
})().catch(error=>{console.error(error);process.exitCode=1;});
