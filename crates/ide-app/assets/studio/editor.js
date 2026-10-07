/* Choro owns source, scope, and persistence. Vvveb supplies element matching,
   inline editing the property inspector and the undo stack. Never serialize the running preview DOM. */
(() => {
  const boot = window.__CHORO_STUDIO__;
  if (!boot || (window.top !== window && !boot.inline)) return;
  const $ = id => document.getElementById(id);
  const uuid = () => typeof crypto.randomUUID==='function'?crypto.randomUUID():([1e7]+-1e3+-4e3+-8e3+-1e11).replace(/[018]/g,c=>(c^crypto.getRandomValues(new Uint8Array(1))[0]&15>>c/4).toString(16));
  let source = {...structuredClone(boot.document),overrides:structuredClone(boot.overrides||{})}, clean, selectedId = null, mode = !boot.thumbnail && boot.mode==='preview' ? 'preview' : 'edit';
  let revision = boot.revision, fingerprint = boot.fingerprint, pending = false, dirty = false, timer, saveId = null, savedText = JSON.stringify(source), inFlightText, flushRequest = null;
  let frame = $('screen'), scale = 1, textEditBefore = null;
  let commenting = false, commentScroll = { x: 0, y: 0 }, commentFocus = null;
  const commentAvailable = !boot.thumbnail && !boot.system_specimen;
  function installCommentInput(doc, emit) {
    const win = doc.defaultView;
    let enabled = false;
    const report = focus => { if (enabled) emit({ type: 'studio-comment-scroll', x: win.scrollX, y: win.scrollY, ...(typeof focus === 'string' ? { focus } : {}) }); };
    doc.addEventListener('keydown', event => {
      if (!event.defaultPrevented && !event.repeat && !event.isComposing && !event.metaKey && !event.ctrlKey && !event.altKey && event.key.toLowerCase() === 'c'
          && !event.target.closest?.('input,textarea,select,[contenteditable]:not([contenteditable="false"])')) {
        event.preventDefault(); event.stopPropagation(); emit({ type: 'studio-comment-toggle' });
      }
    }, true);
    win.addEventListener('message', event => {
      if (event.source !== win.parent) return;
      if (event.data?.type === 'studio-comments-state') { enabled = !!event.data.enabled; report(); }
      if (enabled && event.data?.type === 'studio-comment-wheel' && [event.data.dx, event.data.dy].every(Number.isFinite)) {
        win.scrollBy({ left: Math.max(-2000, Math.min(2000, event.data.dx)), top: Math.max(-2000, Math.min(2000, event.data.dy)), behavior: 'instant' }); report();
      }
      if (enabled && event.data?.type === 'studio-comment-focus' && [event.data.x, event.data.y].every(Number.isFinite)) {
        win.scrollTo({ left: Math.max(0, event.data.x - win.innerWidth / 2), top: Math.max(0, event.data.y - win.innerHeight / 2), behavior: 'instant' }); report(event.data.focus);
      }
    });
    doc.addEventListener('scroll', report, true);
  }
  function commentGeometry() {
    const r = frame.getBoundingClientRect(), area = $('canvas').getBoundingClientRect();
    return { screen_id: boot.screen_id, name: boot.screens.find(s => s.id === boot.screen_id)?.name || 'Screen',
      x: r.left - area.left, y: r.top - area.top, width: boot.width, height: boot.authored_height || boot.height,
      visibleWidth: r.width, visibleHeight: r.height, zoom: scale, scrollX: commentScroll.x, scrollY: commentScroll.y };
  }
  window.choroCommentGeometry = commentGeometry;
  window.choroCommentFocus = pin => {
    if (!boot.prototype || pin.screen_id !== boot.screen_id || ![pin.x, pin.y].every(Number.isFinite)) return;
    const focus = uuid();
    commentFocus = { ...pin, focus };
    frame.contentWindow?.postMessage({ type: 'studio-comment-focus', x: pin.x * boot.width, y: pin.y * (boot.authored_height || boot.height), focus }, '*');
  };
  function revealComment(pin) {
    const area = $('canvas').getBoundingClientRect(), rect = frame.getBoundingClientRect();
    const sidebar = document.querySelector('.comments-panel')?.getBoundingClientRect();
    const right = sidebar && sidebar.left > area.left && sidebar.left < area.right ? sidebar.left : area.right;
    const x = rect.left + (pin.x * boot.width - commentScroll.x) * scale;
    const y = rect.top + (pin.y * (boot.authored_height || boot.height) - commentScroll.y) * scale;
    if (x >= area.left + 12 && x <= right - 44 && y >= area.top + 44 && y <= area.bottom - 12) return;
    // Reveal the actual scrolled pin in the uncovered stage without changing
    // the zoom or remounting the running prototype.
    camera.mode = 'free';
    camera.x += (x - (area.left + right) / 2) / scale;
    camera.y += (y - (area.top + area.bottom) / 2) / scale;
    cameraChanged();
  }
  window.choroCommentWheel = event => {
    if (event.ctrlKey || event.metaKey) cameraInput({ kind: 'zoom', x: event.clientX, y: event.clientY, dx: 0, dy: event.deltaY });
    else frame.contentWindow?.postMessage({ type: 'studio-comment-wheel', dx: event.deltaX, dy: event.deltaY }, '*');
  };
  const publishCommentGeometry = () => { if (commenting) window.dispatchEvent(new Event('studio-comment-geometry')); };
  if(boot.draft){source=boot.draft.document;revision=boot.draft.revision;fingerprint=boot.draft.fingerprint;dirty=true;}
  const parse = html => new DOMParser().parseFromString(html,'text/html');
  const send = payload => {
    const message={...payload,session:boot.session};
    if(boot.inline)parent.postMessage({type:'studio-editor',session:boot.session,message},'*');
    else window.ipc.postMessage(JSON.stringify(message));
  };
  let tokenCss = boot.tokens_css;
  let assets = boot.assets || {};
  // Text may be authored with semantic/formatting elements, not just headings
  // and generic wrappers. Keep shapes, form fields and layout sections out of
  // rich-text editing, but allow the same editing path for all these text tags.
  const editableTextTags=new Set(('H1 H2 H3 H4 H5 H6 P SPAN A BUTTON LABEL LI TD TH DIV '+
    'STRONG B EM I U S SMALL MARK SUB SUP ABBR CITE Q DFN CODE KBD SAMP VAR TIME DATA OUTPUT '+
    'DEL INS BDI BDO PRE BLOCKQUOTE DT DD FIGCAPTION CAPTION LEGEND SUMMARY ADDRESS').split(' '));
  const inspectorPreference = 'choro.studio.inspector-collapsed.v1';
  let inspectorCollapsed = (()=>{try{return localStorage.getItem(inspectorPreference)==='true';}catch{return false;}})();
  let liveBaseline = null, initializedToolbar = false, propertyBefore = null;
  // The status dot reads its semantic color from data-state; the words stay the contract.
  const statusStates={'Saved':'saved','Unsaved':'unsaved','Saving…':'saving','Save failed':'failed'};
  let toolbarQueued=false, lastToolbar='';
  function publishToolbar(){
    if(!boot.native_toolbar||toolbarQueued)return;
    toolbarQueued=true;
    queueMicrotask(()=>{
      toolbarQueued=false;
      const state={status:$('error').textContent&&!pending?'failed':$('status').dataset.state||'saved',can_undo:Vvveb.Undo.undoIndex>=0||!!Vvveb.WysiwygEditor.isActive,can_redo:Vvveb.Undo.undoIndex<Vvveb.Undo.mutations.length-1,inspector_open:!inspectorCollapsed,recovery_available:!$('recover').hidden};
      const key=JSON.stringify(state);
      if(key!==lastToolbar){lastToolbar=key;send({type:'toolbar-state',...state});}
    });
  }
  const setStatus=text=>{const node=$('status');node.textContent=text;node.dataset.state=statusStates[text];publishToolbar();};
  const serialize = () => '<!doctype html>\n' + clean.documentElement.outerHTML;
  function normalize() {
    clean = parse(source.html);
    let i=0; const used = new Set(), reserved=new Set([...clean.body.querySelectorAll('[data-studio-id]')].map(node=>node.dataset.studioId));
    for (const node of clean.body.querySelectorAll('*')) {
      let id=node.getAttribute('data-studio-id');
      if (!id || used.has(id)) { id='element-'+(++i); while(reserved.has(id)) id='element-'+(++i); reserved.add(id); node.setAttribute('data-studio-id',id); }
      used.add(id);
    }
  }
  function assetUrl(path, resources = assets) {
    const normalized=path.replace(/^\.\.\/\.\.\//,'').replace(/^\.\//,'');
    return resources[normalized] || (path.startsWith('data:image/') ? path : '');
  }
  function sanitize(doc, preview, incoming) {
    const content = incoming ? { ...incoming.document, overrides: incoming.overrides || {} } : source;
    const resources = incoming?.assets || assets, cssTokens = incoming?.tokens_css ?? tokenCss;
    doc.querySelectorAll('script,iframe,object,embed,base,meta[http-equiv],link').forEach(n=>n.remove());
    for(const node of doc.querySelectorAll('*')) {
      for(const attr of [...node.attributes]) {
        if(/^on/i.test(attr.name) || ['srcdoc','formaction','action','srcset'].includes(attr.name)) node.removeAttribute(attr.name);
      }
      if(node.hasAttribute('src')) node.setAttribute('src',assetUrl(node.getAttribute('src'), resources));
      if(node.hasAttribute('href')) node.setAttribute('href','#');
      node.removeAttribute('contenteditable');
    }
    const policy=doc.createElement('meta'); policy.httpEquiv='Content-Security-Policy';
    policy.content="default-src 'none'; img-src data:; font-src data:; style-src 'unsafe-inline'; script-src "+(preview?"'unsafe-inline'":"'none'")+"; connect-src 'none'; form-action 'none'; base-uri 'none'";
    doc.head.prepend(policy);
    if(!preview){
      const authored=doc.querySelector('style[data-studio-rules]');
      const rules=doc.createElement('style');rules.id='vvvebjs-styles';rules.textContent=authored?.textContent||'';
      authored?.remove();doc.head.append(rules);
    }
    const style=doc.createElement('style'); style.textContent=(cssTokens+'\n:root{'+Object.entries(content.overrides||{}).map(([k,v])=>'--'+k+':'+v+';').join('')+'}\n'+content.css).replace(/<\//g,'<\\/').replace(/url\(\s*['"]?([^'")]+)['"]?\s*\)/g,(match,path)=>{const url=assetUrl(path, resources);return url?'url("'+url+'")':match;}); doc.head.insertBefore(style,doc.getElementById('vvvebjs-styles'));
    if(preview) {
      // A prototype scrolls its own page. Without containment WebKit chains the
      // leftover wheel delta out to this frame, so reaching the end of the page
      // — or any sideways trackpad drift — pans the stage at the same time.
      // Only an explicit zoom gesture, forwarded by message below, moves the
      // camera from inside the screen.
      const isolate=doc.createElement('style');
      isolate.textContent='html{overscroll-behavior:contain}'+
        (boot.prototype?'html{overflow-x:hidden !important;overflow-y:auto !important}body{overflow:visible !important}':'');
      doc.head.append(isolate);
      const script=doc.createElement('script');
      script.textContent=content.js+'\n;document.addEventListener("click",e=>{const a=e.target.closest("[data-studio-screen]");if(a){e.preventDefault();parent.postMessage({type:"studio-navigate",screen_id:a.dataset.studioScreen},"*")}});';
      doc.body.append(script);
      if (commentAvailable) {
        const comments = doc.createElement('script');
        comments.textContent = '(' + installCommentInput.toString() + ')(document,value=>parent.postMessage(value,"*"));';
        doc.head.append(comments);
      }
      if(cameraEnabled){
        const controls=doc.createElement('script');
        controls.textContent='('+installCameraInput.toString()+')(document,input=>parent.postMessage({type:"studio-camera",input},"*"),()=>true,false,'+!!boot.prototype+');';
        doc.head.append(controls);
      }
    }
    return '<!doctype html>'+doc.documentElement.outerHTML;
  }
  let inlineView={x:0,y:0,zoom:1},inlineHasCamera=false,lastInlineSize='',lastInlineLayout='';
  let activationPoint=null;
  function activatePoint(){
    if(!activationPoint||mode!=='edit'||!frame.contentDocument||Vvveb.WysiwygEditor.doc!==frame.contentDocument)return;
    const point=activationPoint;activationPoint=null;
    const node=frame.contentDocument.elementFromPoint(point.x,point.y);
    if(node){node.dispatchEvent(new MouseEvent('dblclick',{bubbles:true,cancelable:true}));frame.contentWindow.focus();}
  }
  const inlinePost=(type,data={})=>parent.postMessage({type,session:boot.session,...data},'*');
  function publishInlineLayout(){
    if(!boot.inline||!inlineHasCamera)return;
    const area=$('canvas').getBoundingClientRect(), r=frame.getBoundingClientRect();
    const rectangles=[];
    const push=r=>{if(r.width>0&&r.height>0)rectangles.push({x:r.x,y:r.y,width:r.width,height:r.height});};
    const left=Math.max(area.left,r.left),top=Math.max(area.top,r.top),right=Math.min(area.right,r.right),bottom=Math.min(area.bottom,r.bottom);
    if(right>left&&bottom>top)push({x:left,y:top,width:right-left,height:bottom-top});
    for(const id of ['studio-toolbar','right-panel','wysiwyg-editor','error','clr-picker']){
      const node=$(id);if(node&&!node.hidden&&node.getClientRects().length)push(node.getBoundingClientRect());
    }
    const layout={rectangles,area:{x:area.x,y:area.y,width:area.width,height:area.height}};
    const key=JSON.stringify(layout);
    if(key!==lastInlineLayout){lastInlineLayout=key;inlinePost('studio-inline-layout',layout);}
  }
  // A camera changes only presentation. The child viewport and document stay live.
  const cameraEnabled=!boot.thumbnail&&!boot.system_specimen&&!boot.inline;
  const clampZoom=value=>Math.max(.02,Math.min(4,value));
  let camera, cameraKey, cameraTimer, reportedZoom;
  function restoreCamera(){
    cameraKey='choro.studio.camera.v1:'+boot.screen_id+':'+boot.width+'x'+boot.height;
    camera={mode:'fit',zoom:1,x:boot.width/2,y:boot.height/2};
    // The player shows one screen at a time and navigates between them, so a
    // per-screen remembered camera lands every click somewhere different. Each
    // prototype screen opens fitted and centred, in the same place as the last.
    if(boot.prototype)return;
    try{
      const raw=localStorage.getItem(cameraKey);
      const saved=raw&&raw.length<512?JSON.parse(raw):null;
      if(saved&&['fit','free'].includes(saved.mode)&&[saved.zoom,saved.x,saved.y].every(Number.isFinite)&&saved.zoom>=.02&&saved.zoom<=4&&Math.abs(saved.x)<1e7&&Math.abs(saved.y)<1e7)camera=saved;
    }catch{}
  }
  function persistCamera(){
    clearTimeout(cameraTimer);
    if(cameraEnabled&&!boot.prototype)try{localStorage.setItem(cameraKey,JSON.stringify(camera));}catch{}
  }
  function cameraChanged(){size();clearTimeout(cameraTimer);cameraTimer=setTimeout(persistCamera,150);}
  function syncZoom(){
    if(boot.prototype&&reportedZoom!==camera.zoom){reportedZoom=camera.zoom;send({type:'camera',zoom:camera.zoom});}
    const zoom=$('zoom'), custom=$('zoom-custom');
    $('zoom-selection').disabled=mode!=='edit'||!selectedId;
    if(camera.mode==='fit'){zoom.value='fit';return;}
    const value=String(camera.zoom);
    if([...zoom.options].some(option=>option.value===value)){zoom.value=value;return;}
    const label=Math.round(camera.zoom*100)+'%';
    if(custom.textContent!==label)custom.textContent=label;
    if(zoom.value!=='custom')zoom.value='custom';
  }
  function zoomAt(value,x,y){
    const area=$('canvas'), next=clampZoom(value);
    camera.x+=(x-area.clientWidth/2)*(1/scale-1/next);
    camera.y+=(y-area.clientHeight/2)*(1/scale-1/next);
    camera.mode='free';camera.zoom=next;cameraChanged();
  }
  function cameraInput(input,child=false){
    if(boot.inline){
      const r=frame.getBoundingClientRect();
      inlinePost('studio-inline-camera',{input:{...input,x:child?r.left+input.x*scale:input.x,y:child?r.top+input.y*scale:input.y}});
      return;
    }
    if(!cameraEnabled||!input||typeof input!=='object')return;
    if(input.kind==='end'){persistCamera();return;}
    if(!['pan','zoom','gesture'].includes(input.kind)||![input.x,input.y,input.dx,input.dy].every(n=>Number.isFinite(n)&&Math.abs(n)<1e6))return;
    const area=$('canvas'), bounds=area.getBoundingClientRect(), f=frame.getBoundingClientRect();
    const x=child?f.left-bounds.left+input.x*scale:input.x-bounds.left;
    const y=child?f.top-bounds.top+input.y*scale:input.y-bounds.top;
    if(input.kind==='pan'){
      camera.mode='free';camera.zoom=scale;camera.x-=input.dx/scale;camera.y-=input.dy/scale;cameraChanged();
    }else{
      const factor=input.kind==='gesture'?input.dy:Math.exp(-Math.max(-200,Math.min(200,input.dy))*.01);
      if(factor>0&&factor<=10)zoomAt(scale*factor,x,y);
    }
  }
  // This same small input adapter runs inside the opaque Preview frame. It can
  // only request camera movement; it cannot save or inspect the host document.
  function installCameraInput(doc,emit,accept=()=>true,panWheel=true,scrollPage=false){
    const win=doc.defaultView;
    let space=false, drag=null, gesture=0, suppressClick=false;
    const typing=target=>!!target?.closest?.('input,textarea,select,button,a,[contenteditable]:not([contenteditable="false"]),[role="textbox"]');
    const cursor=doc.createElement('style');doc.head.append(cursor);
    const updateCursor=()=>{cursor.textContent=space||drag?'*{cursor:'+(drag?'grabbing':'grab')+' !important}':'';};
    const stop=event=>{event.preventDefault();event.stopImmediatePropagation();};
    const end=()=>{if(drag)emit({kind:'end'});drag=null;updateCursor();};
    doc.addEventListener('keydown',event=>{
      if(event.key==='Escape'){space=false;end();}
      if(event.code==='Space'&&!typing(event.target)&&!event.metaKey&&!event.ctrlKey&&!event.altKey){space=true;updateCursor();event.preventDefault();}
    },true);
    doc.addEventListener('keyup',event=>{if(event.code==='Space'){space=false;end();}},true);
    win.addEventListener('blur',()=>{space=false;end();});
    doc.addEventListener('pointerdown',event=>{
      if(!accept(event)||(event.button!==1&&!(event.button===0&&space))){suppressClick=false;return;}
      stop(event);suppressClick=true;drag={id:event.pointerId,x:event.screenX,y:event.screenY};
      try{event.target.setPointerCapture(event.pointerId);}catch{}updateCursor();
    },true);
    doc.addEventListener('pointermove',event=>{
      if(!drag||event.pointerId!==drag.id)return;
      stop(event);
      // Screen coordinates remain stable while the iframe moves under the pointer.
      emit({kind:'pan',x:event.clientX,y:event.clientY,dx:event.screenX-drag.x,dy:event.screenY-drag.y});
      drag.x=event.screenX;drag.y=event.screenY;
    },true);
    doc.addEventListener('pointerup',event=>{if(drag){stop(event);end();}},true);
    doc.addEventListener('pointercancel',end,true);
    doc.addEventListener('lostpointercapture',end,true);
    doc.addEventListener('click',event=>{if(suppressClick){suppressClick=false;stop(event);}},true);
    // WebKit can leave native wheel scrolling inert inside a scaled, opaque
    // iframe even though selection auto-scroll works. Route prototype wheels to
    // the actual scroll containers, keeping the stage out of the scroll chain.
    const scrollContent=(target,dx,dy)=>{
      const root=doc.scrollingElement;
      for(let node=target?.nodeType===1?target:target?.parentElement;node;node=node.parentElement){
        const style=win.getComputedStyle(node);
        for(const [axis,delta] of [['x',dx],['y',dy]]){
          if(!delta)continue;
          const vertical=axis==='y',position=vertical?'scrollTop':'scrollLeft';
          const overflow=vertical?style.overflowY:style.overflowX;
          if(/^(hidden|clip)$/.test(overflow)||(node!==root&&!/^(auto|scroll|overlay)$/.test(overflow)))continue;
          const before=node[position];
          node.scrollTo({left:node.scrollLeft+(vertical?0:delta),top:node.scrollTop+(vertical?delta:0),behavior:'instant'});
          const containment=vertical?style.overscrollBehaviorY:style.overscrollBehaviorX;
          const remaining=/^(contain|none)$/.test(containment)?0:delta-(node[position]-before);
          if(vertical)dy=remaining;else dx=remaining;
        }
        if(!dx&&!dy)break;
      }
    };
    doc.addEventListener('wheel',event=>{
      if(!accept(event))return;
      const zoom=event.ctrlKey||event.metaKey;
      const unit=event.deltaMode===1?16:event.deltaMode===2?win.innerHeight:1;
      if(!panWheel&&!zoom){
        if(scrollPage&&event.cancelable){stop(event);if(!gesture)scrollContent(event.target,event.deltaX*unit,event.deltaY*unit);}
        return;
      }
      stop(event);if(gesture)return;
      emit({kind:zoom?'zoom':'pan',x:event.clientX,y:event.clientY,dx:-event.deltaX*unit,dy:(zoom?1:-1)*event.deltaY*unit});
    },{capture:true,passive:false});
    doc.addEventListener('gesturestart',event=>{if(accept(event)){stop(event);gesture=event.scale||1;}},{passive:false});
    doc.addEventListener('gesturechange',event=>{
      if(!gesture)return;stop(event);
      emit({kind:'gesture',x:event.clientX,y:event.clientY,dx:0,dy:event.scale/gesture});gesture=event.scale;
    },{passive:false});
    doc.addEventListener('gestureend',event=>{if(gesture){stop(event);gesture=0;emit({kind:'end'});}},{passive:false});
  }
  function changeZoom(){
    if(boot.inline){
      const value=$('zoom').value;
      const r=mode==='edit'&&selectedId?selected(frame.contentDocument)?.getBoundingClientRect():null;
      inlinePost('studio-inline-camera',{input:{kind:value==='selection'?'fit-selection':value==='fit'?'fit-screen':'preset',zoom:Number(value),rect:r?{x:r.x,y:r.y,width:r.width,height:r.height}:null}});
      return;
    }
    if(!cameraEnabled){size();return;}
    const value=$('zoom').value,area=$('canvas');
    if(value==='fit'){camera.mode='fit';cameraChanged();}
    else if(value==='selection'){
      const node=mode==='edit'&&selected(frame.contentDocument);
      if(node){const r=node.getBoundingClientRect();camera={mode:'free',zoom:clampZoom(Math.min((area.clientWidth-56)/Math.max(1,r.width),(area.clientHeight-56)/Math.max(1,r.height))),x:r.x+r.width/2,y:r.y+r.height/2};cameraChanged();}
      else syncZoom();
    }else if(Number(value)>0)zoomAt(Number(value),area.clientWidth/2,area.clientHeight/2);
    persistCamera();
  }
  restoreCamera();
  document.body.classList.toggle('camera-stage',cameraEnabled||!!boot.inline);
  document.body.classList.toggle('inline-canvas',!!boot.inline);
  document.body.classList.toggle('prototype-player',!!boot.prototype);
  if(cameraEnabled||boot.inline){
    // Backstop for the same chaining, for engines that do not honour overscroll
    // containment across a frame boundary. A player's own wheel gestures arrive
    // as messages, never as host events over the screen.
    installCameraInput(document,cameraInput,event=>!!event.target.closest?.('#canvas')&&!event.target.closest?.('.screen-comment-capture')&&!(event.type==='wheel'&&mode==='preview'&&!boot.system_specimen&&event.target.closest?.('#frame-wrap')));
    window.addEventListener('pagehide',persistCamera);
  }
  function size() {
    if(boot.inline){
      // Panning moves a single composited surface. It must not resize the
      // authored iframe or rebuild the zoom control on every wheel event.
      const r=$('canvas').getBoundingClientRect(),wrap=$('frame-wrap');
      scale=inlineView.zoom;
      wrap.style.left='0px';wrap.style.top='0px';
      wrap.style.transform='translate3d('+(inlineView.x-r.left)+'px,'+(inlineView.y-r.top)+'px,0)';
      const key=boot.width+':'+boot.height+':'+scale;
      if(key!==lastInlineSize){
        lastInlineSize=key;camera.mode='free';camera.zoom=scale;syncZoom();
        Object.assign(wrap.style,{width:(boot.width*scale)+'px',height:(boot.height*scale)+'px'});
        Object.assign($('frame-surface').style,{width:boot.width+'px',height:boot.height+'px',transform:'scale('+scale+')'});
        Object.assign(frame.style,{width:boot.width+'px',height:boot.height+'px'});
      }
      if(Vvveb.WysiwygEditor.isActive)outline();
      publishInlineLayout();publishCommentGeometry();return;
    }
    const area=$('canvas'), style=getComputedStyle(area);
    const availableWidth=area.clientWidth-parseFloat(style.paddingLeft)-parseFloat(style.paddingRight);
    const availableHeight=area.clientHeight-parseFloat(style.paddingTop)-parseFloat(style.paddingBottom);
    const fit=Math.min(1,availableWidth/boot.width,availableHeight/boot.height);
    if(cameraEnabled){
      if(camera.mode==='fit'){camera.zoom=clampZoom(fit);camera.x=boot.width/2;camera.y=boot.height/2;}
      scale=camera.zoom;
      Object.assign($('frame-wrap').style,{left:(area.clientWidth/2-camera.x*scale)+'px',top:(area.clientHeight/2-camera.y*scale)+'px'});
      syncZoom();
    }else scale=boot.thumbnail?1:($('zoom').value==='fit'?Math.max(.15,fit):Number($('zoom').value));
    Object.assign($('frame-wrap').style,{width:(boot.width*scale)+'px',height:(boot.height*scale)+'px'});
    Object.assign($('frame-surface').style,{width:boot.width+'px',height:boot.height+'px',transform:'scale('+scale+')'});
    Object.assign(frame.style,{width:boot.width+'px',height:boot.height+'px'});
    outline();publishInlineLayout();publishCommentGeometry();
  }
  function syncInspector() {
    const editing=mode==='edit'&&!boot.system_specimen&&!boot.thumbnail;
    const collapsed=editing&&inspectorCollapsed;
    document.body.classList.toggle('inspector-collapsed',collapsed);
    $('right-panel').hidden=!editing||collapsed;
    const toggle=$('inspector-toggle'), expanded=!collapsed;
    toggle.hidden=!editing;
    toggle.setAttribute('aria-expanded',String(expanded));
    const label=expanded?'Hide inspector':'Show inspector';
    toggle.setAttribute('aria-label',label);toggle.title=label;
    publishToolbar();
  }
  function toggleInspector() {
    if(mode!=='edit'||boot.system_specimen||boot.thumbnail)return;
    inspectorCollapsed=!inspectorCollapsed;
    try{localStorage.setItem(inspectorPreference,String(inspectorCollapsed));}catch{}
    syncInspector();size();
  }
  function render() {
    normalize(); selectedId=null; liveBaseline=null; $('outline').style.display='none'; $('wysiwyg-editor').style.display='none';
    document.body.classList.toggle('preview',mode==='preview');
    syncInspector();
    // Replace the frame so switching sandbox privileges never reuses its document.
    const next=document.createElement('iframe'); next.id='screen';next.title='Design screen';
    next.setAttribute('sandbox',boot.system_specimen?'allow-same-origin allow-scripts':mode==='edit'?'allow-same-origin allow-scripts':'allow-scripts');
    frame.replaceWith(next);frame=next;
    // Fix the child viewport before srcdoc starts parsing. Authored scripts and
    // media queries must never observe the host canvas width, even for one frame.
    size();
    frame.addEventListener('load',()=>{
      size();
      frame.contentWindow?.postMessage({ type: 'studio-comments-state', enabled: commenting }, '*');
      if(boot.thumbnail){
        const doc=frame.contentDocument;
        Promise.race([Promise.all([doc.fonts.ready,...[...doc.images].map(image=>image.decode().catch(()=>{}))]),new Promise(resolve=>setTimeout(resolve,1500))]).then(()=>setTimeout(()=>send({type:'thumbnail-ready'}),100));
        return;
      }
      if(boot.system_specimen){
        const doc=frame.contentDocument;
        const activate=event=>{const node=event.target.closest('[data-system-token],[data-system-recipe]');if(node){event.preventDefault();send(node.dataset.systemToken?{type:'system-token',token:node.dataset.systemToken}:{type:'system-recipe',recipe:node.dataset.systemRecipe});}};
        doc.addEventListener('click',activate);
        doc.addEventListener('keydown',event=>{if(event.key==='Enter'||event.key===' ')activate(event);});
        send({type:'system-ready'});
        return;
      }
      if(mode!=='edit')return;
      const doc=frame.contentDocument;
      if (commentAvailable) installCommentInput(doc, value => {
        if (value.type === 'studio-comment-toggle') send({ type: 'comment-toggle', request_id: uuid() });
        else { commentScroll = value; publishCommentGeometry(); }
      });
      installCameraInput(doc,input=>cameraInput(input,true));
      doc.addEventListener('keydown',historyShortcut,true);
      window.FrameDocument=doc;window.FrameWindow=frame.contentWindow;
      Vvveb.Builder.iframe=frame;Vvveb.Builder.frameBody=doc.body;Vvveb.Builder.selectedEl=null;
      Vvveb.StyleManager.styles={};Vvveb.StyleManager.currentElement=null;Vvveb.StyleManager.init(doc);
      if(!initializedToolbar){Vvveb.WysiwygEditor.init(doc);initializedToolbar=true;}
      Vvveb.WysiwygEditor.doc=doc;
      liveBaseline=editingSnapshot();
      doc.addEventListener('click',event=>{
        event.preventDefault();
        if(Vvveb.WysiwygEditor.isActive)return;
        const node=event.target.closest('[data-studio-id]');if(node)select(node.dataset.studioId);
      });
      doc.addEventListener('dblclick',event=>{
        event.preventDefault();const node=event.target.closest('[data-studio-id]');
        if(!node || node.namespaceURI!=='http://www.w3.org/1999/xhtml' || !editableTextTags.has(node.tagName))return;
        finishText();select(node.dataset.studioId);textEditBefore=JSON.stringify(source);
        Vvveb.WysiwygEditor.edit(node);outline();
      });
      doc.addEventListener('keydown',event=>{
        if(event.key==='Escape'){event.preventDefault();finishText();}
        if(event.key==='Enter' && !event.defaultPrevented && Vvveb.WysiwygEditor.isActive){event.preventDefault();doc.execCommand('insertLineBreak');}
        if((event.metaKey||event.ctrlKey)&&event.key==='s'){event.preventDefault();finishText();flush();}
      });
      doc.addEventListener('input',()=>syncFromLive(false));
      doc.addEventListener('paste',event=>{if(Vvveb.WysiwygEditor.isActive){event.preventDefault();doc.execCommand('insertText',false,event.clipboardData.getData('text/plain'));}});
      doc.addEventListener('scroll',outline,true);
      activatePoint();
    });
    frame.srcdoc=sanitize(parse(serialize()),mode==='preview'&&!boot.system_specimen);
    $('edit').setAttribute('aria-pressed',mode==='edit');$('preview').setAttribute('aria-pressed',mode==='preview');
  }
  let prototypeLoad = null;
  function preparePrototype(incoming) {
    if (!boot.prototype || incoming?.session !== boot.session || !incoming.prototype || !incoming.document
        || !Number.isInteger(incoming.width) || !Number.isInteger(incoming.height)
        || incoming.width < 240 || incoming.width > 3840 || incoming.height < 240 || incoming.height > 4096) return;
    if (prototypeLoad) { clearTimeout(prototypeLoad.timer); prototypeLoad.frame.remove(); }
    // Native navigation has already checked draft/request protection. Suspend
    // the clean comment layer until ready reattaches it to the destination.
    if (commenting) window.choroStudioReply({ session: boot.session, type: 'comment-mode', enabled: false });
    const next = document.createElement('iframe'), token = uuid();
    next.id = 'screen-pending'; next.title = 'Loading prototype screen';
    next.setAttribute('sandbox', 'allow-scripts');
    Object.assign(next.style, { position: 'absolute', left: '0px', top: '0px', width: incoming.width + 'px', height: incoming.height + 'px', border: '0', visibility: 'hidden', pointerEvents: 'none' });
    const pending = prototypeLoad = { frame: next, incoming, token, loaded: false, painted: false, timer: null };
    frame.style.pointerEvents = 'none';
    const fail = () => {
      if (prototypeLoad !== pending) return;
      clearTimeout(pending.timer); next.remove(); prototypeLoad = null;
      frame.style.pointerEvents = commenting ? 'none' : '';
      $('error').textContent = 'Could not open this screen. Try the link again.';
      send({ type: 'prototype-failed', screen_id: incoming.screen_id, previous_screen_id: boot.screen_id });
      send({ type: 'ready' });
    };
    next.addEventListener('error', fail, { once: true });
    next.addEventListener('load', () => { pending.loaded = true; commitPrototype(); }, { once: true });
    pending.timer = setTimeout(fail, 8000);
    const doc = parse(sanitize(parse(incoming.document.html), true, incoming));
    const ready = doc.createElement('script');
    ready.textContent = 'addEventListener("load",()=>{document.body.getBoundingClientRect();Promise.race([Promise.allSettled([document.fonts.ready,...[...document.images].map(image=>image.decode())]),new Promise(resolve=>setTimeout(resolve,500))]).then(()=>parent.postMessage({type:"studio-prototype-ready",token:' + JSON.stringify(token) + '},"*"))},{once:true});';
    doc.body.append(ready);
    frame.parentElement.append(next);
    next.srcdoc = '<!doctype html>' + doc.documentElement.outerHTML;
  }
  function commitPrototype() {
    const prepared = prototypeLoad;
    if (!prepared?.loaded || !prepared.painted) return;
    clearTimeout(prepared.timer); prototypeLoad = null;
    Object.assign(boot, prepared.incoming);
    source = { ...structuredClone(boot.document), overrides: structuredClone(boot.overrides || {}) };
    tokenCss = boot.tokens_css; assets = boot.assets || {};
    revision = boot.revision; fingerprint = boot.fingerprint;
    dirty = false; pending = false; savedText = JSON.stringify(source); normalize();
    commentScroll = { x: 0, y: 0 }; commentFocus = null;
    // Moving a loaded iframe reloads its browsing context in WebKit. Leave the
    // prepared frame in place and remove only the old one before revealing it.
    frame.remove(); frame = prepared.frame;
    frame.id = 'screen'; frame.title = 'Design screen';
    frame.style.visibility = ''; frame.style.position = ''; frame.style.left = ''; frame.style.top = '';
    frame.style.pointerEvents = commenting ? 'none' : '';
    $('error').textContent = '';
    if (boot.theme) window.choroStudioReply({ session: boot.session, type: 'theme', theme: boot.theme });
    restoreCamera(); size();
    frame.contentWindow?.postMessage({ type: 'studio-comments-state', enabled: commenting }, '*');
    send({ type: 'ready' });
  }
  function selected(doc=clean){return [...doc.querySelectorAll('[data-studio-id]')].find(n=>n.dataset.studioId===selectedId);}
  function select(id) {
    if(mode!=='edit')return;
    document.body.classList.remove('element-deselected');
    selectedId=id;const node=selected(frame.contentDocument);if(!node)return;
    Vvveb.Builder.selectedEl=node;
    const component=Vvveb.Components.matchNode(node)||Vvveb.Components.get('_base');
    try {Vvveb.Components.render(component.type);} catch(error) {send({type:'render-error',error:error.message+' '+error.stack});return;}
    if(cameraEnabled||boot.inline)syncZoom();
    outline();send({type:'selection',element:id});
  }
  function outline(){
    if(mode!=='edit'||!frame.contentDocument)return;
    const node=Vvveb.WysiwygEditor.isActive?Vvveb.WysiwygEditor.element:selected(frame.contentDocument);if(!node)return;
    const r=node.getBoundingClientRect();Object.assign($('outline').style,{display:'block',left:r.x+'px',top:r.y+'px',width:r.width+'px',height:r.height+'px'});
    if(Vvveb.WysiwygEditor.isActive){
      const canvas=$('canvas').getBoundingClientRect(), f=frame.getBoundingClientRect(), toolbar=$('wysiwyg-editor');
      const left=Math.max(canvas.left+4,Math.min(f.left+r.x*scale,canvas.right-toolbar.offsetWidth-4));
      const top=Math.max(canvas.top+4,f.top+r.y*scale-toolbar.offsetHeight-6);
      Object.assign(toolbar.style,{left:left+'px',top:top+'px'});
    }
  }
  function editingSnapshot(){
    const body=frame.contentDocument.body.cloneNode(true);
    for(const node of [body,...body.querySelectorAll('*')]){
      node.removeAttribute('contenteditable');node.removeAttribute('spellcheckker');
    }
    return body;
  }
  // Apply only DOM differences produced in Edit. Unchanged authored attributes,
  // local asset paths and scripts stay in the clean source model.
  function mergeNode(target,before,after){
    if(before.isEqualNode(after))return target;
    if(before.nodeType!==after.nodeType || before.nodeName!==after.nodeName){const replacement=clean.importNode(after,true);target.replaceWith(replacement);return replacement;}
    if(after.nodeType!==1){target.nodeValue=after.nodeValue;return target;}
    for(const name of new Set([...before.getAttributeNames(),...after.getAttributeNames()])){
      if(before.getAttribute(name)===after.getAttribute(name))continue;
      if(after.hasAttribute(name)){const value=after.getAttribute(name);target.setAttribute(name,name==='src'?(Object.keys(assets).find(path=>assets[path]===value)||value):value);}else target.removeAttribute(name);
    }
    const old=[...before.childNodes], next=[...after.childNodes], originals=[...target.childNodes], used=new Set();
    const mapped=old.map((node,index)=>{
      const id=node.nodeType===1?node.getAttribute('data-studio-id'):null;
      const match=id?originals.find(n=>n.nodeType===1&&n.getAttribute('data-studio-id')===id):originals.find(n=>!used.has(n)&&n.nodeType===node.nodeType&&n.nodeName===node.nodeName&&n.nodeValue===node.nodeValue);
      if(match)used.add(match);return match;
    });
    const aligned=old.length===next.length&&old.every((node,i)=>node.nodeType===next[i].nodeType&&(node.nodeType!==1||node.getAttribute('data-studio-id')===next[i].getAttribute('data-studio-id')));
    if(aligned){next.forEach((node,i)=>{if(mapped[i])mergeNode(mapped[i],old[i],node);});return target;}
    const children=next.map(node=>{
      const id=node.nodeType===1?node.getAttribute('data-studio-id'):null;
      const index=id?old.findIndex(n=>n.nodeType===1&&n.getAttribute('data-studio-id')===id):old.findIndex(n=>n.isEqualNode(node));
      return index>=0&&mapped[index]?mergeNode(mapped[index],old[index],node):clean.importNode(node,true);
    });
    // Hidden authored nodes (for example inert scripts) are absent from Edit,
    // so preserve them when an adjacent text node gains inline formatting.
    target.replaceChildren(...children,...originals.filter(node=>!used.has(node)));
    return target;
  }
  function syncFromLive(recordUndo=true){
    if(mode!=='edit'||!liveBaseline)return;
    const before=propertyBefore||JSON.stringify(source), current=editingSnapshot();propertyBefore=null;
    const rulesNow=frame.contentDocument.getElementById('vvvebjs-styles')?.textContent||'';
    if(liveBaseline.isEqualNode(current) && rulesNow===(clean.querySelector('style[data-studio-rules]')?.textContent||'') && before===JSON.stringify(source))return;
    mergeNode(clean.body,liveBaseline,current);liveBaseline=current;
    const rules=frame.contentDocument.getElementById('vvvebjs-styles')?.textContent||'';
    let style=clean.querySelector('style[data-studio-rules]');
    if(rules){if(!style){style=clean.createElement('style');style.dataset.studioRules='';clean.head.append(style);}style.textContent=rules;}
    else style?.remove();
    source.html=serialize();
    const after=JSON.stringify(source);
    if(before===after)return;
    if(recordUndo&&!Vvveb.WysiwygEditor.isActive)addUndo({type:'studio',oldValue:before,newValue:after});
    markDirty();outline();
  }
  function finishText(){
    if(!Vvveb.WysiwygEditor.isActive)return;
    const before=textEditBefore;syncFromLive(false);
    Vvveb.WysiwygEditor.destroy(Vvveb.WysiwygEditor.element);textEditBefore=null;
    if(before&&before!==JSON.stringify(source))addUndo({type:'studio',oldValue:before,newValue:JSON.stringify(source)});
    if(selectedId)select(selectedId);
  }
  function historyShortcut(event){
    if(mode!=='edit'||boot.thumbnail||boot.system_specimen||event.defaultPrevented||event.isComposing||event.altKey||!(event.metaKey||event.ctrlKey))return;
    // Inspector fields own their text history. Authored inline text instead
    // commits one edit before using the same document history as the buttons.
    if(event.target?.closest?.('input,textarea,select')||
       (event.target?.ownerDocument===document&&event.target?.isContentEditable))return;
    const key=event.key.toLowerCase();
    const undo=key==='z'||event.code==='KeyZ';
    const redo=!event.shiftKey&&(key==='y'||event.code==='KeyY');
    if(!undo&&!redo)return;
    event.preventDefault();event.stopPropagation();
    $(redo||event.shiftKey?'redo':'undo').click();
  }
  function markDirty(){dirty=JSON.stringify(source)!==savedText;setStatus(dirty?'Unsaved':'Saved');send({type:'dirty',dirty,document:source,revision,fingerprint});clearTimeout(timer);if(dirty)timer=setTimeout(flush,500);}
  function flush(){clearTimeout(timer);if(pending||!dirty)return;pending=true;saveId=uuid();inFlightText=JSON.stringify(source);setStatus('Saving…');send({type:'save',id:saveId,revision,fingerprint,document:JSON.parse(inFlightText)});}
  function sendFlushed(){send({type:'flushed',request_id:flushRequest});flushRequest=null;}
  window.choroStudioReply=reply=>{
    if(reply.session!==boot.session)return;
    if (reply.type === 'prototype-screen') { preparePrototype(reply.bootstrap); return; }
    if (reply.type === 'comment-mode' && commentAvailable) {
      if (prototypeLoad) return;
      commenting = !!reply.enabled;
      if (!commenting) commentFocus = null;
      window.choroCommentsEnabled = commenting;
      window.choroCommentSelection = reply.selected ?? null;
      if (commenting && mode === 'edit') { finishText(); flush(); }
      frame.contentWindow?.postMessage({ type: 'studio-comments-state', enabled: commenting }, '*');
      if (boot.inline) { frame.style.pointerEvents = commenting ? 'none' : ''; }
      else {
        if (commenting && !window.choroCommentsLoaded) {
          const bundle = $('studio-comments-bundle');
          if (bundle?.textContent.trim()) {
            window.choroCommentsLoaded = true;
            const script = document.createElement('script'); script.textContent = bundle.textContent; document.body.append(script);
          }
        }
        window.dispatchEvent(new CustomEvent('studio-comment-mode', { detail: { enabled: commenting } }));
      }
      publishCommentGeometry(); return;
    }
    if (reply.type === 'comments-result' && commenting) {
      window.dispatchEvent(new CustomEvent('choro-canvas-comment', { detail: reply })); return;
    }
    // Only the trusted host can invoke these controls; authored screens never
    // reach this reply channel. Keep one implementation of each editor action.
    if(reply.type==='toolbar-command'&&boot.native_toolbar&&mode==='edit'){
      const id={undo:'undo',redo:'redo',inspector:'inspector-toggle',retry:'retry',recover:'recover'}[reply.command];
      if(typeof id==='string'&&(!['retry','recover'].includes(reply.command)||(!pending&&!$('recover').hidden))){$(id).click();publishToolbar();}
      return;
    }
    if(reply.type==='activate'&&boot.inline){if(Number.isFinite(reply.x)&&Number.isFinite(reply.y)&&reply.x>=0&&reply.y>=0&&reply.x<=boot.width&&reply.y<=boot.height){activationPoint={x:reply.x,y:reply.y};activatePoint();}return;}
    if(reply.type==='deselect'&&boot.inline){finishText();selectedId=null;Vvveb.Builder.selectedEl=null;$('outline').style.display='none';document.body.classList.add('element-deselected');send({type:'selection',element:null});return;}
    if(reply.type==='camera-command'&&boot.prototype){if(reply.command==='fit'){camera.mode='fit';cameraChanged();}else if(['zoom-in','zoom-out'].includes(reply.command)){const area=$('canvas');zoomAt(camera.zoom*(reply.command==='zoom-in'?1.2:1/1.2),area.clientWidth/2,area.clientHeight/2);}return;}
    if(reply.type==='theme'&&reply.theme){
      for(const [key,value] of Object.entries(reply.theme))document.documentElement.style.setProperty('--'+key,String(value));
      const scheme=reply.theme.scheme==='light'?'light':'dark';
      for(const node of [document.documentElement,document.body])node.dataset.bsTheme=scheme;
      Coloris({themeMode:scheme});return;
    }
    if(reply.type==='viewport'&&!boot.system_specimen){if(Number.isInteger(reply.width)&&Number.isInteger(reply.height)&&reply.width>=240&&reply.width<=3840&&reply.height>=240&&reply.height<=4096){persistCamera();boot.width=reply.width;boot.height=reply.height;restoreCamera();size();}return;}
    if(reply.type==='system-section'&&boot.system_specimen){const section=frame.contentDocument?.getElementById('system-'+reply.section);if(section){$('canvas').scrollTop=section.offsetTop*scale;frame.contentWindow.scrollTo(0,0);}return;}
    if(reply.type==='screens'){
      boot.screens=reply.screens.filter(screen=>!screen.archived);
      for(const component of Object.values(Vvveb.Components._components)){
        const property=component.properties.find(p=>p.key==='data-studio-screen');
        if(property)property.data.options=[{value:'',text:'No screen link'},...boot.screens.map(s=>({value:s.id,text:s.name}))];
      }
      return;
    }
    if(reply.type==='flush'){persistCamera();flushRequest=reply.request_id||flushRequest;finishText();flush();if(!dirty&&!pending)sendFlushed();return;}
    if(reply.id!==saveId)return;
    pending=false;
    if(reply.error){$('error').textContent=reply.error;setStatus('Save failed');$('retry').style.display='inline-block';$('recover').hidden=false;return;}
    revision=reply.revision;fingerprint=reply.fingerprint;savedText=inFlightText;$('error').textContent='';$('retry').style.display='none';$('recover').hidden=true;markDirty();if(!dirty)sendFlushed();else if(flushRequest)flush();
  };
  const upstreamAddUndo=Vvveb.Undo.addMutation.bind(Vvveb.Undo);
  const addUndo=mutation=>{upstreamAddUndo(mutation);publishToolbar();};
  const restore=Vvveb.Undo.restore;
  Vvveb.Undo.addMutation=function(mutation){if(mutation.type==='studio')return addUndo(mutation);if(mutation.type!=='characterData')queueMicrotask(()=>syncFromLive(true));};
  Vvveb.Undo.restore=function(mutation,undo){if(mutation.type!=='studio')return restore.call(this,mutation,undo);source=JSON.parse(undo?mutation.oldValue:mutation.newValue);render();markDirty();};
  // Keep upstream renderers and inline commands. Adapt their host boundaries.
  Vvveb.Builder.selectNode=node=>{if(!node?.dataset)return;if(!node.dataset.studioId)node.dataset.studioId='element-'+uuid();selectedId=node.dataset.studioId;Vvveb.Builder.selectedEl=node;outline();};
  Vvveb.Builder.loadNodeComponent=node=>{if(node?.dataset.studioId)select(node.dataset.studioId);};
  Vvveb.Builder._selectNode=Vvveb.Builder.selectNode;
  Vvveb.StyleManager.getSelectorForElement=node=>typeof node==='string'?node:'[data-studio-id="'+CSS.escape(node.dataset.studioId)+'"]';
  // Capture the whole change before upstream controls mutate DOM or tokens.
  // Capture also covers component callbacks that do not emit an Undo mutation.
  $('right-panel').addEventListener('propertyChange',()=>{
    propertyBefore ||= JSON.stringify(source);queueMicrotask(()=>syncFromLive(true));
  },true);
  const setStyle=Vvveb.StyleManager.setStyle.bind(Vvveb.StyleManager);
  Vvveb.StyleManager.setStyle=(node,property,value)=>{
    propertyBefore ||= JSON.stringify(source);
    if(value && !value.startsWith('var(--') && !/url\(/i.test(value)){
      const token=('custom-'+boot.screen_id+'-'+node.dataset.studioId+'-'+property).toLowerCase().replace(/[^a-z0-9-]/g,'-');
      source.overrides[token]=value;value='var(--'+token+')';
      frame.contentDocument.documentElement.style.setProperty('--'+token,source.overrides[token]);
    }
    return setStyle(node,property,value);
  };
  const getStyle=Vvveb.StyleManager.getStyle.bind(Vvveb.StyleManager);
  Vvveb.StyleManager.getStyle=(node,property)=>{const value=getStyle(node,property);return value?.startsWith('var(--')?frame.contentWindow.getComputedStyle(node).getPropertyValue(property):value;};
  // Upstream text/HTML controls operate only on inert Edit content.
  const setHtml=Vvveb.ContentManager.setHtml.bind(Vvveb.ContentManager);
  Vvveb.ContentManager.setHtml=(node,value,outer)=>{
    const fragment=parse(value);fragment.querySelectorAll('script,iframe,object,embed,base,meta,link').forEach(n=>n.remove());
    for(const n of fragment.querySelectorAll('*'))for(const attr of [...n.attributes])if(/^on/i.test(attr.name)||['srcdoc','formaction','action'].includes(attr.name))n.removeAttribute(attr.name);
    return setHtml(node,fragment.body.innerHTML,outer);
  };
  Vvveb.Components.getProperty('html/image','src').onChange=(node,value)=>assetUrl(value);
  ImageInput.init=function(data){return this.render('imageinput',data);};
  ImageInput.setValue=function(value){this.element[0].querySelector('input[type=text]').value=Object.keys(assets).find(path=>assets[path]===value)||value||'';};
  ImageInput.onUpload=async function(event,control){
    const file=this.files?.[0];if(!file)return;
    if(dirty||pending){$('error').textContent='Wait for the current edit to save, then add the image.';return;}
    if(file.size>4*1024*1024){$('error').textContent='Choose an image smaller than 4 MB.';return;}
    const targetId=selectedId, expectedRevision=revision, expectedFingerprint=fingerprint;
    const data=await new Promise((resolve,reject)=>{const reader=new FileReader();reader.onload=()=>resolve(reader.result);reader.onerror=reject;reader.readAsDataURL(file);});
    if(dirty||pending||revision!==expectedRevision||fingerprint!==expectedFingerprint){$('error').textContent='The screen changed while reading the image. Choose the image again.';return;}
    const extension=file.name.split('.').pop().toLowerCase();
    if(!['png','jpg','jpeg','gif','webp','svg'].includes(extension)){$('error').textContent='Choose a PNG, JPEG, GIF, WebP, or SVG image.';return;}
    const name=uuid()+'.'+extension, doc=clean.cloneNode(true);
    const target=[...doc.querySelectorAll('[data-studio-id]')].find(node=>node.dataset.studioId===targetId);
    if(!target)return;
    if(control.querySelector('input[type=text]').name==='src')target.setAttribute('src','assets/'+name);
    else target.style.backgroundImage='url("assets/'+name+'")';
    send({type:'asset',data_url:data,extension,name,revision,fingerprint,document:{...source,html:'<!doctype html>\n'+doc.documentElement.outerHTML}});
  };
  // Extend the actual upstream property registry with Studio's local screen link.
  for(const component of Object.values(Vvveb.Components._components)){
    component.properties.push({name:'Screen link',key:'data-studio-screen',htmlAttr:'data-studio-screen',inputtype:SelectInput,sort:5,data:{options:[{value:'',text:'No screen link'},...boot.screens.filter(s=>!s.archived).map(s=>({value:s.id,text:s.name}))]}});
  }
  const inlineSetStyle=Vvveb.WysiwygEditor.editorSetStyle;
  Vvveb.WysiwygEditor.editorSetStyle=function(...args){const result=inlineSetStyle.apply(this,args);syncFromLive(false);return result;};
  $('wysiwyg-editor').addEventListener('mousedown',event=>{if(event.target.closest('a,button'))event.preventDefault();});
  document.addEventListener('pointerdown',event=>{if(!event.target.closest('#wysiwyg-editor,.clr-picker') && event.target!==frame)finishText();},true);
  $('canvas').addEventListener('scroll',outline);
  document.querySelector('[data-vvveb-action="setState"]').addEventListener('change',event=>{Vvveb.StyleManager.setState(event.target.value);if(selectedId)select(selectedId);});
  // Upstream's empty inspector is a dismissible alert. Keep its node, give it direction.
  const emptyInspector=document.querySelector('#content-tab > .alert');
  if(emptyInspector){const title=document.createElement('strong'),hint=document.createElement('span');title.textContent='Nothing selected';hint.textContent='Click an element on the screen to edit its content and style.';emptyInspector.replaceChildren(title,hint);emptyInspector.classList.remove('alert-dismissible');}
  if(emptyInspector){const empty=emptyInspector.cloneNode(true);empty.id='studio-empty-selection';$('right-panel').append(empty);}
  const scheme=boot.theme?.scheme==='light'?'light':'dark';
  Coloris({el:'.coloris',theme:'polaroid',themeMode:scheme,format:'hex',alpha:true});
  $('undo').onclick=()=>{finishText();Vvveb.Undo.undo();publishToolbar();};$('redo').onclick=()=>{finishText();Vvveb.Undo.redo();publishToolbar();};
  window.addEventListener('keydown',historyShortcut,true);
  function setMode(next){
    if(boot.system_specimen||boot.inline||boot.prototype||mode===next)return;
    finishText();flush();mode=next;send({type:'mode',mode});render();
  }
  $('edit').onclick=()=>setMode('edit');$('preview').onclick=()=>setMode('preview');
  $('inspector-toggle').onclick=toggleInspector;
  $('zoom').onchange=changeZoom;$('retry').onclick=flush;$('recover').onclick=()=>send({type:'recover'});
  window.addEventListener('resize',size);
  new ResizeObserver(size).observe($('canvas'));
  window.addEventListener('message',event=>{if(event.source===frame.contentWindow&&event.data?.type==='studio-camera')cameraInput(event.data.input,true);});
  window.addEventListener('message', event => {
    if (prototypeLoad && event.source === prototypeLoad.frame.contentWindow && event.data?.type === 'studio-prototype-ready' && event.data.token === prototypeLoad.token) {
      prototypeLoad.painted = true; commitPrototype(); return;
    }
    if (event.source !== frame.contentWindow) return;
    if (event.data?.type === 'studio-comment-toggle' && commentAvailable) send({ type: 'comment-toggle', request_id: uuid() });
    if (commenting && event.data?.type === 'studio-comment-scroll' && [event.data.x, event.data.y].every(Number.isFinite)) {
      commentScroll = event.data;
      if (commentFocus && commentFocus.focus === event.data.focus) { const pin = commentFocus; commentFocus = null; revealComment(pin); }
      publishCommentGeometry();
    }
  });
  window.addEventListener('message',event=>{if(!commenting&&!prototypeLoad&&event.source===frame.contentWindow&&event.data?.type==='studio-navigate'&&boot.screens.some(s=>s.id===event.data.screen_id))send({type:'navigate',screen_id:event.data.screen_id});});
  window.addEventListener('keydown',event=>{if((event.metaKey||event.ctrlKey)&&event.key==='s'){event.preventDefault();flush();}});
  if (commentAvailable && !boot.inline) window.addEventListener('keydown', event => {
    if (!event.defaultPrevented && !event.repeat && !event.isComposing && !event.metaKey && !event.ctrlKey && !event.altKey && event.key.toLowerCase() === 'c'
        && !event.target.closest?.('input,textarea,select,[contenteditable]:not([contenteditable="false"])')) {
      event.preventDefault(); send({ type: 'comment-toggle', request_id: uuid() });
    }
  });
  if(boot.theme)for(const [key,value] of Object.entries(boot.theme))document.documentElement.style.setProperty('--'+key,value);
  for(const node of [document.documentElement,document.body])node.dataset.bsTheme=scheme;
  document.body.classList.toggle('native-toolbar',!!boot.native_toolbar);
  if(boot.system_specimen&&!boot.thumbnail){mode='preview';for(const id of ['edit','preview','undo','redo'])$(id).hidden=true;}
  if(boot.thumbnail){document.querySelector('header').style.display='none';$('right-panel').style.display='none';document.body.style.gridTemplateRows='1fr';$('layout').style.gridTemplateColumns='1fr';$('canvas').style.padding='0';$('canvas').style.overflow='hidden';}
  if(boot.inline){$('zoom').querySelector('[value="4"]').disabled=true;}
  if(boot.inline||boot.prototype){
    const caption=$('session-label');caption.hidden=false;caption.textContent=(boot.screens.find(s=>s.id===boot.screen_id)?.name||'Screen');
  }
  if(boot.inline){
    window.addEventListener('message',event=>{
      if(event.source!==parent||event.data?.session!==boot.session)return;
      if(event.data.type==='studio-reply')window.choroStudioReply(event.data.reply);
      if(event.data.type==='studio-camera'&&[event.data.x,event.data.y,event.data.zoom].every(Number.isFinite)&&event.data.zoom>=.02&&event.data.zoom<=4){inlineView=event.data;inlineHasCamera=true;size();}
    });
    let layoutTick;
    const observe=new MutationObserver(()=>{cancelAnimationFrame(layoutTick);layoutTick=requestAnimationFrame(publishInlineLayout);});
    for(const id of ['studio-toolbar','right-panel','wysiwyg-editor','error','clr-picker'])if($(id))observe.observe($(id),{attributes:true,childList:true,subtree:true});
  }
  render();send({type:'ready'});if(boot.draft)markDirty();
})();
