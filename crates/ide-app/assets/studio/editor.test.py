#!/usr/bin/env python3
"""Exercise the actual editor in the bundled, isolated WebKit helper. No server."""
import json, pathlib, subprocess, sys, tempfile, time
root = pathlib.Path(__file__).resolve().parent
helper_args = [arg for arg in sys.argv[1:] if not arg.startswith('--')]
helper = pathlib.Path(helper_args[0]) if helper_args else next(pathlib.Path('target/debug/build').glob('ide-app-*/out/choro-studio-thumbnail'))
checks = r'''
(async()=>{
 Object.defineProperty(window,"__testPhase",{set(value){window.ipc.postMessage(JSON.stringify({type:"phase",phase:value}));}});window.__testPhase="start";
 window.testErrors=[];window.addEventListener("error",e=>{window.testErrors.push(e.message);e.stopImmediatePropagation();},true);
 // MessageChannel avoids background timer throttling in the offscreen test window.
 const wait=async(predicate,label)=>{window.__testPhase=label;const start=performance.now();while(performance.now()-start<8000){if(predicate())return;await new Promise(resolve=>{const channel=new MessageChannel();channel.port1.onmessage=()=>{channel.port1.close();channel.port2.close();resolve();};channel.port2.postMessage(null);});}throw Error('Timed out: '+label+'; '+JSON.stringify(window.testErrors));};
 const assert=(condition,label)=>{if(!condition)throw Error(label);};
 window.addEventListener('unhandledrejection',e=>window.ipc.postMessage(JSON.stringify({type:'render-error',error:String(e.reason)})));
 const originalSend=window.ipc.postMessage;
 const messages=[];let autoAck=true;
 window.ipc.postMessage=raw=>{const value=JSON.parse(raw);messages.push(value);if(value.type==='save'&&autoAck)queueMicrotask(()=>window.choroStudioReply({session:'test',id:value.id,revision:value.revision+1,fingerprint:'next'}));else if(value.type!=='save')originalSend(raw);};
 const frame=()=>document.getElementById('screen');
 const close=(left,right)=>Math.abs(left-right)<1;
 const assertFrameBounds=(width,height,label)=>{
   const wrapper=document.getElementById('frame-wrap').getBoundingClientRect(), screen=frame().getBoundingClientRect();
   assert(close(wrapper.width,screen.width)&&close(wrapper.height,screen.height),label+' wrapper must match the visible frame: '+JSON.stringify({wrapper:[wrapper.width,wrapper.height],screen:[screen.width,screen.height]}));
   const surface=document.getElementById('frame-surface');
   assert(surface.offsetWidth===width&&surface.offsetHeight===height,label+' surface keeps authored dimensions');
 };
 // Every visible toolbar control must sit fully inside the toolbar and the host, without overlap.
 const assertToolbarReachable=(label)=>{
   const bar=document.getElementById('studio-toolbar').getBoundingClientRect(), host=document.body.getBoundingClientRect();
   const rects=['edit','preview','undo','redo','status','retry','recover','zoom','inspector-toggle'].map(id=>[id,document.getElementById(id)]).filter(([,node])=>node.getClientRects().length).map(([id,node])=>[id,node.getBoundingClientRect()]);
   for(const [id,r] of rects)assert(r.width>=6&&r.height>=6&&r.left>=bar.left-.5&&r.right<=Math.min(bar.right,host.right)+.5&&r.top>=bar.top-.5&&r.bottom<=bar.bottom+.5,label+': '+id+' is clipped or offscreen '+JSON.stringify({control:[r.left,r.top,r.right,r.bottom],bar:[bar.left,bar.top,bar.right,bar.bottom]}));
   for(let i=0;i<rects.length;i++)for(let j=i+1;j<rects.length;j++){const a=rects[i][1],b=rects[j][1];assert(a.right<=b.left+.5||b.right<=a.left+.5||a.bottom<=b.top+.5||b.bottom<=a.top+.5,label+': '+rects[i][0]+' overlaps '+rects[j][0]);}
   return rects.map(([id])=>id);
 };
 const narrowHost=async(width,label)=>{document.body.style.width=width?width+'px':'';await wait(()=>Math.abs(document.getElementById('studio-toolbar').getBoundingClientRect().width-(width||innerWidth))<1,label);};
 const layoutMode=()=>getComputedStyle(frame().contentDocument.body).getPropertyValue('--fixture-layout').trim();
 let prototypeResult;
 window.addEventListener('message',e=>{if(e.source===frame().contentWindow&&e.data?.type==='prototype-result')prototypeResult=e.data;});
 const title=()=>Vvveb.Builder.iframe===frame()?frame().contentDocument?.querySelector('h1'):null;
 const click=()=>title().dispatchEvent(new MouseEvent('click',{bubbles:true}));
 if(window.__CHORO_STUDIO__.prototype){
   const KEY='choro.studio.camera.v1:test-screen:800x600', SEEDED=localStorage.getItem(KEY);
   // The player frame is opaque (allow-scripts only), so readiness arrives by message.
   await wait(()=>prototypeResult,'prototype screen');
   await wait(()=>frame().getBoundingClientRect().width>0,'prototype fit');
   // Navigation rebuilds the player per screen. If a saved camera were restored
   // here, every screen of a flow would land somewhere different.
   assert(document.getElementById('zoom').value==='fit','The player opens fitted, ignoring a camera saved for this screen');
   const fitted=frame().getBoundingClientRect();
   const area=document.getElementById('canvas').getBoundingClientRect();
   assert(close((fitted.left+fitted.right)/2,(area.left+area.right)/2),'A fitted screen is centred, so every screen of a flow lands in the same place');
   document.getElementById('canvas').dispatchEvent(new WheelEvent('wheel',{bubbles:true,cancelable:true,ctrlKey:true,clientX:400,clientY:300,deltaY:-40}));
   await wait(()=>frame().getBoundingClientRect().width>fitted.width,'prototype ctrl-wheel zoom');
   // Zooming still works; it just never outlives the screen you did it on.
   window.dispatchEvent(new Event('pagehide'));
   assert(localStorage.getItem(KEY)===SEEDED,'The player never writes a per-screen camera');
   assert(window.testErrors.length===0,'Unexpected errors: '+JSON.stringify(window.testErrors));
   originalSend(JSON.stringify({type:'thumbnail-ready'}));
   return;
 }
 if(window.__CHORO_STUDIO__.system_specimen){
   assert(document.body.classList.contains('preview'),'System specimen hides editing inspector');
   assert(frame().getAttribute('sandbox')==='allow-same-origin allow-scripts','System uses the Edit isolation boundary for host selection');
   await wait(()=>messages.some(m=>m.type==='system-ready'),'system specimen loaded');
   assert(frame().contentDocument.querySelector('script')===null,'System specimen never executes prototype code');
   assert(frame().contentDocument.querySelector('meta[http-equiv]').content.includes("script-src 'none'"),'System CSP blocks authored scripts');
   for(const id of ['edit','preview','undo','redo','inspector-toggle'])assert(document.getElementById(id).hidden,'System hides document editing control '+id);
   document.getElementById('edit').click();
   assert(document.body.classList.contains('preview'),'A system cannot enter screen editing');
   assert(!messages.some(m=>m.type==='save'||m.type==='dirty'),'Opening a system does not write screen documents');
   if(frame().contentDocument.getElementById('system-colors')){window.choroStudioReply({session:window.__CHORO_STUDIO__.session,type:'system-section',section:'colors'});assert(document.getElementById('canvas').scrollTop>0,'Library section navigation scrolls the specimen');window.choroStudioReply({session:window.__CHORO_STUDIO__.session,type:'system-section',section:'typography'});}
   const swatch=frame().contentDocument.querySelector('[data-system-token]');
   if(swatch){swatch.click();assert(messages.some(m=>m.type==='system-token'),'Selecting a specimen opens native token controls');}
   const recipe=frame().contentDocument.querySelector('[data-system-recipe]');
   if(recipe){recipe.click();assert(messages.some(m=>m.type==='system-recipe'),'Selecting a component opens native recipe controls');}
   originalSend(JSON.stringify({type:'thumbnail-ready'}));return;
 }
 if(window.__CHORO_STUDIO__.mode==='preview'){
   assert(document.body.classList.contains('preview'),'Restored Preview starts without an inspector');
   assert(document.getElementById('inspector-toggle').hidden,'Restored Preview hides the editing-only inspector control');
   assert(frame().getAttribute('sandbox')==='allow-scripts','Restored Preview keeps script isolation');
   await wait(()=>prototypeResult,'restored preview prototype');
   assert(prototypeResult.viewportWidth===window.__CHORO_STUDIO__.width,'Preview is born at the authored viewport, not the host width: '+prototypeResult.viewportWidth);
   assert(prototypeResult.desktopLayout,'Preview media queries use the authored desktop viewport before load');
   assertFrameBounds(window.__CHORO_STUDIO__.width,window.__CHORO_STUDIO__.height,'Restored Preview');
   prototypeResult=undefined;
   document.getElementById('edit').click();
   assert(messages.some(m=>m.type==='mode'&&m.mode==='edit'),'Edit choice is sent to the host');
 }
 await wait(()=>title()?.textContent==='Original','initial screen');
 if(window.__CHORO_STUDIO__.testHistoryShortcuts){
   const key=(target,options)=>{const event=new KeyboardEvent('keydown',{key:'z',code:'KeyZ',bubbles:true,cancelable:true,...options});target.dispatchEvent(event);return event;};
   const edit=value=>{
     const node=title(),doc=frame().contentDocument;
     node.dispatchEvent(new MouseEvent('dblclick',{bubbles:true,cancelable:true}));
     const range=doc.createRange(),selection=frame().contentWindow.getSelection();
     range.selectNodeContents(node);selection.removeAllRanges();selection.addRange(range);
     assert(doc.execCommand('insertText',false,value),'Type an undoable text edit');
   };
   const flush=async id=>{window.choroStudioReply({session:'test',type:'flush',request_id:id});await wait(()=>messages.some(m=>m.type==='flushed'&&m.request_id===id),id);};
   // The shortcut must finish active text and undo that edit, not a prior one.
   edit('Active typing');
   assert(key(title(),{metaKey:true}).defaultPrevented,'Cmd+Z is handled by Studio');
   await wait(()=>title()?.textContent==='Original','undo active text');
   assert(key(document.body,{metaKey:true,shiftKey:true,key:'Z'}).defaultPrevented,'Cmd+Shift+Z is handled by Studio');
   await wait(()=>title()?.textContent==='Active typing','redo active text');
   key(document.body,{ctrlKey:true});await wait(()=>title()?.textContent==='Original','Ctrl+Z');
   // Twelve separately saved edits exceed the requested ten-step history.
   for(let i=1;i<=12;i++){edit('Edit '+i);await flush('history-edit-'+i);}
   for(let i=11;i>=0;i--){
     const target=i%2?frame().contentDocument.body:document.body;
     key(target,i%2?{ctrlKey:true}:{metaKey:true});
     await wait(()=>title()?.textContent===(i?'Edit '+i:'Original'),'undo step '+(12-i));
     assert(frame().contentDocument.querySelector('p').textContent==='Unchanged neighbor','Undo keeps neighboring text');
   }
   for(let i=1;i<=12;i++){
     const options=i%3===0?{ctrlKey:true,key:'y',code:'KeyY'}:i%2?{metaKey:true,shiftKey:true}:{ctrlKey:true,shiftKey:true};
     key(document.body,options);await wait(()=>title()?.textContent==='Edit '+i,'redo step '+i);
   }
   await flush('history-redone');
   assert(messages.filter(m=>m.type==='save').at(-1).document.html.includes('Edit 12'),'Redo persists through the existing save path');
   // Inspector typing uses its own native field history, not the design stack.
   click();const input=document.querySelector('#right-panel input[name=color]');
   const index=Vvveb.Undo.undoIndex;
   assert(!key(input,{metaKey:true}).defaultPrevented&&Vvveb.Undo.undoIndex===index,'Inspector field keeps native undo');
   input.value='#aa2244';input.dispatchEvent(new Event('change',{bubbles:true}));
   await wait(()=>Vvveb.Undo.undoIndex===index+1,'property edit history');
   assert(key(document.getElementById('right-panel'),{ctrlKey:true}).defaultPrevented,'Inspector background uses design undo');
   await wait(()=>title()&&Vvveb.Undo.undoIndex===index,'undo property edit');
   assert(!messages.filter(m=>m.type==='dirty').at(-1).document.overrides['custom-test-screen-title-color'],'Shortcut undoes the style token too');
   assert(!key(document.body,{}).defaultPrevented,'Plain Z is not a shortcut');
   assert(!key(document.body,{metaKey:true,altKey:true}).defaultPrevented,'Option-modified Z is left alone');
   assert(!key(document.body,{ctrlKey:true,isComposing:true}).defaultPrevented,'IME composition is left alone');
   edit('New branch');await flush('history-new-branch');
   key(document.body,{metaKey:true,shiftKey:true});
   assert(title()?.textContent==='New branch'&&Vvveb.Undo.undoIndex===Vvveb.Undo.mutations.length-1,'A new edit discards the old redo branch');
   document.getElementById('preview').click();
   assert(!key(document.body,{metaKey:true}).defaultPrevented,'Preview does not undo design edits');
   assert(window.testErrors.length===0,'Unexpected errors: '+JSON.stringify(window.testErrors));
   originalSend(JSON.stringify({type:'thumbnail-ready'}));return;
 }
 if(window.__CHORO_STUDIO__.testSemanticText){
   // Real billing markup: the date cell worked, but the metric's <strong>
   // could be selected without ever entering the live text editor.
   const cases=[...frame().contentDocument.querySelectorAll('[data-text-case]')].map(node=>[node.dataset.textCase,node.textContent]);
   for(const [id,original] of cases){
     const node=frame().contentDocument.querySelector('[data-text-case="'+id+'"]');
     node.dispatchEvent(new MouseEvent('click',{bubbles:true}));
     node.dispatchEvent(new MouseEvent('dblclick',{bubbles:true,cancelable:true}));
     assert(Vvveb.WysiwygEditor.isActive&&Vvveb.WysiwygEditor.element===node,'Double-click starts editing '+node.tagName);
     assert(node.isContentEditable&&frame().contentDocument.activeElement===node,'Text input focuses '+node.tagName);
     assert(getComputedStyle(document.getElementById('wysiwyg-editor')).display!=='none','Inline formatting appears for '+node.tagName);
     const doc=frame().contentDocument,range=doc.createRange(),selection=frame().contentWindow.getSelection();
     range.selectNodeContents(node);selection.removeAllRanges();selection.addRange(range);
     const replacement='Edited '+id;
     assert(doc.execCommand('insertText',false,replacement),'Typing edits '+node.tagName);
     node.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
     window.choroStudioReply({session:'test',type:'flush',request_id:'text-'+id});
     await wait(()=>messages.some(m=>m.type==='flushed'&&m.request_id==='text-'+id),'save '+id);
     const saved=new DOMParser().parseFromString(messages.filter(m=>m.type==='save').at(-1).document.html,'text/html');
     assert(saved.querySelector('[data-text-case="'+id+'"]').textContent===replacement,'Saved text for '+id);
     assert(saved.querySelector('p').textContent==='Unchanged neighbor'&&saved.querySelector('[data-text-neighbor]').textContent==='8 invoices','Editing leaves neighboring text intact');
     assert(saved.querySelector('[data-chart]').outerHTML.includes('<path')&&!saved.querySelector('[contenteditable]'),'Editing preserves graphics and excludes editor attributes');
     document.getElementById('undo').click();
     await wait(()=>title()&&frame().contentDocument.querySelector('[data-text-case="'+id+'"]').textContent===original,'undo '+id);
     document.getElementById('redo').click();
     await wait(()=>title()&&frame().contentDocument.querySelector('[data-text-case="'+id+'"]').textContent===replacement,'redo '+id);
     document.getElementById('undo').click();
     await wait(()=>title()&&frame().contentDocument.querySelector('[data-text-case="'+id+'"]').textContent===original,'restore '+id);
   }
   for(const selector of ['[data-chart] path','input']){
     const node=frame().contentDocument.querySelector(selector);
     node.dispatchEvent(new MouseEvent('dblclick',{bubbles:true,cancelable:true}));
     assert(!Vvveb.WysiwygEditor.isActive&&!node.isContentEditable,'Non-text '+selector+' stays outside rich-text editing');
   }
   assert(window.testErrors.length===0,'Unexpected errors: '+JSON.stringify(window.testErrors));
   originalSend(JSON.stringify({type:'thumbnail-ready'}));return;
 }
 assert(frame().contentDocument.documentElement.clientWidth===800&&frame().contentDocument.documentElement.clientHeight===600,'Edit uses the declared authored viewport');
 assert(layoutMode()==='desktop','Edit starts on the authored desktop media-query branch');
 assertFrameBounds(800,600,'Edit Fit');
 assert(['edit','preview','undo','redo','zoom','status','retry','recover','inspector-toggle'].every(id=>document.getElementById(id))&&['fit','selection','4','2','1','0.75','0.5','0.25'].every(value=>[...document.getElementById('zoom').options].some(o=>o.value===value)),'Redesigned toolbar keeps every existing action and zoom choice');
 assert(document.getElementById('undo').getAttribute('aria-label')==='Undo'&&document.getElementById('redo').getAttribute('aria-label')==='Redo','Icon actions stay named');
 assert(document.querySelector('#content-tab > .alert strong')?.textContent==='Nothing selected'&&!document.querySelector('#content-tab > .alert .btn-close'),'Empty inspector gives direction instead of a dismissible alert');
 assert(document.getElementById('status').dataset.state==='saved','Status dot starts saved');
 assertToolbarReachable('Default toolbar');
 const zoom=document.getElementById('zoom');zoom.value='0.5';zoom.dispatchEvent(new Event('change'));
 assert(frame().contentDocument.documentElement.clientWidth===800&&layoutMode()==='desktop','Explicit zoom changes only visual scale');
 assert(close(frame().getBoundingClientRect().width,400)&&close(frame().getBoundingClientRect().height,300),'50% zoom has exact visual frame bounds');
 assertFrameBounds(800,600,'Edit 50%');
 // Camera gestures keep the same authored viewport, selection and undo history.
 const stage=document.getElementById('canvas'), cameraFrame=frame(), cameraDoc=frame().contentDocument;
 const undoBeforeCamera=Vvveb.Undo.undoIndex;
 const wheel=(target,options)=>target.dispatchEvent(new WheelEvent('wheel',{bubbles:true,cancelable:true,...options}));
 let beforeCamera=frame().getBoundingClientRect();
 const anchorX=beforeCamera.left+100,anchorY=beforeCamera.top+90;
 wheel(stage,{clientX:anchorX,clientY:anchorY,deltaY:-40,ctrlKey:true});
 let afterCamera=frame().getBoundingClientRect();
 assert(afterCamera.width>beforeCamera.width,'Ctrl-wheel zooms in');
 assert(close((anchorX-beforeCamera.left)/beforeCamera.width,(anchorX-afterCamera.left)/afterCamera.width)&&Math.abs((anchorX-beforeCamera.left)/beforeCamera.width-(anchorX-afterCamera.left)/afterCamera.width)<.002,'Zoom keeps the authored point under the pointer');
 assert(Math.abs((anchorY-beforeCamera.top)/beforeCamera.height-(anchorY-afterCamera.top)/afterCamera.height)<.002,'Zoom anchors both axes');
 beforeCamera=afterCamera;
 wheel(cameraDoc.body,{clientX:100,clientY:100,deltaX:60,deltaY:80});
 afterCamera=frame().getBoundingClientRect();
 assert(close(afterCamera.left,beforeCamera.left-60)&&close(afterCamera.top,beforeCamera.top-80),'Two-finger pan crosses the live Edit iframe boundary');
 cameraDoc.body.dispatchEvent(new KeyboardEvent('keydown',{code:'Space',key:' ',bubbles:true,cancelable:true}));
 beforeCamera=frame().getBoundingClientRect();
 cameraDoc.body.dispatchEvent(new PointerEvent('pointerdown',{pointerId:7,button:0,screenX:100,screenY:100,bubbles:true,cancelable:true}));
 cameraDoc.body.dispatchEvent(new PointerEvent('pointermove',{pointerId:7,screenX:180,screenY:140,bubbles:true,cancelable:true}));
 cameraDoc.body.dispatchEvent(new PointerEvent('pointerup',{pointerId:7,bubbles:true,cancelable:true}));
 cameraDoc.body.dispatchEvent(new KeyboardEvent('keyup',{code:'Space',key:' ',bubbles:true}));
 afterCamera=frame().getBoundingClientRect();
 assert(close(afterCamera.left,beforeCamera.left+80)&&close(afterCamera.top,beforeCamera.top+40),'Space-drag pans using stable screen coordinates');
 // Consume the suppressed drag click, then select for Fit selection.
 click();click();zoom.value='selection';zoom.dispatchEvent(new Event('change'));
 const fitted=title().getBoundingClientRect(), shown=frame().getBoundingClientRect(), stageBounds=stage.getBoundingClientRect(), fittedScale=shown.width/800;
 assert(close(shown.left+(fitted.left+fitted.width/2)*fittedScale,stageBounds.left+stageBounds.width/2),'Fit selection centers the selected element');
 assert(fitted.width*fittedScale<=stageBounds.width-54,'Fit selection keeps the selection inside the stage');
 assert(frame()===cameraFrame&&frame().contentDocument===cameraDoc&&cameraDoc.documentElement.clientWidth===800&&layoutMode()==='desktop','Camera motion never reloads or responsively reflows the design');
 assert(Vvveb.Undo.undoIndex===undoBeforeCamera&&!messages.some(m=>m.type==='save'||m.type==='dirty'&&m.dirty),'Camera motion creates no saved edits or undo steps');
 if(window.__CHORO_STUDIO__.testCameraState){
   const remembered=frame().getBoundingClientRect();
   window.choroStudioReply({session:'test',type:'viewport',width:390,height:844});
   window.choroStudioReply({session:'test',type:'viewport',width:800,height:600});
   const restored=frame().getBoundingClientRect();
   assert(close(restored.width,remembered.width)&&close(restored.left,remembered.left)&&close(restored.top,remembered.top),'Camera restores independently for each explicit viewport');
   window.choroStudioReply({session:'test',type:'viewport',width:390,height:844});
   localStorage.setItem('choro.studio.camera.v1:test-screen:800x600','{"mode":"free","zoom":-2,"x":null,"y":1e20}');
   window.choroStudioReply({session:'test',type:'viewport',width:800,height:600});
   assert(zoom.value==='fit'&&Number.isFinite(frame().getBoundingClientRect().width),'Invalid saved camera recovers to Fit');
 }

 title().dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));
 const textSpace=new KeyboardEvent('keydown',{code:'Space',key:' ',bubbles:true,cancelable:true});title().dispatchEvent(textSpace);
 assert(!textSpace.defaultPrevented,'Space remains a text character during inline editing');
 title().dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
 zoom.value='fit';zoom.dispatchEvent(new Event('change'));
 const editorLayout=document.getElementById('layout');editorLayout.style.width='700px';
 await wait(()=>document.getElementById('canvas').clientWidth===400,'narrow host layout');
 assert(frame().contentDocument.documentElement.clientWidth===800&&layoutMode()==='desktop','Narrow host scales Edit without changing its media-query viewport');
 assertFrameBounds(800,600,'Narrow Edit Fit');
 editorLayout.style.width='';
 await wait(()=>document.getElementById('canvas').clientWidth>700,'wide host layout');
 assert(frame().contentDocument.documentElement.clientWidth===800&&layoutMode()==='desktop','Wide host preserves the authored viewport');
 assertFrameBounds(800,600,'Wide Edit Fit');
 assert(frame().contentDocument.querySelector('script')===null,'Edit must remove executable script elements');
 assert(frame().contentDocument.body.getAttribute('onload')===null,'Edit must remove executable handlers');
 assert(!window.__editEscaped,'CSS cannot close its style element and execute code');
 // Selection, a no-op inline session, and other-screen updates must not save/reload this document.
 const initialFrame=frame();
 const initialWidth=window.__CHORO_STUDIO__.width, initialHeight=window.__CHORO_STUDIO__.height;
 window.choroStudioReply({session:'wrong-session',type:'viewport',width:390,height:844});
 assert(window.__CHORO_STUDIO__.width===initialWidth,'A stale session cannot resize the screen');
 window.choroStudioReply({session:'test',type:'viewport',width:390,height:844});
 assert(frame()===initialFrame && frame().contentDocument.documentElement.clientWidth===390,'Mobile viewport resizes the same editor document');
 await wait(()=>layoutMode()==='mobile','mobile media query');
 assert(layoutMode()==='mobile','Explicit Mobile control is allowed to change the media-query branch');
 assertFrameBounds(390,844,'Mobile Edit');
 assert(!messages.some(m=>m.type==='save'||m.type==='dirty'&&m.dirty),'Viewport changes do not alter authored content');
 window.choroStudioReply({session:'test',type:'viewport',width:-1,height:844});
 assert(window.__CHORO_STUDIO__.width===390,'Invalid viewport is rejected');
 window.choroStudioReply({session:'test',type:'viewport',width:280,height:600});
 assert(frame().contentDocument.documentElement.clientWidth===280,'Small authored mobile viewport can be restored');
 window.choroStudioReply({session:'test',type:'viewport',width:initialWidth,height:initialHeight});
 await wait(()=>layoutMode()==='desktop','desktop media query restored');
 assert(layoutMode()==='desktop','Returning to Desktop restores the desktop media-query branch');

 title().dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));
 title().dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
 window.choroStudioReply({session:'test',type:'screens',screens:[{id:'settings',name:'Settings',archived:false}]});
 assert(frame()===initialFrame,'Another screen update preserves the editor');
 assert(Vvveb.Components.get('_base').properties.find(p=>p.key==='data-studio-screen').data.options.length===2,'New screens appear as link targets');
 assert(!messages.some(m=>m.type==='save'||m.type==='dirty'&&m.dirty),'A no-op inline session creates no save');
 const neighborId=frame().contentDocument.querySelector('p').dataset.studioId;
 const canvasRect=document.getElementById('canvas').getBoundingClientRect(), panelRect=document.getElementById('right-panel').getBoundingClientRect();
 assert(canvasRect.width>400 && panelRect.width===300 && panelRect.left>=canvasRect.right-1,'Canvas and upstream inspector occupy separate columns');
 window.__testPhase='text';click();window.__testPhase='text-selected';
 const titleRect=title().getBoundingClientRect(), frameRect=frame().getBoundingClientRect(), outlineRect=document.getElementById('outline').getBoundingClientRect(), frameScale=frameRect.width/initialWidth;
 const expectedOutline=[frameRect.left+titleRect.left*frameScale,frameRect.top+titleRect.top*frameScale,titleRect.width*frameScale,titleRect.height*frameScale];
 assert(close(expectedOutline[0],outlineRect.left)&&close(expectedOutline[1],outlineRect.top)&&close(expectedOutline[2],outlineRect.width)&&close(expectedOutline[3],outlineRect.height),'Selection outline stays aligned through visual scaling: '+JSON.stringify({expectedOutline,outline:[outlineRect.left,outlineRect.top,outlineRect.width,outlineRect.height]}));
 const content=document.querySelector('#right-panel textarea[name=innerHTML]'), inspectorToggle=document.getElementById('inspector-toggle');
 assert(inspectorToggle.tagName==='BUTTON'&&inspectorToggle.tabIndex===0&&inspectorToggle.getAttribute('aria-controls')==='right-panel','Inspector toggle is a keyboard-focusable control for the panel');
 assert(inspectorToggle.getAttribute('aria-expanded')==='true'&&inspectorToggle.getAttribute('aria-label')==='Hide inspector'&&inspectorToggle.title==='Hide inspector','Expanded inspector state is named for assistive technology and the tooltip');
 const frameBeforeCollapse=frame(), documentBeforeCollapse=frame().contentDocument, selectedBeforeCollapse=Vvveb.Builder.selectedEl, canvasWidthBeforeCollapse=document.getElementById('canvas').clientWidth, frameWidthBeforeCollapse=frame().getBoundingClientRect().width;
 const writesBeforeCollapse=messages.filter(m=>m.type==='dirty'||m.type==='save').length;
 inspectorToggle.focus();inspectorToggle.click();
 await wait(()=>document.getElementById('canvas').clientWidth>canvasWidthBeforeCollapse,'collapsed inspector gives its width to the stage');
 assert(document.body.classList.contains('inspector-collapsed')&&document.getElementById('right-panel').hidden,'Collapsed inspector is removed from layout and accessibility navigation');
 assert(inspectorToggle.getAttribute('aria-expanded')==='false'&&inspectorToggle.getAttribute('aria-label')==='Show inspector'&&inspectorToggle.title==='Show inspector','Collapsed inspector state exposes the reopen action');
 assert(frame()===frameBeforeCollapse&&frame().contentDocument===documentBeforeCollapse&&Vvveb.Builder.selectedEl===selectedBeforeCollapse,'Collapsing preserves the live document and selected element');
 assert(frame().contentDocument.documentElement.clientWidth===initialWidth&&layoutMode()==='desktop','Collapsing changes only visual Fit, not the authored viewport');
 assert(frame().getBoundingClientRect().width>frameWidthBeforeCollapse,'Fit recomputes against the wider stage');
 assert(messages.filter(m=>m.type==='dirty'||m.type==='save').length===writesBeforeCollapse,'Collapsing does not dirty or save the document');
 inspectorToggle.click();
 await wait(()=>document.getElementById('canvas').clientWidth===canvasWidthBeforeCollapse,'reopened inspector restores its column');
 assert(!document.body.classList.contains('inspector-collapsed')&&!document.getElementById('right-panel').hidden&&inspectorToggle.getAttribute('aria-expanded')==='true','Inspector reopens through the same control');
 assert(frame()===frameBeforeCollapse&&document.querySelector('#right-panel textarea[name=innerHTML]')===content&&Vvveb.Builder.selectedEl===selectedBeforeCollapse,'Reopening preserves the frame, inspector controls, and selection');
 content.value='Edited title';window.__testPhase='text-changing';content.dispatchEvent(new KeyboardEvent('keyup',{key:'e',bubbles:true}));window.__testPhase='text-changed';
 await wait(()=>title()?.textContent==='Edited title','text edit');
 assert(content===document.querySelector('#right-panel textarea[name=innerHTML]'),'Inspector keeps the active input while typing');
 const dirtyFrame=frame(), dirtySelection=Vvveb.Builder.selectedEl;
 inspectorToggle.click();
 assert(frame()===dirtyFrame&&title()?.textContent==='Edited title'&&Vvveb.Builder.selectedEl===dirtySelection,'Collapsing preserves the live dirty draft and selection');
 inspectorToggle.click();
 assert(frame()===dirtyFrame&&document.querySelector('#right-panel textarea[name=innerHTML]')===content&&content.value==='Edited title','Reopening preserves the inspector input state for the dirty draft');
 assert(messages.some(m=>m.type==='dirty'&&m.document.html.includes('Edited title')),'Draft is preserved immediately');
 assert(messages.some(m=>m.type==='dirty'&&m.document.html.includes('window.originalScript')),'Clean source retains authored script independently of rendered DOM');
 window.__testPhase='undo';document.getElementById('undo').click();window.__testPhase='undo-clicked';await wait(()=>title()?.textContent==='Original','undo');
 window.__testPhase='redo';document.getElementById('redo').click();await wait(()=>title()?.textContent==='Edited title','redo');
 window.__testPhase='color';click();const color=document.querySelector('#right-panel input[name=color]');assert(color,'Missing color: '+JSON.stringify(window.testErrors));color.value='var(--color-primary)';color.dispatchEvent(new Event('change',{bubbles:true}));
 await wait(()=>frame().contentDocument.getElementById('vvvebjs-styles').textContent.includes('var(--color-primary)'),'token edit');
 await wait(()=>messages.some(m=>m.type==='save'),'autosave');
 await wait(()=>document.getElementById('status').textContent==='Saved','acknowledgement');
 assert(document.getElementById('status').dataset.state==='saved','Status state follows the saved label');

 if(window.__CHORO_STUDIO__.testAssets){
   const picture=frame().contentDocument.querySelector('img');picture.click();
   const imageInput=document.querySelector('#right-panel input[name=src]');
   assert(imageInput.value==='assets/first.svg','Inspector shows local asset paths');
   imageInput.value='assets/second.svg';imageInput.dispatchEvent(new FocusEvent('focusout',{bubbles:true}));
   await wait(()=>messages.filter(m=>m.type==='dirty').at(-1)?.document.html.includes('src="assets/second.svg"'),'save local image path');
   assert(picture.src===window.__CHORO_STUDIO__.assets['assets/second.svg'],'Local image is displayed immediately');
   window.choroStudioReply({session:'test',type:'flush',request_id:'image-save'});
   await wait(()=>document.getElementById('status').textContent==='Saved','image saved');
   const upload=document.querySelector('#right-panel input[type=file]');
   const transfer=new DataTransfer();transfer.items.add(new File(['<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"/>'],'import.svg',{type:'image/svg+xml'}));upload.files=transfer.files;
   upload.dispatchEvent(new Event('change',{bubbles:true}));
   await wait(()=>messages.some(m=>m.type==='asset'),'native asset import');
   const request=messages.find(m=>m.type==='asset');
   assert(request.document.html.includes('src="assets/'+request.name+'"'),'Import and assignment share a native transaction');
   assert(request.revision>0&&request.fingerprint==='next','Asset import carries its source revision');
   assert(window.testErrors.length===0,'Image control errors: '+JSON.stringify(window.testErrors));
   originalSend(JSON.stringify({type:'thumbnail-ready'}));return;
 }
 if(window.__CHORO_STUDIO__.testProperties){
 // Real upstream style controls persist token overrides, including a second edit
 // that changes only the token value, then undo both as separate transactions.
 window.__testPhase='custom-style';
 let customColor=document.querySelector('#right-panel input[name=color]');
 customColor.value='#aa2244';customColor.dispatchEvent(new Event('change',{bubbles:true}));
 await wait(()=>messages.filter(m=>m.type==='dirty').at(-1)?.document.overrides['custom-test-screen-title-color']==='#aa2244','custom color token');
 customColor.value='#2244aa';customColor.dispatchEvent(new Event('change',{bubbles:true}));
 await wait(()=>messages.filter(m=>m.type==='dirty').at(-1)?.document.overrides['custom-test-screen-title-color']==='#2244aa','second custom color');
 document.getElementById('undo').click();await wait(()=>title() && messages.filter(m=>m.type==='dirty').at(-1)?.document.overrides['custom-test-screen-title-color']==='#aa2244','undo custom value');
 document.getElementById('undo').click();await wait(()=>title() && !('custom-test-screen-title-color' in messages.filter(m=>m.type==='dirty').at(-1).document.overrides),'undo custom token creation');
 // Use the actual floating toolbar on a partial text range; formatting must
 // round trip through clean source and undo without touching the neighboring p.
 window.__testPhase='inline-format';
 title().dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));
 const range=frame().contentDocument.createRange();range.setStart(title().firstChild,0);range.setEnd(title().firstChild,6);
 const selection=frame().contentWindow.getSelection();selection.removeAllRanges();selection.addRange(range);
 document.getElementById('bold-btn').click();
 title().dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
 assert(title().querySelector('span')?.style.fontWeight==='bold','Actual upstream toolbar formats the selected text');
 assert(messages.filter(m=>m.type==='dirty').at(-1).document.html.includes('font-weight: bold'),'Inline markup is saved');
 document.getElementById('undo').click();await wait(()=>title()?.textContent==='Edited title'&&!title().querySelector('span'),'undo inline formatting');
 document.getElementById('redo').click();await wait(()=>title(),'redo inline load');assert(title().querySelector('span')?.style.fontWeight==='bold','Redo inline formatting: '+title().outerHTML+'; '+messages.filter(m=>m.type==='dirty').at(-1).document.html);

 assert(frame().contentDocument.querySelector('p').dataset.studioId===neighborId,'Formatting preserves neighboring element identity');
 title().dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));
 document.querySelector('a[href="#style-tab"]').click();
 originalSend(JSON.stringify({type:'thumbnail-ready'}));return;
 }
 // Implement must flush an active inline edit and wait for that exact save.
 autoAck=false;window.__testPhase='handoff-flush';
 title().dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));
 title().textContent='Final heading';
 const priorSaves=messages.filter(m=>m.type==='save').length;
 window.choroStudioReply({session:'test',type:'flush',request_id:'implement-1'});
 await wait(()=>messages.filter(m=>m.type==='save').length>priorSaves,'flush saves active inline text');
 let save=messages.filter(m=>m.type==='save').at(-1);
 assert(save.document.html.includes('Final heading'),'Flush captures the last inline text');
 assert(!messages.some(m=>m.type==='flushed'&&m.request_id==='implement-1'),'No handoff before save acknowledgement');
 window.choroStudioReply({session:'test',id:save.id,error:'Fixture save conflict'});
 assert(!messages.some(m=>m.type==='flushed'&&m.request_id==='implement-1'),'Save failure never permits handoff');
 assert(title().textContent==='Final heading','Failed save preserves editing buffer');
 assert(document.getElementById('status').dataset.state==='failed'&&!document.getElementById('recover').hidden,'Failed save shows the danger state and recovery actions');
 // Save recovery is the widest toolbar state. Narrow hosts must keep every action reachable,
 // with the inspector open and collapsed, and must never touch the authored viewport.
 const recoveryIds='edit,preview,undo,redo,status,retry,recover,zoom,inspector-toggle';
 for(const width of [560,520,400]){
   await narrowHost(width,'narrow recovery host '+width);
   assert(assertToolbarReachable('Save recovery at '+width+'px').join()===recoveryIds,'Save recovery keeps every action visible at '+width+'px');
   inspectorToggle.click();
   assert(assertToolbarReachable('Collapsed save recovery at '+width+'px').join()===recoveryIds,'Collapsed save recovery keeps every action visible at '+width+'px');
   inspectorToggle.click();
   assert(frame().contentDocument.documentElement.clientWidth===initialWidth&&layoutMode()==='desktop','Toolbar wrapping never changes the authored viewport');
   assertFrameBounds(initialWidth,initialHeight,'Save recovery at '+width+'px');
 }
 await narrowHost(0,'restored recovery host');
 assertToolbarReachable('Restored save recovery');
 document.getElementById('retry').click();
 await wait(()=>messages.filter(m=>m.type==='save').at(-1).id!==save.id,'retry transaction');
 save=messages.filter(m=>m.type==='save').at(-1);
 window.choroStudioReply({session:'test',id:save.id,revision:save.revision+1,fingerprint:'final'});
 assert(messages.some(m=>m.type==='flushed'&&m.request_id==='implement-1'),'Successful retry acknowledges the requested flush');
 assert(frame().contentDocument.querySelector('p').textContent==='Unchanged neighbor','Inline flush preserves neighboring content');
 autoAck=true;
 inspectorToggle.click();
 assert(document.body.classList.contains('inspector-collapsed'),'Inspector can be collapsed before entering Preview');
 window.__testPhase='preview';document.getElementById('preview').click();
 await wait(()=>prototypeResult,'prototype script');
 assert(inspectorToggle.hidden,'Preview hides the editing-only inspector control');
 assert(messages.some(m=>m.type==='mode'&&m.mode==='preview'),'Preview choice is sent to the host for the next screen');
 assert(prototypeResult.parentBlocked,'Prototype cannot access the trusted parent document');
 assert(prototypeResult.networkBlocked,'Prototype network requests are blocked');
 assert(prototypeResult.bridgeBlocked,'Prototype has no native IPC bridge');
 assert(prototypeResult.viewportWidth===initialWidth&&prototypeResult.desktopLayout,'Preview uses the same authored viewport and media-query branch as Edit');
 assert(frame().getAttribute('sandbox')==='allow-scripts','Preview has no same-origin privilege');
 assertFrameBounds(initialWidth,initialHeight,'Preview Fit');
 const previewFrame=frame();
 zoom.value='0.75';zoom.dispatchEvent(new Event('change'));
 assert(frame()===previewFrame&&close(frame().getBoundingClientRect().width,600)&&close(frame().getBoundingClientRect().height,450),'Explicit Preview zoom scales without reloading or resizing authored content');
 assertFrameBounds(initialWidth,initialHeight,'Preview 75%');
 const bridgeBounds=frame().getBoundingClientRect();
 window.dispatchEvent(new MessageEvent('message',{source:window,data:{type:'studio-camera',input:{kind:'pan',x:0,y:0,dx:100,dy:100}}}));
 window.dispatchEvent(new MessageEvent('message',{source:frame().contentWindow,data:{type:'studio-camera',input:{kind:'pan',x:0,y:0,dx:NaN,dy:100}}}));
 assert(close(frame().getBoundingClientRect().left,bridgeBounds.left),'Camera bridge rejects foreign windows and invalid numbers');
 window.dispatchEvent(new MessageEvent('message',{source:frame().contentWindow,data:{type:'studio-camera',input:{kind:'zoom',x:200,y:100,dx:0,dy:-40}}}));
 assert(frame().getBoundingClientRect().width>bridgeBounds.width&&frame()===previewFrame,'Opaque Preview camera accepts bounded view-only gestures without reloading');
 const beforeOpaqueGesture=frame().getBoundingClientRect().width;
 frame().contentWindow.postMessage('test-camera-gesture','*');
 await wait(()=>frame().getBoundingClientRect().width>beforeOpaqueGesture,'gesture from opaque Preview document');
 assert(frame()===previewFrame&&frame().style.width===initialWidth+'px','The actual Preview input relay preserves its live authored viewport');
 // A prototype page scrolls itself. Delta that chains out of the frame must not
 // also pan the stage; only the relayed zoom gesture above may move the camera.
 const beforeChained=frame().getBoundingClientRect();
 frame().dispatchEvent(new WheelEvent('wheel',{bubbles:true,cancelable:true,clientX:200,clientY:150,deltaX:40,deltaY:120}));
 const afterChained=frame().getBoundingClientRect();
 assert(close(afterChained.left,beforeChained.left)&&close(afterChained.top,beforeChained.top)&&close(afterChained.width,beforeChained.width),'Scrolling inside the Preview page never pans or zooms the stage');
 const stageBefore=document.getElementById('canvas');
 assert(stageBefore.scrollTop===0&&stageBefore.scrollLeft===0,'The Preview stage itself never scrolls');


 zoom.value='fit';zoom.dispatchEvent(new Event('change'));
 editorLayout.style.width='500px';
 await wait(()=>document.getElementById('canvas').clientWidth===500,'narrow Preview host');
 assert(frame()===previewFrame&&frame().style.width===initialWidth+'px','Narrow Preview host keeps the running authored viewport');
 assertFrameBounds(initialWidth,initialHeight,'Narrow Preview Fit');
 editorLayout.style.width='';
 await wait(()=>document.getElementById('canvas').clientWidth>1000,'wide Preview host');
 assert(frame()===previewFrame&&frame().style.width===initialWidth+'px','Wide Preview host keeps the running authored viewport');
 assertFrameBounds(initialWidth,initialHeight,'Wide Preview Fit');
 window.choroStudioReply({session:'test',type:'viewport',width:390,height:844});
 assert(frame()===previewFrame && window.__CHORO_STUDIO__.width===390,'Mobile toggle preserves the running prototype while changing only its explicit viewport');
 assertFrameBounds(390,844,'Mobile Preview');
 window.choroStudioReply({session:'test',type:'viewport',width:initialWidth,height:initialHeight});
 window.__testPhase='edit';document.getElementById('edit').click();await wait(()=>title()?.textContent==='Final heading','return to edit');
 assert(document.body.classList.contains('inspector-collapsed')&&!inspectorToggle.hidden&&inspectorToggle.getAttribute('aria-expanded')==='false','Returning to Edit restores the collapsed inspector preference');
 inspectorToggle.click();assert(!document.body.classList.contains('inspector-collapsed'),'Inspector remains reopenable after returning from Preview');
 inspectorToggle.click();assert(document.body.classList.contains('inspector-collapsed'),'Inspector remains collapsible after reopening');
 assert(!frame().contentDocument.querySelector('[data-runtime]'),'Prototype DOM changes never enter source');
 assert(window.testErrors.length===0,'Unexpected errors: '+JSON.stringify(window.testErrors));
 originalSend(JSON.stringify({type:'thumbnail-ready'}));
})().catch(error=>window.ipc.postMessage(JSON.stringify({type:'render-error',error:error.message+"\n"+error.stack})));
'''
boot={'session':'test','screen_id':'test-screen','document':{'html':'<!doctype html><html><head></head><body onload="window.bad=true"><h1 data-studio-id="title">Original</h1><p>Unchanged neighbor</p><script>window.originalScript=true</script></body></html>','css':'body{font:24px system-ui;padding:32px;background:white;color:#202124;--fixture-layout:desktop}@media(max-width:600px){body{--fixture-layout:mobile}}','js':"const initialViewportWidth=innerWidth,desktopLayout=matchMedia('(min-width: 601px)').matches;let parentBlocked=false;try{parent.document.body}catch(e){parentBlocked=true};const node=document.createElement('div');node.dataset.runtime='true';document.body.append(node);let networkBlocked=false;document.addEventListener('securitypolicyviolation',e=>{if(e.effectiveDirective==='connect-src')networkBlocked=true});fetch('https://example.invalid/studio-isolation-test').catch(()=>{});setTimeout(()=>parent.postMessage({type:'prototype-result',parentBlocked,networkBlocked,bridgeBlocked:!window.ipc,viewportWidth:initialViewportWidth,desktopLayout},'*'),50);"},'revision':0,'fingerprint':'initial','tokens':{'color-primary':'#335cff'},'tokens_css':':root{--color-primary:#335cff}','screens':[],'width':800,'height':600,'assets':{},'thumbnail':False}
boot['document']['js'] += ";window.addEventListener('message',event=>{if(event.source===parent&&event.data==='test-camera-gesture')document.body.dispatchEvent(new WheelEvent('wheel',{bubbles:true,cancelable:true,ctrlKey:true,clientX:200,clientY:100,deltaY:-20}));});"
boot['testCameraState']='--camera-state' in sys.argv or '--camera-native-state' in sys.argv
boot['prototype']='--prototype' in sys.argv
boot['system_specimen']='--system' in sys.argv
boot['testAssets']='--assets' in sys.argv
if boot['testAssets']:
    import base64
    boot['document']['html']=boot['document']['html'].replace('</body>','<img data-studio-id="picture" src="assets/first.svg" alt="Local image"></body>')
    boot['assets']={f'assets/{name}.svg':'data:image/svg+xml;base64,'+base64.b64encode(f'<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="{color}"/></svg>'.encode()).decode() for name,color in [('first','red'),('second','blue')]}
