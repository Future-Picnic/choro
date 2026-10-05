#!/usr/bin/env python3
"""Isolated WebKit canvas correctness + frame pacing probe; never controls Choro UI."""
import json,pathlib,subprocess,tempfile,sys,time,platform
root=pathlib.Path(__file__).resolve().parent
bundle=root.parents[1]/'web/studio-canvas/dist'
bundle_arg=next((arg.split('=',1)[1] for arg in sys.argv if arg.startswith('--bundle=')),None)
if bundle_arg: bundle=pathlib.Path(bundle_arg)
work=pathlib.Path(tempfile.mkdtemp(prefix='choro-canvas-check-'))
# The fixture uses the same WKWebView configuration as the existing Studio helper.
visible='--visible' in sys.argv
protocol='--protocol' in sys.argv
unsectioned='--unsectioned' in sys.argv
dense='--dense' in sys.argv
comments_only='--comments' in sys.argv
capture_width=int(next((arg.split('=',1)[1] for arg in sys.argv if arg.startswith('--width=')),1400))
capture_height=int(next((arg.split('=',1)[1] for arg in sys.argv if arg.startswith('--height=')),900))
light='--light' in sys.argv
swift=(root/'thumbnail.swift').read_text().replace('deadline:.now()+10','deadline:.now()+60')
swift=swift.replace('window.addEventListener(\\"error\\",e=>window.ipc.postMessage(JSON.stringify({type:\\"render-error\\",error:e.message})));', '')
swift=swift.replace('baseURL:nil','baseURL:URL(string:"https://canvas.invalid/")').replace('url == "about:blank"','url == "https://canvas.invalid/" || url == "about:blank"')
swift=swift.replace('if type == "render-error"', 'if type == "canvas-metrics" { fputs(raw+"\\n",stderr);return }\n        if type == "render-error"')
# Drive fixture waits from the native run loop. An occluded WKWebView can suspend
# MessageChannel and animation tasks even while the thumbnail deadline runs.
swift=swift.replace('let config = WKWebViewConfiguration()', 'Timer.scheduledTimer(withTimeInterval:0.02,repeats:true) { [weak self] _ in self?.view?.evaluateJavaScript("window.choroFixtureTick?.()",completionHandler:nil) }\n        let config = WKWebViewConfiguration()')
if visible:
 swift=swift.replace('x:-10000,y:-10000','x:120,y:120').replace('window.orderBack(nil)','window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps:true)').replace('setActivationPolicy(.prohibited)','setActivationPolicy(.regular)')
