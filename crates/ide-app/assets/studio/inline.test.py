#!/usr/bin/env python3
"""Real WebKit integration: one inline editor, camera hit-testing, saves and Prototype.
Uses a private nonpersistent helper; never opens Choro or touches design files.
"""
import ast, json, pathlib, subprocess, tempfile, sys
root=pathlib.Path(__file__).resolve().parent
work=pathlib.Path(tempfile.mkdtemp(prefix='choro-inline-test-'))
# Reuse the production-asset bundler from the existing editor regression harness.
module=ast.parse((root/'editor.test.py').read_text())
exec(compile(ast.Module(body=[n for n in module.body if isinstance(n,ast.FunctionDef) and n.name=='bundle'],type_ignores=[]),str(root/'editor.test.py'),'exec'))
editor_boot=dict(session='editor-a',screen_id='a',inline=True,native_toolbar=True,revision=1,fingerprint='initial',document=dict(html='<html><body><h1 data-studio-id="title">Original</h1><p>Neighbor</p></body></html>',css='body{font:24px system-ui;padding:32px;background:white;color:#202124}',js=''),tokens={},tokens_css='',screens=[dict(id='a',name='First',archived=False),dict(id='b',name='Second',archived=False)],width=800,height=600,assets={},thumbnail=False,mode='edit')
probe=r'''
<script>
window.addEventListener('error',e=>parent.postMessage({type:'fixture-error',error:e.message},'*'));
const fixtureTick=()=>new Promise(resolve=>{const c=new MessageChannel();c.port1.onmessage=()=>{c.port1.close();c.port2.close();resolve()};c.port2.postMessage(0)});
window.addEventListener('message',async e=>{
 if(e.source!==parent||e.data?.type!=='fixture-command')return;
 try{
  const end=performance.now()+5000;let frame;
  while(performance.now()<end){frame=document.getElementById('screen');if(frame?.contentDocument?.querySelector('h1'))break;await fixtureTick()}
  const doc=frame.contentDocument,title=doc.querySelector('h1');
  if(!window.fixtureDocument)window.fixtureDocument=doc;
  if(e.data.action==='edit'){title.dispatchEvent(new MouseEvent('dblclick',{bubbles:true}));title.textContent='Changed';title.dispatchEvent(new Event('input',{bubbles:true}));}
  if(['retry','undo','redo','inspector'].includes(e.data.action))window.choroStudioReply({session:window.__CHORO_STUDIO__.session,type:'toolbar-command',command:e.data.action});
  if(e.data.action==='pan')doc.body.dispatchEvent(new WheelEvent('wheel',{bubbles:true,cancelable:true,deltaX:60,deltaY:80,clientX:100,clientY:100}));
  if(e.data.action==='comment-key')doc.body.dispatchEvent(new KeyboardEvent('keydown',{key:'c',bubbles:true}));
  if(e.data.action==='zoom')doc.body.dispatchEvent(new WheelEvent('wheel',{bubbles:true,cancelable:true,ctrlKey:true,deltaY:-40,clientX:100,clientY:100}));
  const r=frame.getBoundingClientRect();
  parent.postMessage({type:'fixture-result',request:e.data.request,text:title.textContent,editing:!!Vvveb.WysiwygEditor.isActive,width:doc.documentElement.clientWidth,height:doc.documentElement.clientHeight,sameDocument:doc===window.fixtureDocument,frame:{x:r.x,y:r.y,width:r.width,height:r.height},error:document.getElementById('error').textContent},'*');
 }catch(error){parent.postMessage({type:'fixture-error',error:String(error)},'*')}
});
</script>
'''
editors={}
for name in ['a','b']:
    boot={**editor_boot,'session':'editor-'+name,'screen_id':name}
    page=bundle(boot)
    head,tail=page.rsplit('</body>',1)
    editors['/fixture/editor-'+name]=head+probe+'</body>'+tail

