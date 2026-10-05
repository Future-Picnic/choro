/** One persistent trusted editor overlay, clipped to its artboard and controls.
 * Empty canvas pixels remain hit-testable by React Flow. Culling a node never
 * unmounts the active editor or loses a draft. */
export type InlineDescriptor = { screen_id: string; editor_session: string; document: string };
type Rect = { x: number; y: number; width: number; height: number };
export function createInlineEditor(
  send: (type: string, data?: Record<string, unknown>) => void,
  camera: (input: any, screen: string, area?: Rect) => void,
  changed: () => void,
) {
  let descriptor: InlineDescriptor | null = null, frame: HTMLIFrameElement | null = null;
  let pendingFlush: string | null = null, timeout: ReturnType<typeof setTimeout> | undefined;
  let area: Rect | undefined;
  let pendingFocus = false;
  let lastPosition = '';
  let ready=false, activation: {x:number;y:number}|null=null;
  const activate = () => { if(ready&&activation&&descriptor){reply({session:descriptor.editor_session,type:'activate',...activation});activation=null;} };
  const focus = () => {
    pendingFocus = true;
    if (descriptor && area && area.width > 0 && area.height > 0) {
      pendingFocus = false;
      camera({kind:'fit-screen'}, descriptor.screen_id, area);
    }
  };
  const validRect = (r: Rect) => r && [r.x,r.y,r.width,r.height].every(n=>Number.isFinite(n)&&Math.abs(n)<1e7) && r.width>=0 && r.height>=0;
  const reply = (value: any) => {
    if (descriptor && value.session===descriptor.editor_session)
      frame?.contentWindow?.postMessage({type:'studio-reply',session:descriptor.editor_session,reply:value},'*');
  };
  const close = () => {
    clearTimeout(timeout);frame?.remove();frame=null;descriptor=null;area=undefined;pendingFlush=null;pendingFocus=false;lastPosition='';ready=false;activation=null;changed();
  };
  const listener = (event: MessageEvent) => {
    if (!descriptor || event.source!==frame?.contentWindow || event.data?.session!==descriptor.editor_session) return;
    const value=event.data;
    if(value.type==='studio-editor'){
      if(value.message?.session!==descriptor.editor_session)return;
      if(value.message.type==='ready'){clearTimeout(timeout);lastPosition='';ready=true;camera({kind:'sync'},descriptor.screen_id,area);activate();}
      if(value.message.type==='flushed' && pendingFlush && value.message.request_id===pendingFlush){
        const request_id=pendingFlush;pendingFlush=null;send('flushed',{request_id});return;
      }
      send('inline-editor',{message:value.message});
    }else if(value.type==='studio-inline-camera'){
      camera(value.input,descriptor.screen_id,area);
    }else if(value.type==='studio-inline-layout' && Array.isArray(value.rectangles) && value.rectangles.length<=8 && value.rectangles.every(validRect)){
      if(validRect(value.area))area=value.area;
      // Separate, clockwise subpaths form a union, including floating text/color
      // controls. The rest of this full-size iframe is not a pointer target.
      const paths=value.rectangles.map((r:Rect)=>{
        const x=Math.max(0,r.x),y=Math.max(0,r.y),right=Math.min(innerWidth,r.x+r.width),bottom=Math.min(innerHeight,r.y+r.height);
        return right>x&&bottom>y?`M${x} ${y}H${right}V${bottom}H${x}Z`:'';
      }).join(' ');
      if(frame){frame.style.clipPath=paths?`path("${paths}")`:'inset(50%)';frame.style.visibility='visible';}
      if(pendingFocus)focus();
    }
  };
  // Keyboard events inside the authored frame are handled by editor.js. Empty
  // canvas space belongs to this document, so route its shortcuts to that same
  // live editor rather than creating a second history or opening another frame.
  const onHistoryKey = (event: KeyboardEvent) => {
    if (!descriptor || !ready || event.defaultPrevented || event.isComposing || event.altKey || !(event.metaKey || event.ctrlKey)) return;
    const target = event.target;
    if (target instanceof Element && (target.closest('input,textarea,select,[contenteditable]:not([contenteditable="false"])'))) return;
    const key = event.key.toLowerCase();
    const undo = key === 'z' || event.code === 'KeyZ';
    const redo = !event.shiftKey && (key === 'y' || event.code === 'KeyY');
    if (!undo && !redo) return;
    event.preventDefault();event.stopPropagation();
    reply({session:descriptor.editor_session,type:'toolbar-command',command:redo || event.shiftKey ? 'redo' : 'undo'});
  };
  addEventListener('message',listener);
  addEventListener('keydown',onHistoryKey);
  return {
    get screen(){return descriptor?.screen_id;},
    get session(){return descriptor?.editor_session;},
    get area(){return area;},
    commentMode(enabled:boolean){
      if(frame){frame.style.pointerEvents=enabled?'none':'';if(enabled){frame.blur();document.body.tabIndex=-1;document.body.focus();}}
      if(descriptor)reply({session:descriptor.editor_session,type:'comment-mode',enabled});
    },
    focus,
    activate(x:number,y:number){
      if(descriptor&&Number.isFinite(x)&&Number.isFinite(y)){activation={x,y};activate();}
    },
    open(next: InlineDescriptor | null){
      if(!next){close();return;}
      if(typeof next.document!=='string'||!next.editor_session)return;
      if(descriptor?.editor_session===next.editor_session)return;
      close();descriptor={...next,document:''};
      frame=document.createElement('iframe');frame.className='inline-editor';frame.title='Edit screen on canvas';
      frame.setAttribute('sandbox','allow-scripts allow-same-origin');frame.style.visibility='hidden';frame.style.clipPath='inset(50%)';
      frame.srcdoc=next.document;document.body.append(frame);changed();
      timeout=setTimeout(()=>send('editor-failed',{screen_id:next.screen_id,error:'The inline editor did not load. Your saved screen and recovery draft are preserved.'}),10000);
    },
    position(x:number,y:number,zoom:number){
      const key=x+':'+y+':'+zoom;
      if(descriptor&&key!==lastPosition){lastPosition=key;frame?.contentWindow?.postMessage({type:'studio-camera',session:descriptor.editor_session,x,y,zoom},'*');}
    },
    reply,
    theme(theme:Record<string,string>){if(descriptor)reply({session:descriptor.editor_session,type:'theme',theme});},
    flush(request_id:string){
      if(!descriptor){send('flushed',{request_id});return;}
      pendingFlush=request_id;reply({session:descriptor.editor_session,type:'flush',request_id});
    },
    dispose(){close();removeEventListener('message',listener);removeEventListener('keydown',onHistoryKey);},
  };
}