if protocol:
 swift=swift.replace('config.websiteDataStore = .nonPersistent()', 'config.websiteDataStore = .nonPersistent(); config.setURLSchemeHandler(CanvasImages(), forURLScheme: "choro-canvas-image")')
 swift=r'''import AppKit
import WebKit
final class CanvasImages: NSObject, WKURLSchemeHandler {
 func webView(_ webView:WKWebView,start task:WKURLSchemeTask){
  guard let url=task.request.url,url.host=="localhost",url.path.hasPrefix("/fixture/"),let key=url.path.components(separatedBy:"/").last else {task.didFailWithError(NSError(domain:"fixture",code:404));return}
  let parts=key.replacingOccurrences(of:".png",with:"").components(separatedBy:"-")
  guard parts.count==2,let id=Int(parts[0]),let tier=Int(parts[1]),[256,512,1024,2048].contains(tier) else {task.didFailWithError(NSError(domain:"fixture",code:400));return}
  let authoredWidth=id%3==0 ? 390.0 : 1440.0, authoredHeight=960.0
  let ratio=Double(tier)/max(authoredWidth,authoredHeight), width=Int((authoredWidth*ratio).rounded()),height=Int((authoredHeight*ratio).rounded())
  guard let bitmap=NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:width,pixelsHigh:height,bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0),let context=NSGraphicsContext(bitmapImageRep:bitmap) else{return}
  NSGraphicsContext.saveGraphicsState();NSGraphicsContext.current=context
  NSColor(calibratedRed:0.90,green:0.92,blue:Double(id%20)/100+0.75,alpha:1).setFill();NSRect(x:0,y:0,width:width,height:height).fill()
  NSColor(calibratedRed:0.2,green:0.4,blue:0.3,alpha:1).setFill();NSRect(x:0,y:height-height/8,width:width,height:height/8).fill()
  NSGraphicsContext.restoreGraphicsState()
  guard let data=bitmap.representation(using:.png,properties:[:]) else{return}
  task.didReceive(HTTPURLResponse(url:url,statusCode:200,httpVersion:nil,headerFields:["Content-Type":"image/png","Cache-Control":"no-store"])!);task.didReceive(data);task.didFinish()
 }
 func webView(_ webView:WKWebView,stop task:WKURLSchemeTask){}
}
'''+swift
(work/'probe.swift').write_text(swift)
subprocess.run(['xcrun','swiftc','-O','-framework','AppKit','-framework','WebKit',str(work/'probe.swift'),'-o',str(work/'probe')],check=True)
js=(bundle/'canvas.js').read_text().replace('</script','<\\/script');css=(bundle/'canvas.css').read_text()
# Mirrors ide_core::studio::board_geometry so fixture positions are authoritative.
HEADER,PAD,CAPTION,SPACING,EMPTY_W,EMPTY_H,MIN_W=192,48,44,160,480,280,360
def board(screens,sections,stacked):
 by={s['id']:s for s in screens};x=y=0;positions={};boxes=[]
 for section in sections:
  active=[by[i] for i in section['screen_ids'] if not by[i]['archived']];top=y+HEADER+PAD;gap=section['gap']
  if not active: w,h=EMPTY_W,HEADER+EMPTY_H
  else:
   cursor=extent=0
   for s in active:
    if section['direction']=='horizontal': positions[s['id']]=dict(x=x+PAD+cursor,y=top+CAPTION);cursor+=s['width']+gap;extent=max(extent,s['height'])
    else: positions[s['id']]=dict(x=x+PAD,y=top+CAPTION+cursor);cursor+=CAPTION+s['height']+gap;extent=max(extent,s['width'])
   along=cursor-gap
   w,h=(along+2*PAD,HEADER+2*PAD+CAPTION+extent) if section['direction']=='horizontal' else (extent+2*PAD,HEADER+2*PAD+along)
  w=max(w,MIN_W);boxes.append(dict(section,x=x,y=y,width=w,height=h,header_height=HEADER,active_screen_ids=[s['id'] for s in active]))
  if stacked: y+=h+SPACING
  else: x+=w+SPACING
 return boxes,positions