(work/'editors.json').write_text(json.dumps(editors))
swift=(root/'thumbnail.swift').read_text().replace('deadline:.now()+10','deadline:.now()+30')
swift=swift.replace('WKScriptMessageHandler, WKNavigationDelegate','WKScriptMessageHandler, WKNavigationDelegate, WKURLSchemeHandler')
swift=swift.replace('config.websiteDataStore = .nonPersistent()', 'config.websiteDataStore = .nonPersistent(); config.setURLSchemeHandler(self, forURLScheme:"choro-canvas")')
swift=swift.replace('view.loadHTMLString(next.html,baseURL:nil)', 'view.load(URLRequest(url:URL(string:"choro-canvas://localhost/index.html")!))')
swift=swift.replace('url == "about:blank"', 'url == "choro-canvas://localhost/index.html" || url == "about:blank"')
swift=swift.replace('if type == "render-error"', 'if type == "phase" { fputs(raw+"\\n",stderr);return }\n        if type == "render-error"')
swift=swift.replace('    func finish(error:String?) {','''    func webView(_ webView:WKWebView,start task:WKURLSchemeTask){
        let data=Data((job?.html ?? "").utf8)
        task.didReceive(URLResponse(url:task.request.url!,mimeType:"text/html",expectedContentLength:data.count,textEncodingName:"utf-8"));task.didReceive(data);task.didFinish()
    }
    func webView(_ webView:WKWebView,stop task:WKURLSchemeTask){}
    func finish(error:String?) {''')
