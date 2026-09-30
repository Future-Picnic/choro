// Compiled by the pinned upstream template compiler.
const studioTemplates={
"vvveb-input-textinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>   <input name="',key,'" type="text" class="form-control"/>  </div>   ');}return p.join('');
},
"vvveb-input-textareainput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>   <textarea name="',key,'" '); if (typeof rows !== 'undefined') { p.push(' rows="',rows,'" '); } else { p.push(' rows="3" '); } p.push(' class="form-control"></textarea>   </div>   ');}return p.join('');
},
"vvveb-input-checkboxinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div class="form-check'); if (typeof className !== 'undefined') { p.push(' ',className,''); } p.push('">     <input name="',key,'" class="form-check-input" type="checkbox" id="',key,'_check">     <label class="form-check-label" for="',key,'_check">'); if (typeof text !== 'undefined') { p.push(' ',text,' '); } p.push('</label>  </div>   ');}return p.join('');
},
"vvveb-input-radioinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>     '); for ( var i = 0; i < options.length; i++ ) { p.push('    <label class="form-check-input  '); if (typeof inline !== 'undefined' && inline == true) { p.push('custom-control-inline'); } p.push('"  title="',options[i].title,'">     <input name="',key,'" class="form-check-input" type="radio" value="',options[i].value,'" id="',key,'',i,'" ');if (options[i].checked) { p.push('checked="',options[i].checked,'"'); } p.push('>     <label class="form-check-label" for="',key,'',i,'">',options[i].text,'</label>   </label>    '); } p.push('   </div>   ');}return p.join('');
},
"vvveb-input-radiobuttoninput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div class="btn-group btn-group-sm ');if (extraclass) { p.push('',extraclass,''); } p.push('" role="group">   '); var namespace = 'rb-' + Math.floor(Math.random() * 100); p.push('     '); for ( var i = 0; i < options.length; i++ ) { p.push('    <input name="',key,'" class="btn-check" type="radio" value="',options[i].value,'" id="',namespace,'',key,'',i,'" ');if (options[i].checked) { p.push('checked="',options[i].checked,'"'); } p.push(' autocomplete="off">   <label class="btn btn-outline-secondary ');if (options[i].extraclass) { p.push('',options[i].extraclass,''); } p.push('" for="',namespace,'',key,'',i,'" title="',options[i].title,'">     ');if (options[i].icon) { p.push('<i class="',options[i].icon,'"></i>'); } p.push('     ',options[i].text,'   </label>    '); } p.push('       </div>   ');}return p.join('');
},
"vvveb-input-toggle":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('      <div class="form-check form-switch '); if (typeof className !== 'undefined') { p.push(' ',className,''); } p.push('">         <input    type="checkbox"    name="',key,'"    value="',on,'"    ');if (off !== null) { p.push(' data-value-off="',off,'" '); } p.push('   ');if (on !== null) { p.push(' data-value-on="',on,'" '); } p.push('    class="form-check-input" type="checkbox" role="switch"   id="',key,'">         <label class="form-check-label"for="',key,'">         </label>     </div>   ');}return p.join('');
},
"vvveb-input-header":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('    <h6 class="header">',header,'</h6>   ');}return p.join('');
},
"vvveb-input-select":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>    <select class="form-select" name="',key,'">    '); var optgroup = false; for ( var i = 0; i < options.length; i++ ) { p.push('     '); if (options[i].optgroup) {  p.push('      '); if (optgroup) {  p.push('       </optgroup>      '); } p.push('      <optgroup label="',options[i].optgroup,'">     '); optgroup = true; } else { p.push('    <option value="',options[i].value,'"      ');       for (attr in options[i]) {        if (attr != "value" && attr != "text") {        p.push('         ',attr,'=',options[i][attr],'        '); }       } p.push('>    ',options[i].text,'</option>    '); } } p.push('   </select>    </div>   ');}return p.join('');
},
"vvveb-input-icon-select":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div class="input-list-select">      <div class="elements">    <div class="row">     '); for ( var i = 0; i < options.length; i++ ) { p.push('     <div class="col">      <div class="element">       ',options[i].value,'       <label>',options[i].text,'</label>      </div>     </div>     '); } p.push('    </div>   </div>  </div>   ');}return p.join('');
},
"vvveb-input-html-list-select":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div class="input-html-list-select">      <div class="current-element">   </div>      <div class="popup">    <select class="form-select">     '); var optgroup = false; for ( var i = 0; i < options.length; i++ ) { p.push('      '); if (options[i].optgroup) {  p.push('       '); if (optgroup) {  p.push('        </optgroup>       '); } p.push('       <optgroup label="',options[i].optgroup,'">      '); optgroup = true; } else { p.push('      <option value="',options[i].value,'">',options[i].text,'</option>     '); } } p.push('    </select>        <div class="search">       <input class="form-control search" placeholder="Search elements" type="text">       <button class="clear-backspace">        <i class="la la-times"></i>       </button>    </div>        <div class="elements">     ',elements,'    </div>   </div>  </div>   ');}return p.join('');
},
"vvveb-input-html-list-dropdown":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div class="input-html-list-select" '); if (typeof id !== "undefined") { p.push(' id=',id,' '); } p.push('>      <div class="current-element">      </div>      <div class="popup">    <select class="form-select">     '); var optgroup = false; for ( var i = 0; i < options.length; i++ ) { p.push('      '); if (options[i].optgroup) {  p.push('       '); if (optgroup) {  p.push('        </optgroup>       '); } p.push('       <optgroup label="',options[i].optgroup,'">      '); optgroup = true; } else { p.push('      <option value="',options[i].value,'">',options[i].text,'</option>     '); } } p.push('    </select>       <div class="search">       <input class="form-control search" placeholder="Search elements" type="text">       <button class="clear-backspace">        <i class="la la-times"></i>       </button>    </div>        <div class="elements">      ',elements,'    </div>   </div>  </div>   ');}return p.join('');
},
"vvveb-input-dateinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>   <input name="',key,'" type="date" class="form-control"     '); if (typeof min_date === 'undefined') { p.push(' min="',min_date,'" '); } p.push(' '); if (typeof max_date === 'undefined') { p.push(' max="',max_date,'" '); } p.push('   />  </div>   ');}return p.join('');
},
"vvveb-input-listinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div class="sections-container p-0">    '); for ( var i = 0; i < options.length; i++ ) { p.push('   <div class="section-item" draggable="true">    <div class="controls">     <div class="handle"></div>     <div class="info">      <div class="name">',options[i].name,'       <div class="type">',options[i].type,'</div>      </div>     </div>     <div class="buttons">      <a class="delete-btn" href="" title="Remove section"><i class="icon-trash-outline text-danger"></i></a>      <!--       <a class="up-btn" href="" title="Move element up"><i class="la la-arrow-up"></i></a>      <a class="down-btn" href="" title="Move element down"><i class="la la-arrow-down"></i></a>      <a class="properties-btn" href="" title="Properties"><i class="icon-settings-outline"></i></a>      -->    </div>    </div>      <input class="header_check" type="checkbox" id="section-components-',key,'-',i,'">     <label for="section-components-',key,'-',i,'">      <div class="header-arrow"></div>    </label>        <div class="tree">     ',options[i].name,'    </div>   </div>         '); } p.push('     '); if (typeof hide_remove === 'undefined') { p.push('   <div class="mt-3">       <button class="btn btn-sm btn-outline-primary btn-new btn-icon">     <i class="la la-plus la-lg"></i> Add new    </button>       </div>   '); } p.push('      </div>   ');}return p.join('');
},
"vvveb-input-grid":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div class="row g-2">   <div class="col-6">       <label class="form-label">Extra small</label>    <select class="form-select" name="col" autocomplete="off">          <option value="">None</option>     '); for ( var i = 1; i <= 12; i++ ) { p.push('     <option value="',i,'" '); if ((typeof col !== 'undefined') && col == i) { p.push(' selected '); } p.push('>',i,'</option>     '); } p.push('         </select>   </div>         <div class="col-6">    <label class="form-label">Small</label>    <select class="form-select" name="col-sm" autocomplete="off">          <option value="">None</option>     '); for ( var i = 1; i <= 12; i++ ) { p.push('     <option value="',i,'" '); if ((typeof col_sm !== 'undefined') && col_sm == i) { p.push(' selected '); } p.push('>',i,'</option>     '); } p.push('         </select>   </div>      <div class="col-6">    <label class="form-label">Medium</label>    <select class="form-select" name="col-md" autocomplete="off">          <option value="">None</option>     '); for ( var i = 1; i <= 12; i++ ) { p.push('     <option value="',i,'" '); if ((typeof col_md !== 'undefined') && col_md == i) { p.push(' selected '); } p.push('>',i,'</option>     '); } p.push('         </select>   </div>      <div class="col-6">    <label class="form-label">Large</label>    <select class="form-select" name="col-lg" autocomplete="off">          <option value="">None</option>     '); for ( var i = 1; i <= 12; i++ ) { p.push('     <option value="',i,'" '); if ((typeof col_lg !== 'undefined') && col_lg == i) { p.push(' selected '); } p.push('>',i,'</option>     '); } p.push('         </select>   </div>         <div class="col-6">    <label class="form-label">Extra large </label>    <select class="form-select" name="col-xl" autocomplete="off">          <option value="">None</option>     '); for ( var i = 1; i <= 12; i++ ) { p.push('     <option value="',i,'" '); if ((typeof col_xl !== 'undefined') && col_xl == i) { p.push(' selected '); } p.push('>',i,'</option>     '); } p.push('         </select>   </div>      <div class="col-6">    <label class="form-label">Extra extra large</label>    <select class="form-select" name="col-xxl" autocomplete="off">          <option value="">None</option>     '); for ( var i = 1; i <= 12; i++ ) { p.push('     <option value="',i,'" '); if ((typeof col_xxl !== 'undefined') && col_xxl == i) { p.push(' selected '); } p.push('>',i,'</option>     '); } p.push('         </select>   </div>      '); if (typeof hide_remove === 'undefined') { p.push('   <div class="col-12">       <button class="btn btn-sm btn-outline-secondary text-danger">     <i class="la la-trash la-lg"></i> Remove    </button>       </div>   '); } p.push('     </div>   ');}return p.join('');
},
"vvveb-input-textvalue":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div class="row">   <div class="col-6 mb-1">    <label class="form-label">Value</label>    <input name="value" type="text" value="',value,'" class="form-control" autocomplete="off" />   </div>    <div class="col-6 mb-1">    <label class="form-label">Text</label>    <input name="text" type="text" value="',text,'" class="form-control" autocomplete="off" />   </div>    '); if (typeof hide_remove === 'undefined') { p.push('   <div class="col-12">       <button class="btn btn-sm btn-outline-secondary text-danger">     <i class="la la-trash la-lg"></i> Remove    </button>       </div>   '); } p.push('   </div>   ');}return p.join('');
},
"vvveb-input-rangeinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div class="input-range">      <input name="',key,'" type="range" min="',min,'" max="',max,'" step="',step,'" class="form-range" data-input-value/>   <input name="',key,'" type="number" min="',min,'" max="',max,'" step="',step,'" class="form-control" data-input-value/>  </div>   ');}return p.join('');
},
"vvveb-input-imageinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>   <input name="',key,'" type="text" class="form-control"/>   <input name="file" type="file" class="form-control"/>  </div>   ');}return p.join('');
},
"vvveb-input-imageinput-gallery":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>   <img id="thumb-',key,'" class="img-thumbnail p-0" data-target-input="#input-',key,'" data-target-thumb="#thumb-',key,'" style="cursor:pointer" src="" width="225" height="225">   <input name="',key,'" type="text" class="form-control mt-1" id="input-',key,'"/>   <button name="button" class="btn btn-outline-primary btn-sm btn-icon mt-2 w-100" data-target-input="#input-',key,'" data-target-thumb="#thumb-',key,'"><i class="la la-image la-lg me-1"></i><span>Set image</span></button>  </div>   ');}return p.join('');
},
"vvveb-input-svginput-gallery":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>   <svg id="thumb-',key,'" class="img-thumbnail p-0" data-target-input="#input-',key,'" data-target-thumb="#thumb-',key,'" style="cursor:pointer" src="" width="225" height="225"></svg>   <input name="',key,'" type="text" class="form-control mt-1 d-none" id="input-',key,'"/>   <button name="button" class="btn btn-outline-primary btn-sm btn-icon mt-2 w-100" data-target-input="#input-',key,'" data-target-thumb="#thumb-',key,'"><i class="la la-image la-lg me-1"></i><span>Set image</span></button>  </div>   ');}return p.join('');
},
"vvveb-input-videoinput-gallery":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>   <video id="thumb-v',key,'" class="img-thumbnail p-0" data-target-input="#input-v',key,'" data-target-thumb="#thumb-v',key,'" style="cursor:pointer" src="" width="225" height="225" playsinline loop muted controls></video>   <input name="v',key,'" type="text" class="form-control mt-1" id="input-v',key,'"/>   <button name="button" class="btn btn-secondary btn-sm btn-icon mt-2 w-100" data-target-input="#input-v',key,'" data-target-thumb="#thumb-v',key,'"><i class="la la-video la-lg"></i><span>Set video</span></button>  </div>   ');}return p.join('');
},
"vvveb-input-colorinput-native":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>   <input name="',key,'" ');  if (typeof palette !== 'undefined') { p.push(' list="',key,'-color-palette" '); } p.push(' type="color" '); if (typeof value !== 'undefined' && value != false) { p.push(' value="',value,'" '); } p.push('  pattern="#[a-f0-9]{6}" class="form-control form-control-color" placeholder="#000000"/>   ');  if (typeof palette !== 'undefined') { p.push('   <datalist id="',key,'-color-palette">    '); for (const color in palette) { p.push('     <option>',palette[color],'</option>    '); } p.push('     '); } p.push('  </div>   ');}return p.join('');
},
"vvveb-input-colorinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div class="clr-field" style="'); if (typeof value !== 'undefined' && value != false) { p.push('color: ',value,'" '); } p.push('">   <button type="button" aria-labelledby="clr-open-label"></button>   <input name="',key,'" type="text" '); if (typeof value !== 'undefined' && value != false) { p.push(' value="',value,'" '); } p.push(' class="coloris form-control"/>    </div>   ');}return p.join('');
},
"vvveb-input-bootstrap-color-picker-input":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>   <div id="cp2" class="input-group" title="Using input value">     <input name="',key,'" type="text" '); if (typeof value !== 'undefined' && value != false) { p.push(' value="',value,'" '); } p.push('  class="form-control"/>     <span class="input-group-append">    <span class="input-group-text colorpicker-input-addon"><i></i></span>     </span>   </div>  </div>   ');}return p.join('');
},
"vvveb-input-numberinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('  <div>   <input name="',key,'" type="number" value="',value,'"       '); if (typeof min !== 'undefined' && min != false) { p.push('min="',min,'"'); } p.push('       '); if (typeof max !== 'undefined' && max != false) { p.push('max="',max,'"'); } p.push('       '); if (typeof step !== 'undefined' && step != false) { p.push('step="',step,'"'); } p.push('    class="form-control"/>  </div> ');}return p.join('');
},
"vvveb-input-button":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('  <div>   <button class="btn btn-sm btn-icon '); if (typeof className !== 'undefined') { p.push(' ',className,' '); } else { p.push('btn-outline-primary'); } p.push('">    <i class="la  '); if (typeof icon !== 'undefined') { p.push(' ',icon,' '); } else { p.push(' la-plus '); } p.push(' la-lg"></i> ',text,'   </button>  </div>   ');}return p.join('');
},
"vvveb-input-cssunitinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('  <div class="input-group css-unit" id="cssunit-',key,'">   <input name="number" type="number"  '); if (typeof value !== 'undefined' && value != false) { p.push(' value="',value,'" '); } p.push('       '); if (typeof min !== 'undefined' && min != false) { p.push('min="',min,'"'); } p.push('       '); if (typeof max !== 'undefined' && max != false) { p.push('max="',max,'"'); } p.push('       '); if (typeof step !== 'undefined' && step != false) { p.push('step="',step,'"'); } p.push('    class="form-control"/>   <select class="form-select small-arrow" name="unit">    <option value="em">em</option>    <option value="rem">rem</option>    <option value="px">px</option>    <option value="%">%</option>    <option value="vw">vw</option>    <option value="vh">vh</option>    <option value="ex">ex</option>    <option value="ch">ch</option>    <option value="cm">cm</option>    <option value="mm">mm</option>    <option value="in">in</option>    <option value="pt">pt</option>    <option value="auto">auto</option>    <option value="">-</option>   </select>  </div>   ');}return p.join('');
},
"vvveb-filemanager-folder":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('  '); var rand = Math.floor(Math.random() * 10000); p.push('   <li data-folder="',folder,'" class="folder">   <label for="',folder,'',rand,'"><span>',folderTitle,'</span></label> <input type="checkbox" id="',folder,'',rand,'" />   <ol></ol>  </li> ');}return p.join('');
},
"vvveb-filemanager-page":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('  '); var rand = Math.floor(Math.random() * 10000); p.push('   <li data-url="',url,'" data-file="',file,'" data-page="',name,'" class="file'); if (typeof className !== 'undefined') { p.push(' ',className,''); } p.push('" ');if (typeof post_id !== 'undefined') { p.push('data-post_id="',post_id,'"'); } p.push(' ');if (typeof product_id !== 'undefined') { p.push('data-product_id="',product_id,'"'); } p.push(' ');if (typeof global !== 'undefined' && global) { p.push('data-global="true"'); } p.push('>   <label for="',name,'',rand,'" '); if (typeof description !== 'undefined') { p.push(' title="',description,'" '); } p.push('>    <span>',title,'</span>    <!-- <span class="text-muted small">',url,'</span> -->    <div class="file-actions">     <button href="#" class="delete btn btn-outline-danger" title="Delete"><i class="la la-trash"></i></button>     <a href="',url,'" target="_blank" class="view btn btn-outline-primary" title="View page"><i class="la la la-external-link-alt"></i></a>     <button href="#" class="rename btn btn-outline-primary" title="Rename"><i class="la la-pen"></i></button>     <button href="#" class="duplicate btn btn-outline-primary" title="Duplicate"><i class="icon-copy-outline"></i></button>    </div>   </label> <input type="checkbox" id="',name,'',rand,'" />   <!-- <ol></ol> -->  </li> ');}return p.join('');
},
"vvveb-filemanager-component":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('  <li data-url="',url,'" data-component="',name,'" class="component">   <a href="',url,'"><span>',title,'</span></a>  </li> ');}return p.join('');
},
"vvveb-breadcrumb-navigaton-item":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('  <li class="breadcrumb-item"><a href="#" '); if (typeof className !== 'undefined') { p.push('class="',className,'"'); } p.push('>',name,'</a></li> ');}return p.join('');
},
"vvveb-input-sectioninput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('  <div>   '); var namespace = '-' + Math.floor(Math.random() * 1000); p.push('   <label class="header" data-header="',key,'" for="header_',key,'',namespace,'" '); if (typeof group !== 'undefined' && group != null) { p.push('data-group="',group,'" '); } p.push('>    <span>',header,'</span>     <div class="header-arrow"></div>   </label>    <input class="header_check" type="checkbox" '); if (typeof expanded !== 'undefined' && expanded == false) { p.push(' '); } else { p.push('checked="true"'); } p.push(' id="header_',key,'',namespace,'">    <div class="section row" data-section="',key,'" '); if (typeof group !== 'undefined' && group != null) { p.push('data-group="',group,'" '); } p.push('></div>    </div> ');}return p.join('');
},
"vvveb-input-popoverinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('  <div>   '); var namespace = '-' + Math.floor(Math.random() * 1000); p.push('   <label class="header" data-header="',key,'" for="header_',key,'',namespace,'" '); if (typeof group !== 'undefined' && group != null) { p.push('data-group="',group,'" '); } p.push('>    <span>',header,'</span>     <div class="header-arrow"></div>   </label>    <input class="header_check" type="checkbox" '); if (typeof expanded !== 'undefined' && expanded == false) { p.push(' '); } else { p.push('checked="true"'); } p.push(' id="header_',key,'',namespace,'">    <div class="section row" data-section="',key,'" '); if (typeof group !== 'undefined' && group != null) { p.push('data-group="',group,'" '); } p.push('></div>    </div> ');}return p.join('');
},
"vvveb-property":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div class="mb-2 '); if (typeof col !== 'undefined' && col != false) { p.push(' col-sm-',col,' '); } else { p.push('row'); } p.push(' '); if (typeof inline !== 'undefined' && inline == true) { p.push('inline'); } p.push(' " data-key="',key,'" '); if (typeof group !== 'undefined' && group != null) { p.push('data-group="',group,'" '); } p.push('>      '); if (typeof name !== 'undefined' && name != false) { p.push('<label class="'); if (typeof inline === 'undefined' ) { p.push('col-sm-4'); } p.push(' form-label" for="input-model">    <span>',name,'</span>    <!-- <button class="btn-close ms-1" title="Clear"></button> -->   </label>'); } p.push('      <div class="'); if (typeof inline === 'undefined') { p.push('col-sm-'); if (typeof name !== 'undefined' && name != false) { p.push('8'); } else { p.push('12'); } } p.push(' input"></div>     </div>      ');}return p.join('');
},
"vvveb-input-autocompletelist":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>   <input name="',key,'" type="text" class="form-control"/>      <div class="form-control autocomplete-list" style="min-height: 150px; overflow: auto;">   </div>  </div>   ');}return p.join('');
},
"vvveb-input-tagsinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('   <div>   <div class="form-control tags-input p-0" style="height:auto;">           <input name="',key,'" type="text" class="form-control" style="border:none;min-width:60px;"/>   </div>  </div>   ');}return p.join('');
},
"vvveb-input-noticeinput":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('  <div>   <div class="alert alert-dismissible fade show alert-',type,'" role="alert">        <button type="button" class="btn-close" data-bs-dismiss="alert" aria-label="Close"></button>        <strong class="d-block mb-1">',title,'</strong>        ',text,'   </div>  </div> ');}return p.join('');
},
"vvveb-section":function anonymous(obj
) {
let p=[],print=function(){p.push.apply(p,arguments);};with(obj){p.push('  '); var suffix = Math.floor(Math.random() * 10000); p.push('   <div class="section-item" draggable="true">   <div class="controls">    <div class="handle"></div>    <div class="info">     <div class="name">',name,'       <div class="type">',type,'</div>     </div>    </div>    <div class="buttons">     <a class="delete-btn" href="" title="Remove section"><i class="la la-trash text-danger"></i></a>     <!--      <a class="up-btn" href="" title="Move element up"><i class="la la-arrow-up"></i></a>     <a class="down-btn" href="" title="Move element down"><i class="la la-arrow-down"></i></a>     -->     <a class="properties-btn" href="" title="Properties"><i class="icon-settings-outline"></i></a>   </div>   </div>     <input class="header_check" type="checkbox" id="section-components-',suffix,'">    <label for="section-components-',suffix,'">     <div class="header-arrow"></div>   </label>      <div class="tree">    <ol>     <!--     <li data-component="Products" class="file">             <label for="idNaN" style="background-image:url(/js/vvvebjs/icons/products.svg)"><span>Products</span></label>             <input type="checkbox" id="idNaN">     </li>     <li data-component="Posts" class="file">             <label for="idNaN" style="background-image:url(/js/vvvebjs/icons/posts.svg)"><span>Posts</span></label>             <input type="checkbox" id="idNaN">     </li>     -->    </ol>   </div>  </div>   ');}return p.join('');
}
};
window.tmpl=(id,data)=>{const fn=studioTemplates[id];if(!fn)throw new Error("Unbundled Vvveb template: "+id);return data ? fn(data) : fn;};