results=[]
for count in ([10] if comments_only else [200] if dense else [10,50,100,200]):
 screens=[dict(id=str(i),name=f'Screen {i+1}',width=1440 if i%3 else 390,height=960,archived=False,content_key=str(i)) for i in range(count)]
 grouped=count-2 if dense else int(count*.6)
 members_per_section=2 if dense else 4
 sections=[dict(id=f'section-{k}',name='Add images and videos from the camera roll' if k==2 else f'Flow {chr(65+k) if k<26 else k}',direction='horizontal' if k%2==0 else 'vertical',gap=96,
  title_style='full_width_header' if k%3==1 else 'left_title',header_alignment='center' if k%4==1 else 'left',screen_ids=[str(i) for i in range(k*members_per_section,min(grouped,(k+1)*members_per_section))]) for k in range((grouped+members_per_section-1)//members_per_section)]
 sections.append(dict(id='section-empty',name='Empty',direction='horizontal',gap=96,title_style='left_title',header_alignment='left',screen_ids=[]))
 boxes,positions=board(screens,sections,stacked=count!=100)
 bottom=max(b['y']+b['height'] for b in boxes)+SPACING
 for j,i in enumerate(range(grouped,count)): positions[str(i)]=dict(x=(j%4)*1560,y=bottom+(j//4)*1080)
 if unsectioned:
  boxes=[]
  positions={str(i):dict(x=(i%4)*1560,y=(i//4)*1080) for i in range(count)}
 boot=dict(session='fixture',revision=1,fingerprint='fixture',screens=screens,sections=boxes,arrangement='stacked' if count!=100 else 'side_by_side',layout=dict(schema_version=1,overview_mode='canvas',viewport=dict(x=30,y=50,zoom=.35),positions=positions,selected_screen_id=None),theme={},test=True,dense=dense,visible_benchmark=visible,protocol_mode=protocol,comments_showcase=comments_only)
 if light: boot['theme']=dict(bg='#f7f5fa',stage='#efedf2',surface='#ffffff',raised='#eeebf3',text='#242128',muted='#5b5561',line2='#d0cad6',ink='#484277',scheme='light')
 harness=r'''
 const nativeSend=window.ipc.postMessage;
 const messages=[];const started=performance.now();let readyAt=0,observerDeferrals=0;
 nativeSend(JSON.stringify({type:'canvas-metrics',event:'harness'}));
 window.addEventListener('error',e=>{e.stopImmediatePropagation();if(e.message==='ResizeObserver loop completed with undelivered notifications.'){observerDeferrals++;return;}nativeSend(JSON.stringify({type:'render-error',error:JSON.stringify({message:e.message,stack:e.error?.stack,file:e.filename,line:e.lineno,column:e.colno})}));},true);
 window.addEventListener('unhandledrejection',e=>nativeSend(JSON.stringify({type:'render-error',error:String(e.reason)})));
 const ticks=[];
 window.choroFixtureTick=()=>{for(const resolve of ticks.splice(0))resolve();};
 const tick=()=>new Promise(r=>ticks.push(r));
 const sleep=async ms=>{const start=performance.now();while(performance.now()-start<ms)await tick();};
 window.ipc.postMessage=raw=>{const m=JSON.parse(raw);messages.push(m);
  if(m.type==='failed'){nativeSend(JSON.stringify({type:'render-error',error:m.error}));return;}
  if(m.type==='previews')for(const r of m.requests){
   const s=window.__CHORO_CANVAS__.screens.find(s=>s.id===r.screen_id),scale=r.tier/Math.max(s.width,s.height),w=Math.round(s.width*scale),h=Math.round(s.height*scale);
   const svg=`<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><rect width="100%" height="100%" fill="#f4f5f7"/><rect x="0" y="0" width="100%" height="60" fill="#365a50"/><text x="16" y="100" font-size="24">${s.name}</text><rect x="16" y="140" width="70%" height="100" fill="#d4dad8"/></svg>`;
   window.choroCanvasReply({session:'fixture',type:'preview',screen_id:r.screen_id,content_key:r.content_key,tier:r.tier,key:r.screen_id+'-'+r.tier,width:w,height:h,test_url:window.__CHORO_CANVAS__.protocol_mode?undefined:'data:image/svg+xml,'+encodeURIComponent(svg)});
  }
  if(m.type==='ready'){nativeSend(JSON.stringify({type:'canvas-metrics',event:'ready'}));readyAt=performance.now()-started;queueMicrotask(run);}
 };
 async function run(){try{
  await sleep(400);
  if(window.__CHORO_CANVAS__.screens.length===10&&!window.__CHORO_CANVAS__.comments_showcase){
   const f=window.choroCanvasTest.flow;
   const assert=(value,label)=>{if(!value)throw Error(label);};
   const warm=f.getNode('1').data.preview;
   assert(warm,'The initial screen has a decoded preview');
   const requestsBefore=messages.filter(m=>m.type==='previews'&&m.requests.length).length;
   await f.setViewport({x:-30000,y:-30000,zoom:.35});await sleep(200);
   assert(!document.querySelector('.artboard img'),'Distant cached images are not mounted');
   await f.setViewport({x:30,y:50,zoom:.35});await sleep(250);
   assert(f.getNode('1').data.preview===warm,'Returning reuses the same decoded preview without a placeholder');
   assert(messages.filter(m=>m.type==='previews'&&m.requests.length).length===requestsBefore,'Returning to cached artboards does not request another render');
   assert(document.querySelector('.react-flow__node[data-id="1"] img').naturalWidth>0,'The retained custom-protocol image remains displayable');
   const original=f.getNodes().find(n=>n.id==='0');
   original.data.begin();f.setNodes(all=>all.map(n=>n.id==='0'?{...n,width:n.width+100,measured:{width:n.width+100,height:n.height}}:n));await sleep(60);
   dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));await sleep(60);
   original.data.end(original.width+100,original.height,0,0);await sleep(60);
   assert(f.getNodes().find(n=>n.id==='0').width===original.width,'Cancelled resize must restore authoritative width');
   assert(!messages.some(m=>m.type==='resize'),'Cancelled resize must not save');
   const node=document.querySelector('.react-flow__node[data-id="0"]');node.focus();node.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));await sleep(60);
   assert(messages.some(m=>m.type==='select'&&m.screen_id==='0')&&messages.some(m=>m.type==='open'&&m.screen_id==='0'),'Keyboard selection and open must agree');
   f.setNodes(all=>all.map(n=>n.id==='0'?{...n,position:{x:123,y:0},dragging:true}:n));await sleep(60);
   const state={session:'fixture',type:'state',revision:5,fingerprint:'newer',screens:window.__CHORO_CANVAS__.screens,layout:window.__CHORO_CANVAS__.layout};
   window.choroCanvasReply(state);await sleep(60);
   assert(f.getNodes().find(n=>n.id==='0').position.x===123,'Metadata must preserve a live drag');
   f.setNodes(all=>all.map(n=>({...n,dragging:false})));await sleep(60);
   window.choroCanvasReply({...state,screens:state.screens.map(s=>({...s,width:s.id==='0'?700:s.width}))});await sleep(60);
   window.choroCanvasReply({session:'fixture',type:'resize-result',screen_id:'0',request_id:'stale',revision:4,fingerprint:'old',screens:state.screens,positions:state.layout.positions});await sleep(60);
   assert(f.getNodes().find(n=>n.id==='0').width===700,'Stale resize response must not roll state back');
   const count=messages.filter(m=>m.type==='previews').length;
   const cameraSaves=messages.filter(m=>m.type==='camera').length;
   for(let i=0;i<12;i++){await f.setViewport({x:30+i*10,y:50,zoom:.35});await sleep(25);}
   assert(messages.filter(m=>m.type==='previews').length<=count+2,'Panning across cached screens must not spam identical preview demand');
   assert(messages.filter(m=>m.type==='camera').length===cameraSaves,'Programmatic pan must not persist camera state on every move-end');
   const saveDeadline=performance.now()+1500;
   while(messages.filter(m=>m.type==='camera').length===cameraSaves&&performance.now()<saveDeadline)await tick();
   assert(messages.filter(m=>m.type==='camera').length===cameraSaves+1,'The final camera is persisted once after movement settles: '+cameraSaves+' → '+messages.filter(m=>m.type==='camera').length);
   window.choroCanvasReply({...state,revision:6});await sleep(60);
   // ---- Sections: boards, labels, selection, drag-and-drop, menus ----
   const sec=window.__CHORO_CANVAS__.sections;
   if(sec.length){
   const A=sec[0].id,B=sec[1].id,E=sec[sec.length-1];
   const T=window.choroCanvasTest,pos=id=>({...f.getNode(id).position}),sent=t=>messages.filter(m=>m.type===t);
   const reply=(m,error=null)=>window.choroCanvasReply({session:'fixture',type:'move-result',request_id:m.request_id,error,revision:7,fingerprint:'r7',screens:state.screens,sections:sec,positions:state.layout.positions});
   assert(f.getNodes().filter(n=>n.type==='section').length===sec.length,'Every section has a board node');
   assert(document.querySelector('.react-flow__node-section .section-title')?.textContent==='Flow A','Section titles render');
   assert(document.querySelector('.section-bar'),'Full-width header variant renders');
   const surface=document.querySelector('.react-flow__node-section .section-surface').getBoundingClientRect();
   const title=document.querySelector('.react-flow__node-section .section-title').getBoundingClientRect();
   assert(title.bottom<=surface.top+1,'Left titles rest above the section surface');
   const unseenData=f.getNode('9').data;
   await f.setViewport({x:30,y:50,zoom:.1});await sleep(120);
   // Zoom may legitimately make this screen visible and decode a preview.
   const afterZoom=f.getNode('9').data;
   if(afterZoom.preview===unseenData.preview)
    assert(afterZoom===unseenData,'Section title zoom updates preserve unchanged artboard data');
   const small=document.querySelector('.react-flow__node-section .section-title').getBoundingClientRect().height;
   await f.setViewport({x:30,y:50,zoom:1});await sleep(120);
   const large=document.querySelector('.react-flow__node-section .section-title').getBoundingClientRect().height;
   assert(small>=16&&small<=45&&large>=16&&large<=45,'Titles stay readable independent of zoom: '+small+' / '+large);
   await f.setViewport({x:30,y:50,zoom:.35});await sleep(120);
   document.querySelector(`.react-flow__node[data-id="section:${A}"]`).dispatchEvent(new MouseEvent('click',{bubbles:true}));await sleep(60);
   assert(sent('select-section').some(m=>m.section_id===A),'Clicking a section selects it');
   assert(document.querySelector(`.react-flow__node[data-id="section:${A}"] .section.selected`),'The selected section uses the emphasis color');
   const moves=sent('move-screen').length;
   T.drop('1',pos('1').x+300,pos('1').y+100);await sleep(40);
   assert(sent('move-screen').length===moves,'A same-slot drop snaps back without an edit');
   const origin4=pos('4'),origin2=pos('2');
   const stationaryData=f.getNode('2').data;
   T.begin('4');T.hover('4',origin2.x+10,origin2.y+10);await sleep(60);
   assert(f.getNode('2').data===stationaryData,'Insertion feedback preserves stationary artboard data');
   assert(document.querySelector('.section.drop-target')&&document.querySelector('.section-marker'),'Dragging shows the target section and insertion point');
   assert(pos('2').x===origin2.x,'Other screens do not reflow before a drop');
   T.escape();await sleep(60);
   assert(!document.querySelector('.section.drop-target'),'Escape clears the drop target');
   T.finish('4',origin2.x+10,origin2.y+10);await sleep(40);
   assert(sent('move-screen').length===moves,'Escape cancels the drag without saving');
   assert(pos('4').x===origin4.x&&pos('4').y===origin4.y,'Escape restores the authoritative position');
   T.begin('5');
   window.choroCanvasReply({...state,revision:7,fingerprint:'r7',sections:sec.map(s=>s.id===B?{...s,name:'Renamed while dragging'}:s)});await sleep(60);
   T.finish('5',origin2.x+10,origin2.y+10);await sleep(40);
   const move=sent('move-screen').at(-1);
   assert(move&&move.screen_id==='5'&&move.section_id===A&&move.before_screen_id==='2','A drop requests the target section and insertion point');
   assert(move.revision===6&&move.fingerprint==='newer','A drop carries the interaction-start revision despite metadata during the drag');
   assert(f.getNode('5').data.pending&&!f.getNode('5').draggable,'A dropped screen waits for the host');
   window.choroCanvasReply({session:'fixture',type:'move-result',request_id:'stale',revision:9,fingerprint:'x',screens:state.screens,sections:sec,positions:{}});await sleep(40);
   assert(f.getNode('5').data.pending,'An uncorrelated move result is ignored');
   reply(move,'Studio conflict');await sleep(60);
   assert(!f.getNode('5').data.pending&&pos('5').x===state.layout.positions['5'].x&&pos('5').y===state.layout.positions['5'].y,'A conflict rolls back to authoritative positions');
   T.drop('3',-6000,-6000);await sleep(40);
   const out=sent('move-screen').at(-1);
   assert(out.screen_id==='3'&&out.section_id===null&&out.position.x===-6000,'Dragging out of a section uses the drop position');
   reply(out);await sleep(40);
   T.drop('7',E.x+100,E.y+200);await sleep(40);
   const into=sent('move-screen').at(-1);
   assert(into.screen_id==='7'&&into.section_id===E.id&&into.before_screen_id===null,'Empty sections accept drops');
   reply(into);await sleep(40);
   const p8=pos('8');await f.setViewport({x:200-p8.x*.35,y:200-p8.y*.35,zoom:.35});await sleep(120);
   document.querySelector('.react-flow__node[data-id="8"]').dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:300,clientY:300}));await sleep(60);
   assert(document.querySelector('.canvas-menu'),'Right-click opens the canvas menu');
   [...document.querySelectorAll('.canvas-menu button')].find(b=>b.textContent==='Move to section').click();await sleep(40);
   [...document.querySelectorAll('.canvas-submenu button')].find(b=>b.textContent==='Flow A').click();await sleep(40);
   const viaMenu=sent('move-screen').at(-1);
   assert(viaMenu.screen_id==='8'&&viaMenu.section_id===A&&viaMenu.revision===7,'Move to section works from the canvas menu');
   assert(!document.querySelector('.canvas-menu'),'The menu closes after an action');
   assert(f.getNode('8').data.pending,'Menu moves await a correlated host response');
   reply(viaMenu,'Menu move conflict');await sleep(60);
   assert(!f.getNode('8').data.pending&&f.getNode('8').data.error==='Menu move conflict','Menu failures unlock the screen and surface the conflict');
   await f.setViewport({x:30,y:50,zoom:.35});await sleep(120);
   document.querySelector(`.react-flow__node[data-id="section:${A}"]`).dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:300,clientY:300}));await sleep(60);
   [...document.querySelectorAll('.canvas-menu button')].find(b=>b.textContent==='Ungroup section').click();await sleep(40);
   const ungroup=sent('context-action').at(-1);
   assert(ungroup&&ungroup.section_id===A&&ungroup.action==='ungroup'&&ungroup.screen_id===null,'Section menu sends validated context actions');
   window.choroCanvasReply({session:'fixture',type:'command',command:'fit-section',section_id:B});await sleep(120);
   const v=f.getViewport(),b=sec[1];
   assert(b.y*v.zoom+v.y>=-1&&(b.y+b.height)*v.zoom+v.y<=innerHeight+1,'Fit section includes its header and screens');
   await f.setViewport({x:30,y:50,zoom:.35});await sleep(120);
   }
  }
  const progress=event=>nativeSend(JSON.stringify({type:'canvas-metrics',event,elapsed_ms:performance.now()-started}));
  if(window.__CHORO_CANVAS__.screens.length===10){progress('comments-start');await window.runCanvasCommentsTest({sleep,messages,progress});progress('comments-done');}
  if(document.querySelector('iframe'))throw Error('Live HTML mounted in overview');
  const flow=window.choroCanvasTest.flow,frames=[];let prev=0;
  const frame=fn=>window.__CHORO_CANVAS__.visible_benchmark?requestAnimationFrame(fn):tick().then(()=>fn(performance.now()));
  await new Promise(resolve=>{let n=0;function step(t){if(prev)frames.push(t-prev);prev=t;
   if(n%30===0)nativeSend(JSON.stringify({type:'canvas-metrics',event:'gesture-progress',step:n}));
   flow.setViewport({x:30-(n%90)*(window.__CHORO_CANVAS__.dense?4:12),y:50-(n%60)*4,zoom:.35+(n%60)*.001});
   if(++n<90)frame(step);else resolve();}frame(step);});
  await sleep(250);const stats=window.choroCanvasStats();frames.sort((a,b)=>a-b);
  if(stats.decoded_bytes+stats.decoding_bytes>64*1024*1024||stats.decoding>2||stats.iframes!==0)throw Error('Resource budget exceeded');
  if(stats.mounted>=window.__CHORO_CANVAS__.screens.length&&window.__CHORO_CANVAS__.screens.length>=50)throw Error('Offscreen nodes not culled');
  if(stats.sections_mounted>=window.__CHORO_CANVAS__.sections.length&&window.__CHORO_CANVAS__.sections.length>=8)throw Error('Offscreen sections not culled: '+JSON.stringify({stats,viewport:flow.getViewport(),mounted:[...document.querySelectorAll('.react-flow__node-section')].map(n=>n.dataset.id).slice(0,8)}));
  nativeSend(JSON.stringify({type:'canvas-metrics',count:window.__CHORO_CANVAS__.screens.length,ready_ms:readyAt,observer_deferrals:observerDeferrals,protocol_mode:window.__CHORO_CANVAS__.protocol_mode,display_scale:devicePixelRatio,visible_benchmark:window.__CHORO_CANVAS__.visible_benchmark,p95_frame_ms:window.__CHORO_CANVAS__.visible_benchmark?frames[Math.floor(frames.length*.95)]:null,max_frame_ms:window.__CHORO_CANVAS__.visible_benchmark?Math.max(...frames):null,...stats}));
  // Save a useful overview after recording the gesture/resource metrics.
  window.choroCanvasReply({session:'fixture',type:'command',command:'fit-all'});await sleep(300);
  if(window.__CHORO_CANVAS__.comments_showcase){
   window.choroCanvasReply({session:'fixture',type:'comment-mode',enabled:true});await sleep(60);
   const read=messages.filter(m=>m.type==='comments-read').at(-1);
   const n=flow.getNode('1');await flow.setViewport({x:60-n.position.x*.65,y:100-n.position.y*.65,zoom:.65});await sleep(80);
   window.choroCanvasReply({session:'fixture',type:'comments-result',request_id:read.request_id,comments:{schema_version:1,revision:1,pins:[
    {id:'showcase',screen_id:'1',x:.25,y:.45,body:'Make this action easier to find.',resolved:false,created_at:1791140000},
    {id:'showcase-secondary',screen_id:'1',x:.65,y:.7,body:'Give the secondary action a clearer label.',resolved:false,created_at:1791140040},
    {id:'showcase-navigation',screen_id:'8',x:.5,y:.4,body:'Check the navigation on this screen.',resolved:false,created_at:1791140080}
   ]}});await sleep(60);
   document.querySelector('.comment-pin').click();await sleep(60);
  }
  nativeSend(JSON.stringify({type:'thumbnail-ready'}));
 }catch(e){nativeSend(JSON.stringify({type:'render-error',error:String(e)}));}}
 '''
 comment_tests=(root/'canvas-comments.test.js').read_text()
 html='<!doctype html><html><head><style>'+css+'</style></head><body><div id="root"></div><script>window.__CHORO_CANVAS__='+json.dumps(boot)+';'+comment_tests+harness+'</script><script>'+js+'</script></body></html>'
 job=dict(html=html,output=str(work/f'canvas-{count}.png'),width=capture_width,height=capture_height,output_width=capture_width,output_height=capture_height)
 run=subprocess.run([str(work/'probe')],input=json.dumps(job)+'\n',text=True,capture_output=True,timeout=70)
 responses=[json.loads(line) for line in run.stdout.splitlines()];assert responses and all(r.get('error') is None for r in responses),(responses,run.stderr)
 metrics=next(json.loads(line) for line in run.stderr.splitlines() if line.startswith('{"type":"canvas-metrics","count"'))
 metrics['machine']=subprocess.check_output(['sysctl','-n','hw.model'],text=True).strip()
 metrics['os']=platform.platform()
 results.append(metrics);print(json.dumps(metrics),flush=True)
 png=(work/f'canvas-{count}.png').read_bytes();assert int.from_bytes(png[16:20],'big')==capture_width and int.from_bytes(png[20:24],'big')==capture_height
 assert metrics['observer_deferrals']<30,metrics
 if not unsectioned: assert metrics['sections_mounted']>=1,metrics
 assert metrics['ready_ms']<1000,metrics
 if visible: assert metrics['p95_frame_ms']<20 and metrics['max_frame_ms']<100,metrics
(work/'metrics.json').write_text(json.dumps(results,indent=2))
print('Evidence:',work)