boot['testProperties']='--properties' in sys.argv
boot['testHistoryShortcuts']='--history-shortcuts' in sys.argv
boot['testSemanticText']='--semantic-text' in sys.argv
if boot['testSemanticText']:
    boot['document']['html']=boot['document']['html'].replace('</body>', '''<section>
      <article><span>Recurring revenue</span><strong data-text-case="revenue">₪64,800</strong><small data-text-case="ratio">79% of total</small><svg data-chart="" viewBox="0 0 110 28"><path d="M0 24L110 9"/></svg></article>
      <article><span data-text-neighbor="">8 invoices</span><em data-text-case="trend">+12.4%</em><b data-text-case="amount">₪2,400</b></article>
      <h2>Subtotal <mark data-text-case="nested">₪3,540</mark></h2>
      <dl><dt data-text-case="term">Revenue</dt><dd data-text-case="definition">Monthly total</dd></dl>
      <time data-text-case="time" datetime="2026-09-18">18 Sep</time>
      <table><tbody><tr><td data-text-case="date">18 Sep</td></tr></tbody></table>
      <input value="Search invoices" />
    </section></body>''')
boot['mode'] = 'preview' if '--preview-start' in sys.argv or boot['prototype'] else 'edit'
boot['document']['css'] += '/* </style><script>parent.__editEscaped=true</script> */'
encoded=json.dumps(boot).replace('<','\\u003c').replace('>','\\u003e')
def bundle(boot):
    encoded=json.dumps(boot).replace('<','\\u003c').replace('>','\\u003e')
    names=['upstream/popper.min.js','upstream/bootstrap.min.js','upstream/builder.js','templates.js','upstream/undo.js','upstream/inputs.js','upstream/autocomplete.js','upstream/components-common.js','upstream/components-html.js','upstream/coloris.js']
    stub="Object.defineProperty(window,'localStorage',{value:{data:new Map(),getItem(key){return this.data.get(key)||null},setItem(key,value){this.data.set(key,String(value))}}});"
    # The player must ignore a stored camera, so seed one it would otherwise use.
    seed=stub+"localStorage.setItem('choro.studio.camera.v1:test-screen:800x600','{\"mode\":\"free\",\"zoom\":2,\"x\":10,\"y\":10}');"
    storage=seed if '--prototype' in sys.argv else stub if '--camera-state' in sys.argv else ''
    vendor=storage+'window.__CHORO_STUDIO__='+encoded+';\n'+'\n'.join((root/'vendor'/name).read_text() for name in names)
    css='\n'.join((root/'vendor'/name).read_text() for name in ['upstream/editor.css','fonts.css','upstream/coloris.min.css'])
    return (root/'editor.html').read_text().replace('/*STUDIO_VENDOR_CSS*/',css).replace('<!--STUDIO_RIGHT_PANEL-->',(root/'vendor/upstream/right-panel.html').read_text()).replace('<!--STUDIO_INLINE_TOOLBAR-->',(root/'vendor/upstream/inline-toolbar.html').read_text()).replace('/*STUDIO_VENDOR*/',vendor.replace('</script','<\\/script')).replace('/*STUDIO_EDITOR*/',(root/'editor.js').read_text())
