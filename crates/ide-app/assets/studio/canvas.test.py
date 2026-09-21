#!/usr/bin/env python3
"""Isolated WebKit canvas correctness + frame pacing probe; never controls Choro UI."""
import json,pathlib,subprocess,tempfile,sys,time,platform
root=pathlib.Path(__file__).resolve().parent
bundle=root.parents[1]/'web/studio-canvas/dist'
work=pathlib.Path(tempfile.mkdtemp(prefix='choro-canvas-check-'))
# The fixture uses the same WKWebView configuration as the existing Studio helper.
visible='--visible' in sys.argv
protocol='--protocol' in sys.argv
swift=(root/'thumbnail.swift').read_text().replace('deadline:.now()+10','deadline:.now()+30')
swift=swift.replace('window.addEventListener(\\"error\\",e=>window.ipc.postMessage(JSON.stringify({type:\\"render-error\\",error:e.message})));', '')
swift=swift.replace('baseURL:nil','baseURL:URL(string:"https://canvas.invalid/")').replace('url == "about:blank"','url == "https://canvas.invalid/" || url == "about:blank"')
swift=swift.replace('if type == "render-error"', 'if type == "canvas-metrics" { fputs(raw+"\\n",stderr);return }\n        if type == "render-error"')
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
results=[]
for count in [10,50,100,200]:
 screens=[dict(id=str(i),name=f'Screen {i+1}',width=1440 if i%3 else 390,height=960,archived=False,content_key=str(i)) for i in range(count)]
 positions={str(i):dict(x=(i%4)*1560,y=(i//4)*1080) for i in range(count)}
 boot=dict(session='fixture',revision=1,fingerprint='fixture',screens=screens,layout=dict(schema_version=1,overview_mode='canvas',viewport=dict(x=30,y=50,zoom=.35),positions=positions,selected_screen_id=None),theme={},test=True,visible_benchmark=visible,protocol_mode=protocol)
 harness=r'''
 const nativeSend=window.ipc.postMessage;
 const messages=[];const started=performance.now();let readyAt=0,observerDeferrals=0;
 nativeSend(JSON.stringify({type:'canvas-metrics',event:'harness'}));
 window.addEventListener('error',e=>{e.stopImmediatePropagation();if(e.message==='ResizeObserver loop completed with undelivered notifications.'){observerDeferrals++;return;}nativeSend(JSON.stringify({type:'render-error',error:JSON.stringify({message:e.message,stack:e.error?.stack,file:e.filename,line:e.lineno,column:e.colno})}));},true);
 window.addEventListener('unhandledrejection',e=>nativeSend(JSON.stringify({type:'render-error',error:String(e.reason)})));
 const tick=()=>new Promise(r=>{const c=new MessageChannel();c.port1.onmessage=()=>{c.port1.close();c.port2.close();r();};c.port2.postMessage(0);});
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
  if(window.__CHORO_CANVAS__.screens.length===10){
   const f=window.choroCanvasTest.flow;
   const assert=(value,label)=>{if(!value)throw Error(label);};
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
   for(let i=0;i<12;i++){await f.setViewport({x:30+i*10,y:50,zoom:.35});await sleep(25);}
   assert(messages.filter(m=>m.type==='previews').length>count,'Moving camera must request visible previews');
   window.choroCanvasReply({...state,revision:6});await sleep(60);
  }
  if(document.querySelector('iframe'))throw Error('Live HTML mounted in overview');
  const flow=window.choroCanvasTest.flow,frames=[];let prev=0;
  const frame=fn=>window.__CHORO_CANVAS__.visible_benchmark?requestAnimationFrame(fn):tick().then(()=>fn(performance.now()));
  await new Promise(resolve=>{let n=0;function step(t){if(prev)frames.push(t-prev);prev=t;
   flow.setViewport({x:30-(n%90)*12,y:50-(n%60)*4,zoom:.35+(n%60)*.001});
   if(++n<90)frame(step);else resolve();}frame(step);});
  await sleep(250);const stats=window.choroCanvasStats();frames.sort((a,b)=>a-b);
  if(stats.decoded_bytes>64*1024*1024||stats.decoding>2||stats.iframes!==0)throw Error('Resource budget exceeded');
  if(stats.mounted>=window.__CHORO_CANVAS__.screens.length&&window.__CHORO_CANVAS__.screens.length>=50)throw Error('Offscreen nodes not culled');
  nativeSend(JSON.stringify({type:'canvas-metrics',count:window.__CHORO_CANVAS__.screens.length,ready_ms:readyAt,observer_deferrals:observerDeferrals,protocol_mode:window.__CHORO_CANVAS__.protocol_mode,display_scale:devicePixelRatio,visible_benchmark:window.__CHORO_CANVAS__.visible_benchmark,p95_frame_ms:window.__CHORO_CANVAS__.visible_benchmark?frames[Math.floor(frames.length*.95)]:null,max_frame_ms:window.__CHORO_CANVAS__.visible_benchmark?Math.max(...frames):null,...stats}));
  nativeSend(JSON.stringify({type:'thumbnail-ready'}));
 }catch(e){nativeSend(JSON.stringify({type:'render-error',error:String(e)}));}}
 '''
 html='<!doctype html><html><head><style>'+css+'</style></head><body><div id="root"></div><script>window.__CHORO_CANVAS__='+json.dumps(boot)+';'+harness+'</script><script>'+js+'</script></body></html>'
 job=dict(html=html,output=str(work/f'canvas-{count}.png'),width=1400,height=900,output_width=1400,output_height=900)
 run=subprocess.run([str(work/'probe')],input=json.dumps(job)+'\n',text=True,capture_output=True,timeout=40)
 responses=[json.loads(line) for line in run.stdout.splitlines()];assert responses and all(r.get('error') is None for r in responses),(responses,run.stderr)
 metrics=next(json.loads(line) for line in run.stderr.splitlines() if line.startswith('{"type":"canvas-metrics","count"'))
 metrics['machine']=subprocess.check_output(['sysctl','-n','hw.model'],text=True).strip()
 metrics['os']=platform.platform()
 results.append(metrics);print(json.dumps(metrics),flush=True)
 png=(work/f'canvas-{count}.png').read_bytes();assert int.from_bytes(png[16:20],'big')==1400 and int.from_bytes(png[20:24],'big')==900
 assert metrics['observer_deferrals']<30,metrics
 assert metrics['ready_ms']<1000,metrics
 if visible: assert metrics['p95_frame_ms']<20 and metrics['max_frame_ms']<100,metrics
(work/'metrics.json').write_text(json.dumps(results,indent=2))
print('Evidence:',work)