(work/'probe.swift').write_text(swift)
subprocess.run(['xcrun','swiftc','-O','-framework','AppKit','-framework','WebKit',str(work/'probe.swift'),'-o',str(work/'probe')],check=True)
dist=root.parents[1]/'web/studio-canvas/dist'
boot=dict(session='fixture',revision=1,fingerprint='initial',screens=[dict(id=name,name='Screen '+name,width=800,height=600,archived=False,content_key=name) for name in ['a','b']],layout=dict(schema_version=1,overview_mode='canvas',viewport=dict(x=70,y=120,zoom=.65),positions=dict(a=dict(x=0,y=0),b=dict(x=920,y=0)),selected_screen_id='a'),theme={},test=True)
# Many image-only screens must not add live authored documents.
for index in range(98):
    name='extra-'+str(index)
    boot['screens'].append(dict(id=name,name=name,width=800,height=600,archived=False,content_key=name))
    boot['layout']['positions'][name]=dict(x=(index%4)*920,y=1500+(index//4)*720)
harness=r'''
const nativeSend=window.ipc.postMessage,messages=[],results=new Map();let rejectSave=false,runStarted=false;
const tick=()=>new Promise(resolve=>{const c=new MessageChannel();c.port1.onmessage=()=>{c.port1.close();c.port2.close();resolve()};c.port2.postMessage(0)});
const wait=async(predicate,label)=>{nativeSend(JSON.stringify({type:'phase',label}));const end=performance.now()+10000;while(performance.now()<end){if(predicate())return;await tick()}throw Error('Timeout: '+label)};
const assert=(value,label)=>{if(!value)throw Error(label)};
const close=(a,b)=>Math.abs(a-b)<1;
addEventListener('error',e=>{if(e.message==='ResizeObserver loop completed with undelivered notifications.'){e.stopImmediatePropagation();e.preventDefault()}} ,true);
addEventListener('message',e=>{if(e.source!==document.querySelector('.inline-editor')?.contentWindow)return;if(e.data?.type==='fixture-result')results.set(e.data.request,e.data);if(e.data?.type==='fixture-stage')nativeSend(JSON.stringify({type:'phase',stage:e.data.stage}));if(e.data?.type==='fixture-error')nativeSend(JSON.stringify({type:'render-error',error:e.data.error}))});
window.ipc.postMessage=raw=>{
 const m=JSON.parse(raw);messages.push(m);
 if(m.type==='open')queueMicrotask(()=>openEditor(m.screen_id));
 if(m.type==='comments-read')queueMicrotask(()=>window.choroCanvasReply({session:'fixture',type:'comments-result',request_id:m.request_id,comments:{schema_version:1,revision:0,pins:[]}}));
 if(m.type==='failed'||m.type==='editor-failed'||m.type==='render-error')nativeSend(JSON.stringify({type:'render-error',error:m.error}));
 if(m.type==='inline-editor'&&m.message.type==='save'){
  const v=m.message;
  queueMicrotask(()=>window.choroStudioReply({session:v.session,id:v.id,...(rejectSave?{error:'Fixture save conflict'}:{revision:v.revision+1,fingerprint:'saved'})}));
 }
 if(m.type==='ready'&&!runStarted){runStarted=true;queueMicrotask(run)}
};
let request=0;
const command=async action=>{const id=++request;document.querySelector('.inline-editor').contentWindow.postMessage({type:'fixture-command',action,request:id},'*');await wait(()=>results.has(id),action);return results.get(id)};
const openEditor=name=>window.choroCanvasReply({session:'fixture',type:'editor',screen_id:name,editor_session:'editor-'+name,document:window.__FIXTURE_EDITORS__['/fixture/editor-'+name]});
async function run(){try{
 await wait(()=>window.choroCanvasTest?.flow?.getNode('a'),'canvas node');
 assert(document.querySelectorAll('iframe').length===0,'Idle overview contains no live HTML');
 document.querySelector('[data-id="a"]').dispatchEvent(new MouseEvent('dblclick',{bubbles:true,clientX:135,clientY:185}));
 await wait(()=>messages.some(m=>m.type==='inline-editor'&&m.message.type==='ready'),'inline editor Ready');
 await wait(()=>document.querySelector('.inline-editor')?.style.visibility==='visible','editor geometry');
 let detail=await command('inspect');const f=window.choroCanvasTest.flow;
 assert(detail.editing,'Double-clicking preview text activates live text editing without another click');
 assert(getComputedStyle(document.querySelector('.inline-editor').contentDocument.querySelector('.zoom')).display==='none','Canvas has no duplicate editor zoom');
 assert(detail.width===800&&detail.height===600,'Authored viewport is independent of canvas zoom');
 assert(close(detail.frame.x,70)&&close(detail.frame.y,120)&&close(detail.frame.width,520),'Live editor exactly covers the artboard');
 const overlay=document.querySelector('.inline-editor');
 await command('comment-key');
 await wait(()=>messages.some(m=>m.type==='inline-editor'&&m.message.type==='comment-toggle'),'C inside the authored screen');
 const beforeComments=f.getViewport();
 window.choroCanvasReply({session:'fixture',type:'comment-mode',enabled:true});
 await wait(()=>document.querySelector('.comments-panel'),'comment overlay');
 assert(document.querySelector('.inline-editor')===overlay&&overlay.style.pointerEvents==='none','Comments keeps the live editor beneath a click-through layer');
 assert((await command('inspect')).sameDocument,'Comments preserves the authored document');
 window.choroCanvasReply({session:'fixture',type:'editor',screen_id:'a',editor_session:'editor-a',document:window.__FIXTURE_EDITORS__['/fixture/editor-a']});
 assert(document.querySelector('.inline-editor')===overlay,'Authoritative editor refresh keeps its iframe while commenting');
 window.choroCanvasReply({session:'fixture',type:'comment-mode',enabled:false});
 await wait(()=>!document.querySelector('.comments-panel'),'leave comment overlay');
 assert(document.querySelector('.inline-editor')===overlay&&overlay.style.pointerEvents===''&&f.getViewport().zoom===beforeComments.zoom,'Leaving Comments restores editing and keeps camera zoom');
 const toolbarState=()=>messages.filter(m=>m.type==='inline-editor'&&m.message.type==='toolbar-state').at(-1)?.message;
 await wait(()=>!!toolbarState(),'native toolbar state');
 assert(getComputedStyle(overlay.contentDocument.getElementById('studio-toolbar')).display==='none','Native screen header replaces the whole web toolbar');
 assert(overlay.contentDocument.getElementById('canvas').getBoundingClientRect().top===0,'No second header or blank toolbar row above the stage');
 const inspectorBefore=toolbarState().inspector_open;
 await command('inspector');await wait(()=>toolbarState().inspector_open!==inspectorBefore,'native inspector collapse');
 assert((await command('inspect')).sameDocument,'Native inspector action preserves the editor document');
 await command('inspector');await wait(()=>toolbarState().inspector_open===inspectorBefore,'native inspector reopen');
 assert(document.elementFromPoint(100,150)===overlay,'Live artboard receives input');
 assert(document.elementFromPoint(30,700)!==overlay,'Empty canvas remains interactive through the clipped overlay');
 assert(document.elementFromPoint(740,180)!==overlay,'A neighboring image artboard remains interactive');
 assert(document.elementFromPoint(1250,180)===overlay,'The shared inspector receives input');
 await f.setViewport({x:150,y:180,zoom:.9});
 for(let i=0;i<20;i++)await tick();detail=await command('inspect');
 assert(close(detail.frame.x,150)&&close(detail.frame.y,180)&&close(detail.frame.width,720)&&detail.sameDocument,'Camera follows canvas without replacing the editor document');
 await command('pan');for(let i=0;i<20;i++)await tick();
 assert(close(f.getViewport().x,90)&&close(f.getViewport().y,100),'Wheel over live content pans the same canvas');
 await command('zoom');for(let i=0;i<20;i++)await tick();assert(f.getViewport().zoom>.9,'Pinch/modified wheel in editor zooms canvas');
 // Culling the artboard must not destroy a live editing session.
 await f.setViewport({x:-30000,y:-30000,zoom:.5});for(let i=0;i<30;i++)await tick();
 assert(document.querySelector('.inline-editor')===overlay&&(await command('inspect')).sameDocument,'Offscreen editor remains alive');
 await f.setViewport({x:70,y:120,zoom:.65});for(let i=0;i<20;i++)await tick();
 rejectSave=true;await command('edit');
 const canvasView=f.getViewport();
 const setView=mode=>window.choroCanvasReply({...window.__CHORO_CANVAS__,type:'state',layout:{...window.__CHORO_CANVAS__.layout,overview_mode:mode,selected_screen_id:'a'}});
 setView('focus');
 await wait(()=>f.getNodes().filter(n=>!n.hidden).length===1,'Focus hides other artboards');
 detail=await command('inspect');assert(detail.text==='Changed'&&detail.sameDocument&&detail.width===800,'Focus preserves the dirty document and authored size');
 assert(document.querySelector('.inline-editor')===overlay,'Focus reuses the editor iframe');
 setView('canvas');
 await wait(()=>f.getNodes().filter(n=>!n.hidden).length===100&&close(f.getViewport().x,canvasView.x)&&close(f.getViewport().zoom,canvasView.zoom),'Canvas restores arrangement and camera');
 assert((await command('inspect')).text==='Changed','Canvas retains unsaved edits after Focus');
 window.choroCanvasReply({session:'fixture',type:'command',command:'flush',request_id:'navigation-1'});
 await wait(()=>messages.some(m=>m.type==='inline-editor'&&m.message.type==='save'),'save transaction');
for(let i=0;i<20;i++)await tick();
 assert(!messages.some(m=>m.type==='flushed'&&m.request_id==='navigation-1'),'Failed save cannot acknowledge canvas navigation');
 detail=await command('inspect');assert(detail.text==='Changed'&&detail.error==='Fixture save conflict','Failed save retains draft and recovery controls');
 await wait(()=>toolbarState()?.status==='failed','native save recovery state');
 rejectSave=false;await command('retry');
 await wait(()=>messages.some(m=>m.type==='flushed'&&m.request_id==='navigation-1'),'successful correlated flush');
 const saves=messages.filter(m=>m.type==='inline-editor'&&m.message.type==='save');assert(saves.at(-1).message.document.html.includes('Changed'),'Save contains the edited text');
 const historyKey=options=>{const event=new KeyboardEvent('keydown',{key:'z',code:'KeyZ',bubbles:true,cancelable:true,...options});document.body.dispatchEvent(event);return event};
 assert(historyKey({metaKey:true}).defaultPrevented,'Canvas Cmd+Z routes to the active editor');
 await wait(()=>messages.some(m=>m.type==='inline-editor'&&m.message.type==='dirty'&&m.message.document.html.includes('Original')),'inline keyboard undo');
 const editedTitle=()=>overlay.contentDocument.getElementById('screen')?.contentDocument?.querySelector('h1')?.textContent;
 await wait(()=>editedTitle()==='Original','undo frame loaded');
 assert(historyKey({ctrlKey:true,shiftKey:true}).defaultPrevented,'Canvas Ctrl+Shift+Z routes redo');
 await wait(()=>editedTitle()==='Changed','inline keyboard redo');
 assert(historyKey({ctrlKey:true}).defaultPrevented,'Canvas Ctrl+Z routes undo');
 await wait(()=>editedTitle()==='Original','second inline keyboard undo');
 assert(document.querySelector('.inline-editor')===overlay,'Keyboard history keeps the same editor overlay');
 window.choroCanvasReply({session:'fixture',type:'command',command:'flush',request_id:'switch-2'});
 await wait(()=>messages.some(m=>m.type==='flushed'&&m.request_id==='switch-2'),'undo save');
 await wait(()=>toolbarState()?.can_redo,'native redo enabled after undo');
 openEditor('b');await wait(()=>messages.some(m=>m.type==='inline-editor'&&m.message.session==='editor-b'&&m.message.type==='ready'),'second editor');
 assert(document.querySelectorAll('iframe').length===1,'Switching artboards keeps exactly one live editor');
 // Sidebar focus can arrive before the new inspector has reported its bounds.
 window.choroCanvasReply({session:'fixture',type:'command',command:'focus-screen',screen_id:'b'});
 const insideStage=()=>{
  const child=document.querySelector('.inline-editor').contentDocument;
  const stage=child.getElementById('canvas').getBoundingClientRect();
  const rect=child.getElementById('screen').getBoundingClientRect();
  return rect.width>100&&rect.x>=stage.x&&rect.y>=stage.y&&rect.right<=stage.right&&rect.bottom<=stage.bottom;
 };
 await wait(insideStage,'sidebar focus fits the selected artboard inside the inspector-aware stage');
 await f.setViewport({x:-30000,y:-30000,zoom:.5});
 window.choroCanvasReply({session:'fixture',type:'command',command:'focus-screen',screen_id:'b'});
 await wait(insideStage,'reselecting the current sidebar screen recenters it');
 assert((await command('inspect')).width===800,'Sidebar focus changes only the camera, never authored dimensions');
 assert(window.choroCanvasStats().mounted<100,'Distant artboards remain culled with a live editor');
 const before=messages.length;
 dispatchEvent(new MessageEvent('message',{source:document.querySelector('.inline-editor').contentWindow,data:{type:'studio-editor',session:'editor-a',message:{session:'editor-a',type:'save'}}}));
 assert(messages.length===before,'A replaced editor session cannot send mutations');
 dispatchEvent(new MessageEvent('message',{source:window,data:{type:'studio-editor',session:'editor-b',message:{session:'editor-b',type:'save'}}}));
 assert(messages.length===before,'A foreign window cannot send editor mutations');
 window.choroCanvasReply({session:'fixture',type:'editor',screen_id:null});
 assert(document.querySelectorAll('iframe').length===0,'Leaving editing releases the live document');
 assert(!historyKey({metaKey:true}).defaultPrevented,'Canvas shortcut does not target a closed editor');
 const readyCount=messages.filter(m=>m.type==='inline-editor'&&m.message.type==='ready').length;
 openEditor('a');await wait(()=>messages.filter(m=>m.type==='inline-editor'&&m.message.type==='ready').length>readyCount,'capture editor Ready');await command('inspect');
 await wait(()=>document.querySelector('.inline-editor')?.style.visibility==='visible','active editor evidence');
 nativeSend(JSON.stringify({type:'thumbnail-ready'}));
}catch(error){nativeSend(JSON.stringify({type:'render-error',error:String(error)+'\n'+error.stack}))}}
'''
html='<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="default-src \'none\'; script-src \'unsafe-inline\'; style-src \'unsafe-inline\'; img-src data:; font-src data:; frame-src about: data:; connect-src \'none\'"><style>'+ (dist/'canvas.css').read_text()+'</style></head><body><div id="root"></div><script>window.__CHORO_CANVAS__='+json.dumps(boot)+';window.__FIXTURE_EDITORS__='+json.dumps(editors).replace('<','\\u003c')+';'+harness+'</script><script>'+(dist/'canvas.js').read_text().replace('</script','<\\/script')+'</script></body></html>'
(work/'canvas.html').write_text(html)
job=dict(html=html,output=str(work/'inline.png'),width=1400,height=900,output_width=1400,output_height=900)
result=subprocess.run([str(work/'probe')],input=json.dumps(job)+'\n',text=True,capture_output=True,timeout=40)
assert result.returncode==0,result.stderr
response=json.loads(result.stdout.strip());assert response.get('error') is None,(response,result.stderr)
print('PASS: direct text activation, Canvas/Focus draft and camera retention, inline geometry, pointer hit-testing, one live editor, gestures, failed-save recovery, undo and stale-message rejection')
print('Evidence:',work)
# Prototype uses the existing opaque-origin isolation boundary and screen links.
prototype={**editor_boot,'inline':False,'prototype':True,'mode':'preview'}
prototype['document']={**prototype['document'],'html':'<html><body><a data-studio-screen="b">Next screen</a></body></html>','js':"setTimeout(()=>document.querySelector('a').click(),50);"}
prototype_checks=r'''<script>
const original=window.ipc.postMessage;window.ipc.postMessage=raw=>{const m=JSON.parse(raw);if(m.type==='navigate'){
 const frame=document.getElementById('screen');
 const zoom=()=>new DOMMatrix(getComputedStyle(document.getElementById('frame-surface')).transform).a;
 const before=zoom();window.choroStudioReply({session:window.__CHORO_STUDIO__.session,type:'camera-command',command:'zoom-in'});
 if(getComputedStyle(document.getElementById('studio-toolbar')).display!=='none'||document.getElementById('canvas').getBoundingClientRect().top!==0){original(JSON.stringify({type:'render-error',error:'Prototype contains a duplicate header'}));return;}
 if(!(zoom()>before)||getComputedStyle(document.querySelector('.zoom')).display!=='none'){original(JSON.stringify({type:'render-error',error:'Prototype native camera or single zoom control failed'}));return;}
 window.choroStudioReply({session:window.__CHORO_STUDIO__.session,type:'camera-command',command:'fit'});
 if(m.screen_id!=='b'||frame.getAttribute('sandbox')!=='allow-scripts'||frame.style.width!=='800px'||getComputedStyle(document.getElementById('edit').parentElement).display!=='none')original(JSON.stringify({type:'render-error',error:'Prototype isolation, viewport or navigation failed'}));
 else original(JSON.stringify({type:'thumbnail-ready'}));
}else original(raw)};
</script>'''
head,tail=bundle(prototype).rsplit('</body>',1)
page=head+prototype_checks+'</body>'+tail
result=subprocess.run([str(work/'probe')],input=json.dumps({**job,'html':page,'output':str(work/'prototype.png')})+'\n',text=True,capture_output=True,timeout=40)
assert result.returncode==0,result.stderr
response=json.loads(result.stdout.strip());assert response.get('error') is None,(response,result.stderr)
print('PASS: Prototype hides editing controls, keeps authored viewport and follows screen links from an opaque sandbox')
