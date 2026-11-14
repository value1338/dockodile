'use strict';
// Hero demo: a small desktop that behaves like Dockodile. Tabs reorder sideways, pull out
// into their own window, windows dock on the bar, the plus adds programs.
(function(){
  var S='<svg viewBox="0 0 12 12">',E='</svg>';
  var chromeTabs=[
    {name:'Dockodile',url:'dockodile.de',html:'<h3>Tabs für alle Programme</h3><p>Chrome behält seine eigenen Tabs. Dockodile legt nur eine Leiste darüber.</p>'},
    {name:'Dokumentation',url:'dockodile.de/anleitung',html:'<h3>Ein Platz für deine Programme</h3><p>Fenster zusammenziehen, als Paket sichern, per Kürzel öffnen.</p>'},
    {name:'GitHub',url:'github.com/value1338/dockodile',html:'<h3>value1338/dockodile</h3><p>Quelltext, Fehlerberichte und Wünsche.</p>'}
  ];
  var apps={
    mail:{name:'Mail',color:'#2F7FE6',badge:'3',icon:'<rect x="1.5" y="2.5" width="9" height="7" rx="1.5"/><path d="M2 3.5l4 3 4-3"/>',
      view:'<h4>Posteingang</h4><div class="row"><b>Lena Brandt</b><span>Angebot für Donnerstag, kurze Rückfrage</span></div><div class="row"><b>Bauamt</b><span>Ihre Unterlagen sind eingegangen</span></div><div class="row"><b>Jonas</b><span>Fotos vom Wochenende</span></div>'},
    cal:{name:'Kalender',color:'#E2574C',icon:'<rect x="1.5" y="2.5" width="9" height="8" rx="1.5"/><path d="M1.5 5h9M4 1.5v2M8 1.5v2"/>',
      view:'<h4>Diese Woche</h4><div class="days"><div class="day">Mo<div class="ev" style="background:#2F7FE6">Jour fixe</div></div><div class="day">Di</div><div class="day">Mi<div class="ev" style="background:#E2574C">Zahnarzt</div></div><div class="day">Do<div class="ev" style="background:#2FA860">Angebot</div></div><div class="day">Fr</div></div>'},
    chrome:{name:'Chrome',color:'#F2A60C',icon:'<circle cx="6" cy="6" r="4.3"/><circle cx="6" cy="6" r="1.6"/>',browser:true},
    chat:{name:'Chat',color:'#2FA860',icon:'<path d="M2 3.5a1.5 1.5 0 011.5-1.5h5A1.5 1.5 0 0110 3.5v3A1.5 1.5 0 018.5 8H6l-2.5 2V8A1.5 1.5 0 012 6.5z"/>',
      view:'<h4>Team</h4><div class="bub">Hast du die neuen Tabs gesehen?</div><div class="bub me">Ja. Endlich alles an einem Platz.</div>'},
    notes:{name:'Notizen',color:'#D9A514',icon:'<rect x="2" y="1.5" width="8" height="9" rx="1.5"/><path d="M4 4.5h4M4 6.5h4M4 8.5h2"/>',
      view:'<h4>Einkauf</h4><div class="row"><b>Samstag</b><span>Brot, Kaffee, Tomaten</span></div><div class="row"><b>Idee</b><span>Paket „Büro“ auf ⌃⌥1 legen</span></div>'},
    music:{name:'Musik',color:'#E0355F',icon:'<path d="M4.5 9V3l5-1v6"/><circle cx="3.5" cy="9" r="1.2"/><circle cx="8.5" cy="8" r="1.2"/>',
      view:'<h4>Jetzt läuft</h4><div class="row"><b>Swamp Jazz</b><span>The Crocodile Trio</span></div><div class="row"><b>Als Nächstes</b><span>Tab Dance</span></div>'},
    maps:{name:'Karten',color:'#3C9A8F',icon:'<path d="M1.5 3l3-1 3 1 3-1v7l-3 1-3-1-3 1z"/><path d="M4.5 2v7M7.5 3v7"/>',
      view:'<h4>Bodensee</h4><div class="row"><b>Kressbronn</b><span>12 Minuten bis zum Hafen</span></div>'}
  };
  var START={tabs:['mail','cal','chrome'],loose:['chat']};
  var stage=document.getElementById('stage'),tabsEl=document.getElementById('tabs'),pane=document.getElementById('pane'),hint=document.getElementById('try');
  if(!stage)return;
  var bar=stage.querySelector('.bar'),tabs,loose,active,browserTab=0,wins={},drag=null,z=5,menu,search;

  function ico(id){return '<span class="ico" style="background:'+apps[id].color+'" aria-hidden="true">'+S+apps[id].icon+E+'</span>';}
  function el(tag,cls,text){var n=document.createElement(tag);if(cls)n.className=cls;if(text)n.textContent=text;return n;}
  function content(id){
    if(!apps[id].browser)return '<div class="view">'+apps[id].view+'</div>';
    var t=chromeTabs[browserTab];
    return '<div class="browser-tabs" role="tablist" aria-label="Tabs in Chrome">'+chromeTabs.map(function(c,i){return '<button type="button" role="tab" data-browser="'+i+'" aria-selected="'+(i===browserTab)+'">'+c.name+'</button>';}).join('')
      +'</div><div class="browser-address">'+t.url+'</div><div class="browser-content">'+t.html+'</div>';
  }
  function render(){
    tabsEl.replaceChildren();
    tabs.forEach(function(id,i){
      var shell=el('div','tab-shell'+(i===active?' on':''));shell.dataset.app=id;
      var b=el('button','tab');b.type='button';b.setAttribute('role','tab');b.setAttribute('aria-selected',i===active);
      b.innerHTML=ico(id)+'<span class="t">'+apps[id].name+'</span>'+(apps[id].badge?'<span class="n" aria-label="'+apps[id].badge+' ungelesen">'+apps[id].badge+'</span>':'');
      b.addEventListener('click',function(){if(tabs.indexOf(id)<0)return;active=tabs.indexOf(id);render();});
      b.addEventListener('pointerdown',function(e){if(e.button)return;drag={kind:'tab',id:id,shell:shell,x:e.clientX,y:e.clientY,off:e.clientX-shell.getBoundingClientRect().left,moved:false};});
      var x=el('button','tab-detach','✕');x.type='button';x.setAttribute('aria-label',apps[id].name+' aus Gruppe lösen');
      x.addEventListener('click',function(){detach(id);});
      shell.append(b,x);tabsEl.append(shell);
    });
    pane.innerHTML=tabs.length?content(tabs[active]):'<p class="empty-group">Leere Gruppe. Zieh ein Fenster auf die Leiste oder nimm das Plus.</p>';
    Object.keys(apps).forEach(function(id){
      var w=wins[id],show=loose.indexOf(id)>=0;
      if(show)w.querySelector('.body').innerHTML=content(id);
      w.hidden=!show;
    });
    hint.textContent=loose.length?'Zieh ein Fenster auf die Tab-Leiste. Tabs lassen sich umsortieren und herausziehen.':'Alles in einem Fenster. Zieh einen Tab nach unten, um ihn zu lösen.';
    if(menu&&!menu.hidden)fillMenu();
  }
  function place(id,x,y){
    var w=wins[id],s=stage.getBoundingClientRect();w.hidden=false;
    var r=w.getBoundingClientRect();
    w.style.left=Math.max(-r.width*.4,Math.min(x,s.width-r.width*.6))+'px';
    w.style.top=Math.max(0,Math.min(y,s.height-34))+'px';
    w.style.zIndex=++z;
  }
  function dock(id,at){
    if(tabs.indexOf(id)>=0)return;
    loose=loose.filter(function(l){return l!==id;});
    if(at==null)at=tabs.length;tabs.splice(at,0,id);active=at;
    wins[id].style.left=wins[id].style.top='';stage.classList.remove('hot');render();
  }
  function detach(id,x,y){
    var i=tabs.indexOf(id);if(i<0)return;
    tabs.splice(i,1);active=Math.max(0,Math.min(active>i?active-1:active,tabs.length-1));
    loose.push(id);render();
    var s=stage.getBoundingClientRect(),n=loose.length-1;
    if(x==null){x=s.width*.2+n*26;y=s.height*.4+n*40;}
    place(id,x,y);
  }
  function cascade(){
    // Below the bar, so the tabs stay reachable.
    var s=stage.getBoundingClientRect(),top=bar.getBoundingClientRect().bottom-s.top+10;
    loose.forEach(function(id,i){
      wins[id].hidden=false;
      var w=wins[id].getBoundingClientRect(),step=Math.min(44,(s.width-w.width-24)/Math.max(loose.length-1,1));
      place(id,12+i*step,top+i*40);
    });
  }
  function reset(){
    tabs=START.tabs.slice();loose=START.loose.slice();active=0;browserTab=0;
    Object.keys(wins).forEach(function(id){wins[id].style.left=wins[id].style.top='';});
    closeMenu();render();
  }
  function overBar(x,y,slack){var r=bar.getBoundingClientRect();slack=slack||0;return x>r.left-10&&x<r.right+10&&y>r.top-16-slack&&y<r.bottom+20+slack;}
  // Position among the tabs. A dragged tab (skip) trades places once it covers half of a
  // neighbour, like browser tabs; a window being dropped goes by the pointer alone.
  function slot(x,skip,left,right){
    var n=0,before=true;
    for(var i=0;i<tabsEl.children.length;i++){
      var c=tabsEl.children[i];if(c===skip){before=false;continue;}
      var r=c.getBoundingClientRect(),mid=r.left+r.width/2;
      if(skip?(before?left>mid:right>mid):x>mid)n++;
    }
    return n;
  }

  // Plus menu: open windows first, installed programs once two letters are typed. As in the app.
  function fillMenu(){
    var q=search.value.trim().toLowerCase(),list=menu.querySelector('.app-list');list.replaceChildren();
    var ids=Object.keys(apps).filter(function(id){
      if(tabs.indexOf(id)>=0)return false;
      return q.length>=2?apps[id].name.toLowerCase().indexOf(q)>=0:loose.indexOf(id)>=0;
    });
    ids.forEach(function(id){
      var b=el('button');b.type='button';b.innerHTML=ico(id)+' '+apps[id].name;b.setAttribute('aria-label',apps[id].name+' hinzufügen');
      b.addEventListener('click',function(){dock(id);closeMenu();});list.append(b);
    });
    if(!ids.length)list.append(el('p','empty',q.length>=2?'Nichts gefunden.':'Kein offenes Fenster. Tippe zwei Buchstaben, um Programme zu suchen.'));
  }
  function closeMenu(){if(menu)menu.hidden=true;}
  var plus=el('button','app-plus','+');plus.type='button';plus.setAttribute('aria-label','Programme hinzufügen');
  menu=el('div','app-menu');menu.hidden=true;
  menu.innerHTML='<b>Programme</b><input type="search" aria-label="Programme durchsuchen" placeholder="Suchen …"><div class="app-list"></div>';
  search=menu.querySelector('input');search.addEventListener('input',fillMenu);
  plus.addEventListener('click',function(){menu.hidden=!menu.hidden;if(!menu.hidden){search.value='';fillMenu();search.focus();}});
  bar.insertBefore(plus,bar.querySelector('.drop'));bar.append(menu);
  document.addEventListener('keydown',function(e){if(e.key==='Escape')closeMenu();});
  document.addEventListener('pointerdown',function(e){if(!menu.hidden&&!menu.contains(e.target)&&e.target!==plus)closeMenu();});

  Object.keys(apps).forEach(function(id){
    var w=el('div','win');w.hidden=true;w.dataset.app=id;
    w.innerHTML='<div class="title"><span class="dots" aria-hidden="true"><i></i><i></i><i></i></span>'+ico(id)+apps[id].name
      +'<button class="dock" type="button" aria-label="'+apps[id].name+' andocken">Andocken</button></div><div class="body view-host"></div>';
    w.querySelector('.dock').addEventListener('click',function(){dock(id);});
    w.addEventListener('pointerdown',function(){w.style.zIndex=++z;});
    w.querySelector('.title').addEventListener('pointerdown',function(e){
      if(e.button||e.target.closest('.dock'))return;
      var r=w.getBoundingClientRect();drag={kind:'win',id:id,dx:e.clientX-r.left,dy:e.clientY-r.top};w.classList.add('drag');e.preventDefault();
    });
    stage.append(w);wins[id]=w;
  });
  // Chrome's own tabs, in the group pane and in its loose window.
  stage.addEventListener('click',function(e){var b=e.target.closest('[data-browser]');if(b){browserTab=+b.dataset.browser;render();}});

  // One drag for everything, on the window: a tab reorders inside the bar and turns into a
  // window once it leaves; that window keeps following the pointer and docks back on the bar.
  window.addEventListener('pointermove',function(e){
    if(!drag)return;
    var s=stage.getBoundingClientRect();
    if(drag.kind==='tab'){
      if(!drag.moved&&Math.abs(e.clientX-drag.x)+Math.abs(e.clientY-drag.y)<6)return;
      drag.moved=true;
      // Sideways the tab follows the pointer and swaps places. A clear pull up or down takes it out.
      if(overBar(e.clientX,e.clientY,26)){
        var sh=drag.shell,from=tabs.indexOf(drag.id),left=e.clientX-drag.off,to=slot(e.clientX,sh,left,left+sh.offsetWidth);
        if(to!==from){
          var sel=tabs[active];tabs.splice(from,1);tabs.splice(to,0,drag.id);active=tabs.indexOf(sel);
          var others=Array.prototype.filter.call(tabsEl.children,function(c){return c!==sh;});
          tabsEl.insertBefore(sh,others[to]||null);
        }
        sh.classList.add('moving');sh.style.transform='';
        sh.style.transform='translateX('+(e.clientX-drag.off-sh.getBoundingClientRect().left)+'px)';
        return;
      }
      var id=drag.id;detach(id,0,0);
      var r=wins[id].getBoundingClientRect();drag={kind:'win',id:id,dx:r.width/2,dy:17};wins[id].classList.add('drag');
    }
    place(drag.id,e.clientX-s.left-drag.dx,e.clientY-s.top-drag.dy);
    stage.classList.toggle('hot',overBar(e.clientX,e.clientY));
  });
  function end(e){
    if(!drag)return;var d=drag;drag=null;
    if(d.kind!=='win'){if(d.shell){d.shell.style.transform='';d.shell.classList.remove('moving');}return;}
    wins[d.id].classList.remove('drag');
    if(stage.classList.contains('hot'))dock(d.id,slot(e.clientX));
    stage.classList.remove('hot');
  }
  window.addEventListener('pointerup',end);window.addEventListener('pointercancel',end);
  window.addEventListener('resize',function(){Object.keys(wins).forEach(function(id){wins[id].style.left=wins[id].style.top='';});});

  var tools=el('div','demo-tools'),all=el('button',null,'Alle Fenster lösen'),again=el('button',null,'Demo zurücksetzen');
  all.type=again.type='button';
  all.addEventListener('click',function(){loose=loose.concat(tabs);tabs=[];active=0;render();cascade();});
  again.addEventListener('click',reset);
  tools.append(all,again);stage.parentNode.append(tools);
  reset();
})();
