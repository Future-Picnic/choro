#!/usr/bin/env python3
"""Vendor Vvveb source from the pinned checkout; no runtime downloads."""
from pathlib import Path
import re,json,subprocess,hashlib,base64,sys
src=Path(sys.argv[1]).resolve()
dst=Path(__file__).resolve().parent
files=['libs/builder/builder.js','libs/autocomplete/autocomplete.js','libs/builder/undo.js','libs/builder/inputs.js','libs/builder/components-common.js','libs/builder/components-html.js','css/editor.css','js/popper.min.js','js/bootstrap.min.js','libs/coloris/coloris.js','libs/coloris/coloris.min.css']
manifest={}
for path in files:
 data=(src/path).read_bytes(); out=dst/'upstream'/Path(path).name;out.write_bytes(data)
 manifest[out.name]={'path':path,'sha256':hashlib.sha256(data).hexdigest()}
html=(src/'editor.html').read_text()
right=html[html.index('        <div id="right-panel">'):html.index('        <div id="bottom-panel">')]
toolbar=html[html.index('                <div id="wysiwyg-editor"'):html.index('                <div id="select-actions">')]
(dst/'upstream/right-panel.html').write_text(right)
(dst/'upstream/inline-toolbar.html').write_text(toolbar)
for name,data in [('right-panel.html',right),('inline-toolbar.html',toolbar)]:
 manifest[name]={'path':'editor.html (verbatim fragment)','sha256':hashlib.sha256(data.encode()).hexdigest()}
# Compile upstream templates using upstream's own compiler, at vendor time rather than unsafe-eval at runtime.
templates={m[1]:m[2] for m in re.finditer(r'<script id="([^"]+)" type="text/html">(.*?)</script>',html,re.S)}
compiler=(src/'libs/builder/builder.js').read_text().split('function buildParams')[0]
js=compiler+'\nconst templates='+json.dumps(templates)+';\nprocess.stdout.write(JSON.stringify(Object.fromEntries(Object.entries(templates).map(([id, html])=>[id,tmpl(html).toString()]))));'
js='const vm=require("vm"); const ctx={process}; vm.createContext(ctx); vm.runInContext('+json.dumps(js)+',ctx);'
functions=json.loads(subprocess.check_output(['node','-e',js],text=True))
compiled='// Compiled by the pinned upstream template compiler.\nconst studioTemplates={\n'+',\n'.join(json.dumps(k)+':'+v for k,v in functions.items())+'\n};\nwindow.tmpl=(id,data)=>{const fn=studioTemplates[id];if(!fn)throw new Error("Unbundled Vvveb template: "+id);return data ? fn(data) : fn;};\n'
(dst/'templates.js').write_text(compiled)
css=(src/'css/editor.css').read_text()
faces=re.findall(r'@font-face\s*\{.*?\}',css,re.S)
fontcss=[]
for face in faces:
 paths=re.findall(r'url\([\'"]?([^\'"\)]+)',face)
 choice=next((p for p in paths if '.woff2' in p),next((p for p in paths if '.woff' in p),None))
 if not choice: continue
 path=(src/'css'/choice.split('?')[0].split('#')[0]).resolve()
 if not path.exists(): continue
 ext=path.suffix[1:]; data=path.read_bytes()
 family=re.search(r'font-family:\s*([^;]+);',face)[1]
 weight=re.search(r'font-weight:\s*([^;]+);',face)
 fontcss.append('@font-face{font-family:'+family+';font-style:normal;font-weight:'+(weight[1] if weight else 'normal')+';src:url(data:font/'+ext+';base64,'+base64.b64encode(data).decode()+') format("'+ext+'");font-display:block;}')
 manifest[path.name]={'path':str(path.relative_to(src.resolve())),'sha256':hashlib.sha256(data).hexdigest()}
(dst/'fonts.css').write_text('\n'.join(fontcss))
(dst/'upstream-manifest.json').write_text(json.dumps({'revision':'1acbab7ebfe3e7b004f1f18c039d26550fc04bd8','files':manifest},indent=2)+'\n')
print('Vendored',len(files),'unchanged files;',len(templates),'upstream templates;',len(fontcss),'font faces')