specimen_file=next((arg.split('=',1)[1] for arg in sys.argv if arg.startswith('--specimen-fixture=')),None)
if specimen_file:
    boot.update(json.loads(pathlib.Path(specimen_file).read_text()))
html=bundle(boot)
head,tail=html.rsplit('</body>',1)
html=head+'<script>'+checks+'</script></body>'+tail
output=pathlib.Path(tempfile.mkdtemp(prefix='choro-studio-editor-test-'))/'editor.png'
result=subprocess.run([str(helper)],input=json.dumps({'html':html,'output':str(output),'width':1100,'height':700})+'\n',text=True,stdout=subprocess.PIPE,stderr=None if '--trace' in sys.argv else subprocess.PIPE,timeout=20)
assert result.returncode==0, result.stderr
response=json.loads(result.stdout.strip())
assert response.get('error') is None, (response,result.stderr)
assert output.stat().st_size>1000
print('PASS: the prototype player opens every screen fitted and stores no camera' if boot['prototype'] else 'PASS: keyboard undo/redo through 12 saved edits, active typing, property changes, redo branching and input/Preview exclusions' if boot['testHistoryShortcuts'] else 'PASS: semantic text double-click, focus, inline toolbar, typed saves, undo/redo and intact neighbors' if boot['testSemanticText'] else 'PASS: system specimen isolation and disabled screen editing' if boot['system_specimen'] else 'PASS: local image selection, clean asset paths, and revisioned native upload/assignment' if boot['testAssets'] else 'PASS: upstream controls, focus preservation, custom-token undo, partial-text formatting and inline undo/redo' if boot['testProperties'] else 'PASS: text, tokens, autosave, undo/redo, correlated Implement flush, failed-save recovery, Edit isolation, Preview sandbox and clean source round trip')
print('Screenshot:',output)

if '--export' in sys.argv:
    export_boot={**boot,'thumbnail':True,'mode':'edit','width':390,'height':844}
    export_output=output.parent/'export.png'
    export_result=subprocess.run([str(helper)],input=json.dumps({'html':bundle(export_boot),'output':str(export_output),'width':390,'height':844,'full_resolution':True})+'\n',text=True,capture_output=True,timeout=20)
    assert export_result.returncode==0, export_result.stderr
    export_response=json.loads(export_result.stdout.strip())
    assert export_response.get('error') is None,(export_response,export_result.stderr)
    png=export_output.read_bytes()
    assert png[:8]==b'\x89PNG\r\n\x1a\n' and int.from_bytes(png[16:20],'big')==390 and int.from_bytes(png[20:24],'big')==844
    print('PASS: full-resolution export preserves the explicit 390 × 844 authored dimensions')

if '--benchmark' in sys.argv:
    boot['thumbnail']=True
    boot['document']['js']=''
    boot['width']=1440;boot['height']=960
    boot['document']['html']='<html><body><h1>Screen overview fixture</h1>'+''.join('<section class="card"><h2>Card '+str(i)+'</h2><p>Sample design content</p></section>' for i in range(30))+'</body></html>'
    boot['document']['css']='body{font:16px system-ui;display:flex;flex-wrap:wrap;gap:16px;padding:32px}h1{width:100%}.card{width:220px;background:#f4f5f7;padding:16px;border-radius:12px}'
    jobs=[]
    for index in range(50):
        encoded=json.dumps(boot).replace('<','\\u003c').replace('>','\\u003e')
        page=bundle(boot)
        jobs.append(json.dumps({'html':page,'output':str(output.parent/f'screen-{index}.png'),'width':1440,'height':960}))
    started=time.monotonic()
    batch=subprocess.run(['/usr/bin/time','-l',str(helper)],input='\n'.join(jobs)+'\n',text=True,capture_output=True,timeout=120)
    responses=[json.loads(line) for line in batch.stdout.splitlines()]
    assert len(responses)==50 and all(r['error'] is None for r in responses),responses
    print(f'PASS: 50 serial screen captures; helper exited after queue drained; {time.monotonic()-started:.2f}s elapsed')
    print(batch.stderr)
