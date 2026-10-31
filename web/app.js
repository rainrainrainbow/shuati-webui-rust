// 国考刷题 — 重构版（参考预览版设计，数据走真实后端）
// 顶栏+仪表盘+题目卡+PC热力图答题卡+模考组卷类型（国考专业/行测/申论、省考行测/申论）
const $=s=>document.querySelector(s);
const $$=s=>document.querySelectorAll(s);
const LS_TOKEN='shuati_token', LS_USER='shuati_user';
const LS_WRONG='shuati_wrong', LS_SRS='shuati_srs', LS_FAV='shuati_fav';
const MODES=[['normal','常规'],['weak','先攻弱项'],['wrong','错题重练'],['srs','到期复习'],['shuffle','打乱顺序'],['fav','⭐ 收藏']];
const SRS_DAYS=[0,1,3,7,15,30];
const TOAST_MS=1800;
const STATUSES=[['undone','未刷'],['done','已刷'],['all','全部']];
// 模考卷类型
const PAPERS=[
  {type:'guokao', paper:'zhuanye', label:'国考·专业卷',  count:100, min:120, emoji:'💼'},
  {type:'guokao', paper:'xingce',  label:'国考·行测卷',  count:135, min:120, emoji:'🎯'},
  {type:'guokao', paper:'shenlun', label:'国考·申论卷',  count:5,   min:180, emoji:'✍️'},
  {type:'shengkao',paper:'xingce', label:'省考·行测卷',  count:100, min:100, emoji:'🌏'},
  {type:'shengkao',paper:'shenlun',label:'省考·申论卷',  count:4,   min:150, emoji:'📝'},
];
const PAPER_NAME=(type,paper)=>{
  const p=PAPERS.find(x=>x.type===type&&x.paper===paper);
  if(p) return p.label;
  const t=type==='shengkao'?'省考':'国考';
  const pp=paper==='xingce'?'行测':paper==='shenlun'?'申论':paper==='zhuanye'?'专业':'行测';
  return `${t}·${pp}卷`;
};

let TOKEN=localStorage.getItem(LS_TOKEN)||'', USERNAME=localStorage.getItem(LS_USER)||'';
let SUBJECTS=[], CUR='', QS=[], RAW_QS=[], IDX=0, ANSWERS={}, RESULTS={}, MODE='normal';
let CAT='全部', SEARCH='', PCMODE=false, VIEW='dash'; // dash | quiz
let QSTATUS='undone'; // 未刷/已刷/全部，默认未刷
let MOCK=null, MOCK_ACTIVE=false, MOCK_TIMER=null, MOCK_DEADLINE=0;
let TARGETED=false, TARGET_NOTE='', FILTER={module:'',difficulty:0};
let EXAMS={guokao:'',shengkao:''};
const GRADED=true;

/* ---------- 工具 ---------- */
const esc=s=>String(s==null?'':s).replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const loadJ=k=>{try{return JSON.parse(localStorage.getItem(k))||{};}catch(e){return {};}};
const saveJ=(k,v)=>{try{localStorage.setItem(k,JSON.stringify(v));}catch(e){}};
const wrongBank=()=>loadJ(LS_WRONG), srs=()=>loadJ(LS_SRS), favs=()=>loadJ(LS_FAV);
const isMock=()=>MOCK_ACTIVE;
const isEssay=()=>QS.length&&QS[0].type==='essay';
let toastTimer;
function toast(msg){const t=$('#toast');t.textContent=msg;t.classList.add('show');clearTimeout(toastTimer);toastTimer=setTimeout(()=>t.classList.remove('show'),TOAST_MS);}
function fmtDate(ts){const d=new Date(ts);return `${d.getMonth()+1}月${d.getDate()}日`;}
function todayStr(){const d=new Date();return d.getFullYear()+'-'+String(d.getMonth()+1).padStart(2,'0')+'-'+String(d.getDate()).padStart(2,'0');}
function shuffleArr(a){const x=a.slice();for(let i=x.length-1;i>0;i--){const j=Math.floor(Math.random()*(i+1));[x[i],x[j]]=[x[j],x[i]];}return x;}

// ---- 认证 ---- 
function api(path,opts){
  opts=opts||{}; opts.headers=Object.assign({},opts.headers||{});
  if(TOKEN) opts.headers['Authorization']='Bearer '+TOKEN;
  return fetch(path,opts);
}
function requireLogin(){ $('#loginmask').classList.remove('hide'); }
function onAuthed(name){
  USERNAME=name; localStorage.setItem(LS_USER,name);
  $('#menu-name').textContent=name;
  const init=(USERNAME[0]||'汉').toUpperCase(); $('#avatar').textContent=init;
  $('#loginmask').classList.add('hide');
  initPC(); loadSubjects(); loadExamDeadline(); updateBell();
}
function logout(){
  if(TOKEN){ fetch('/api/logout',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({token:TOKEN})}).catch(()=>{}); }
  TOKEN=''; USERNAME=''; localStorage.removeItem(LS_TOKEN); localStorage.removeItem(LS_USER);
  $('#subname').textContent=''; $('#menu-name').textContent='';
  requireLogin();
}
function toggleUserMenu(){ $('#user-menu').classList.toggle('hide'); }
document.addEventListener('click',e=>{const m=$('#user-menu');const a=$('#avatar');if(m&&!m.classList.contains('hide')&&!m.contains(e.target)&&e.target!==a)m.classList.add('hide');});
async function doLogin(){
  const u=$('#loginuser').value.trim(), p=$('#loginpass').value;
  $('#loginerr').textContent='';
  if(!u||!p){ $('#loginerr').textContent='请输入用户名和密码'; return; }
  try{
    const r=await fetch('/api/login',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({username:u,password:p})});
    const d=await r.json();
    if(d.ok){ TOKEN=d.token; localStorage.setItem(LS_TOKEN,d.token); onAuthed(d.name||d.username); }
    else $('#loginerr').textContent=d.err||'登录失败';
  }catch(e){ $('#loginerr').textContent='网络错误'; }
}
async function doRegister(){
  const u=$('#loginuser').value.trim(), p=$('#loginpass').value;
  $('#loginerr').textContent='';
  if(!u||!p){ $('#loginerr').textContent='请输入用户名和密码'; return; }
  try{
    const r=await fetch('/api/register',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({username:u,password:p})});
    const d=await r.json();
    if(d.ok){ TOKEN=d.token; localStorage.setItem(LS_TOKEN,d.token); onAuthed(d.name||d.username); toast('✅ 注册成功，欢迎 '+d.name); }
    else $('#loginerr').textContent=d.err||'注册失败';
  }catch(e){ $('#loginerr').textContent='网络错误'; }
}
async function bootAuth(){
  if(!TOKEN){ requireLogin(); return; }
  try{
    const r=await api('/api/me');
    if(r.status===200){ const d=await r.json(); onAuthed(d.name||d.username); }
    else { TOKEN=''; localStorage.removeItem(LS_TOKEN); requireLogin(); }
  }catch(e){ requireLogin(); }
}

/* ---------- 错题本 + SRS + 收藏 ---------- */
function markGrade(qid,correct){
  const wb=wrongBank();
  if(correct){ delete wb[qid]; }
  else {
    const q=QS.find(x=>x.id===qid);
    wb[qid]={stem:q?q.stem.slice(0,60):qid,module:q?q.module:'',subject:CUR,n:(wb[qid]&&wb[qid].n||0)+1,at:Date.now()};
  }
  saveJ(LS_WRONG,wb);
  const s=srs(), rec=s[qid]||{box:0,next:0};
  if(correct){ rec.box=Math.min(rec.box+1,5); rec.next=Date.now()+SRS_DAYS[rec.box]*86400000; }
  else { rec.box=0; rec.next=Date.now(); }
  s[qid]=rec; saveJ(LS_SRS,s); updateBell();
}
function isDue(qid){ const r=srs()[qid]; return !r||r.box===0||r.next<=Date.now(); }
function toggleFav(qid){
  const f=favs(), q=QS.find(x=>x.id===qid);
  if(f[qid]){ delete f[qid]; toast('已取消收藏'); }
  else { f[qid]={stem:q?q.stem.slice(0,60):qid,module:q?q.module:'',at:Date.now()}; toast('⭐ 已收藏'); }
  saveJ(LS_FAV,f); render();
}
function openReport(qid){
  const q=QS.find(x=>x.id===qid);
  const reasons=['答案解析有误','答案有误','题目不完整/缺项','选项有误','题干有误','其他'];
  $('#repbody').innerHTML=`
    <div class="sheethead">纠错反馈 <button class="closebtn" onclick="closeReport()">✕</button></div>
    <div class="rep-stem">${esc((q&&q.stem||'').slice(0,130))}</div>
    <div class="rep-opts">${reasons.map((r,i)=>`<label class="rep-opt" style="${i===0?'border-color:var(--blue);background:#eff6ff;color:var(--blue)':''}"><input type="radio" name="rep" value="${r}" ${i===0?'checked':''}> ${r}</label>`).join('')}</div>
    <textarea id="repnote" class="rep-note" placeholder="补充说明（可选）"></textarea>
    <button class="btn" onclick="submitReport('${qid}')">提交反馈</button>`;
  $('#repmask').classList.add('open');
}
function closeReport(){ $('#repmask').classList.remove('open'); }
async function submitReport(qid){
  const reason=(document.querySelector('input[name="rep"]:checked')||{}).value||'其他';
  const note=$('#repnote').value||'';
  closeReport();
  try{
    const r=await api('/api/report',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({qid,reason,note})});
    const d=await r.json();
    toast(d.ok?'✅ 已提交反馈，感谢！':'提交失败');
  }catch(e){ toast('提交失败，请重试'); }
}

/* ---------- 科目 / 分类 ---------- */
function catOf(s){
  if(s.category) return s.category;
  const id=s.id||'';
  if(id==='jisuanji') return '专业课';
  if(/^zx/.test(id)) return '专项';
  if(/^zt/.test(id)) return '金管局真题';
  if(/^(xingce|yanyu|panduan|ziliao|changshi|shuliang|zhengzhi)$/.test(id)) return '国考行测';
  if(/^slzt/.test(id)) return '申论真题';
  if(/^sl/.test(id)||id==='shenlun') return '国考申论';
  if(/^gd/.test(id)) return '省考';
  return '其他';
}
function cats(){ return ['全部',...new Set(SUBJECTS.map(s=>catOf(s)))]; }
async function loadSubjects(){
  try{
    const d=await (await api('/api/subjects')).json();
    SUBJECTS=d.subjects||[];
  }catch(e){ toast('加载科目失败，请刷新重试'); }
  showDashboard();
}
function SUBJCOUNT(c){ return SUBJECTS.filter(s=>catOf(s)===c).length; }

/* ---------- 仪表盘 ---------- */
function dueCount(){ const s=srs(); return Object.values(s).filter(r=>r.box>0&&r.next<=Date.now()).length; }
function overdueCount(){ const s=srs(); return Object.values(s).filter(r=>r.box>0&&r.next<Date.now()-86400000).length; }
function wrongCount(){ return Object.keys(wrongBank()).length; }
function updateBell(){
  const n=dueCount(); const b=$('#bell-count'); if(!b) return;
  if(n>0){ b.textContent=n; b.classList.remove('hide'); } else b.classList.add('hide');
}
async function statSummary(){
  let total=0, correct=0, pct=0;
  try{ const d=await (await api('/api/stats')).json(); total=d.total||0; correct=d.correct||0; pct=d.overall||0; }catch(e){}
  return {total, correct, pct};
}
function showDashboard(){
  VIEW='dash'; $('#quiz').style.display='none'; $('#dashboard').style.display='block'; $('#foot').innerHTML='';
  const lastSub=localStorage.getItem('shuati_last_subject')||(SUBJECTS[0]?SUBJECTS[0].id:'');
  const lastIdx=parseInt(localStorage.getItem('shuati_last_index')||'0');
  const subj=SUBJECTS.find(s=>s.id===lastSub)||SUBJECTS[0];
  const wb=wrongBank(); const due=dueCount(); const over=overdueCount();
  $('#subname').textContent=USERNAME?(' · '+USERNAME+'　🏠 首页'):' · 首页';
  statSummary().then(g=>{
    $('#dashboard').innerHTML=`
    <div class="wrap">
      <div class="dash-row">
        <div class="dash-card dash-continue">
          <div class="dash-title">▶ 继续上次</div>
          <div style="font-size:16px;font-weight:800;margin:5px 0 2px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(subj?subj.name:'—')}</div>
          <div class="dash-sub">${subj?`第 ${Math.min(lastIdx+1,subj.count)} 题 · 共 ${subj.count} 题`:''}　|　已答对 ${g.correct} 题</div>
          <button class="btn" onclick="resumeLast()">继续刷题 →</button>
        </div>
        <div class="dash-card dash-progress">
          <div class="dash-title">累计正确率</div>
          <div class="dash-big">${g.pct}%</div>
          <div class="dash-bar"><i style="width:${Math.min(g.pct,100)}%"></i></div>
          <div class="dash-sub">${g.correct} / ${g.total} 题已批</div>
        </div>
      </div>
      <div class="dash-section">今日推荐 <span class="more" onclick="showReportView()">学习报告 →</span></div>
      <div class="dash-cards">
        <button class="dash-rec-card ${over?'hot':''}" onclick="quickSrs()">🔁 到期复习 <span>${due} 题${over?` · 逾期 ${over} 题`:' · 状态良好'}</span></button>
        <button class="dash-rec-card" onclick="quickMode('wrong')">📕 错题重练 <span>${wrongCount()} 道经典错题</span></button>
        <button class="dash-rec-card" onclick="quickShuffle()">🔀 随机一套 <span>40 题 · 限时冲刺</span></button>
        <button class="dash-rec-card" onclick="startMock()">📝 模拟考试 <span>按真考时长计时</span></button>
        <button class="dash-rec-card" onclick="startTargeted()">🎯 针对性刷题 <span>薄弱模块 · 精准提升</span></button>
      </div>
      <div class="dash-section">我的科目 <span class="more" onclick="$('#subjects-block').classList.toggle('hide')">展开全部 →</span></div>
      <div id="subjects-block">
        <div class="catbar" id="catbar"></div>
        <div id="subjgrid" style="display:grid;grid-template-columns:repeat(auto-fill,minmax(150px,1fr));gap:9px;padding:8px 0 4px"></div>
      </div>
      <div class="dash-section">刷题模式 <span class="more">点题目即进入</span></div>
      <div class="dash-cards">
        ${MODES.map(([id,label],i)=>`<button class="dash-rec-card" style="min-width:110px" onclick="quickMode('${id}')">${['🎯','🧩','📕','🔁','🔀','⭐'][i]} ${label}<span>智能排序</span></button>`).join('')}
      </div>
    </div>`;
    renderCatbar(); renderSubjGrid();
  });
}
function renderCatbar(){
  const cb=$('#catbar'); cb.innerHTML='';
  cats().forEach(c=>{
    const b=document.createElement('button');
    b.className='catbtn'+(CAT===c?' active':'');
    b.textContent=c==='全部'?'全部':`${c} ${SUBJCOUNT(c)}`;
    b.onclick=()=>{ CAT=c; renderCatbar(); renderSubjGrid(); };
    cb.appendChild(b);
  });
}
function renderSubjGrid(){
  const box=$('#subjgrid'); box.innerHTML='';
  const arr=(CAT==='全部'?SUBJECTS:SUBJECTS.filter(s=>catOf(s)===CAT));
  if(!arr.length){ box.innerHTML='<p class="empty-hint">该分类暂无科目</p>'; return; }
  const last=localStorage.getItem('shuati_last_subject');
  arr.forEach(s=>{
    const b=document.createElement('button');
    const mine=Object.values(wrongBank()).filter(w=>w.subject===s.id).length;
    b.className='dash-subj-btn'+(s.id===last?' cur':'');
    b.innerHTML=`<span class="dash-subj-name">${esc(s.name)}</span><span class="dash-subj-count">${s.count} 题${mine?` · ${mine} 错题`:''}</span>`;
    b.onclick=()=>{ localStorage.setItem('shuati_last_subject',s.id); CUR=s.id; MODE='normal'; loadQuiz(); };
    box.appendChild(b);
  });
}
function goCat(c){ CAT=c; showDashboard(); }
function goDashboard(){ stopMockTimer(); MOCK_ACTIVE=false; MOCK=null; VIEW='dash'; $('#quiz').style.display='none'; $('#dashboard').style.display='block'; $('#foot').innerHTML=''; showDashboard(); }

/* ---------- 进入题目 ---------- */
function resumeLast(){
  const last=localStorage.getItem('shuati_last_subject')||(SUBJECTS[0]?SUBJECTS[0].id:'');
  CUR=SUBJECTS.some(s=>s.id===last)?last:(SUBJECTS[0]?SUBJECTS[0].id:'');
  MODE='normal';
  loadQuiz(()=>{ IDX=Math.min(parseInt(localStorage.getItem('shuati_last_index')||'0'),QS.length-1); render(); });
}
function quickMode(m){
  MODE=m;
  if(m==='srs'){ const d=dueCount(); if(!d){ toast('🎉 今天没有到期题，去刷套卷吧'); return; } }
  const last=localStorage.getItem('shuati_last_subject')||(SUBJECTS[0]?SUBJECTS[0].id:'');
  CUR=SUBJECTS.some(s=>s.id===last)?last:(SUBJECTS[0]?SUBJECTS[0].id:'');
  loadQuiz();
}
function quickSrs(){
  const d=dueCount(); if(!d){ toast('🎉 今天没有到期题，去刷套卷吧'); return; }
  MODE='srs'; const last=localStorage.getItem('shuati_last_subject')||(SUBJECTS[0]?SUBJECTS[0].id:'');
  CUR=SUBJECTS.some(s=>s.id===last)?last:(SUBJECTS[0]?SUBJECTS[0].id:''); loadQuiz();
}
function quickShuffle(){
  MODE='shuffle';
  const zx=SUBJECTS.find(s=>catOf(s)==='专项');
  CUR=zx?zx.id:(SUBJECTS[0]?SUBJECTS[0].id:'');
  loadQuiz(function(){ QS=QS.slice(0,40); render(); });
}

/* ---------- 加载题目（真实后端） ---------- */
async function loadQuiz(post){
  MOCK_ACTIVE=false; MOCK=null; TARGETED=false; TARGET_NOTE=''; stopMockTimer();
  if(!SUBJECTS.some(s=>s.id===CUR)) CUR=SUBJECTS[0]?SUBJECTS[0].id:'';
  ANSWERS={}; RESULTS={}; IDX=0;
  let d;
  try{ d=await (await api('/api/questions?subject='+encodeURIComponent(CUR)+'&status='+QSTATUS)).json(); }
  catch(e){ toast('加载题目失败'); return; }
  RAW_QS=await applyMode(d.questions||[]);
  QS=applyFilter(RAW_QS);
  if(post){ post(); }
  enterQuiz();
}
function applyFilter(qs){
  if(FILTER.module) qs=qs.filter(q=>q.module===FILTER.module);
  if(FILTER.difficulty) qs=qs.filter(q=>q.difficulty>=FILTER.difficulty);
  return qs;
}
async function applyMode(qs){
  const obj=qs.length&&qs[0].type!=='essay';
  if(MODE==='shuffle') return shuffleArr(qs);
  if(MODE==='weak'&&obj){
    let st={};
    try{ const d=await (await api('/api/stats')).json(); (d.modules||[]).forEach(m=>{ if(m.subject===CUR) st[m.module]=m.pct; }); }catch(e){}
    const a=qs.filter(q=>st[q.module]!==undefined&&st[q.module]<60), b=qs.filter(q=>!(st[q.module]!==undefined&&st[q.module]<60));
    if(a.length) toast('先攻弱项：优先 '+a.length+' 道');
    return a.concat(b);
  }
  if(MODE==='wrong'){ const wb=wrongBank(); const a=qs.filter(q=>wb[q.id]); if(!a.length) toast('🎯 暂无错题 —— 继续保持！'); return a; }
  if(MODE==='fav'){ const f=favs(); const a=qs.filter(q=>f[q.id]); if(!a.length) toast('还没有收藏，答题时点 ☆'); return a; }
  if(MODE==='srs'&&obj){ const a=qs.filter(q=>isDue(q.id)); if(!a.length) toast('🎉 无到期题，先做常规积累'); return a; }
  return qs;
}
function enterQuiz(){
  VIEW='quiz'; $('#dashboard').style.display='none'; $('#quiz').style.display='block';
  const subj=SUBJECTS.find(s=>s.id===CUR);
  let label;
  if(TARGETED){ label='🎯 针对性刷题 · '+QS.length+'题'; }
  else{
    label=(subj?subj.name:'')+' · '+QS.length+' 题';
    const ml=MODES.find(m=>m[0]===MODE)?.[1]; if(ml) label+=' · '+ml;
  }
  $('#subname').textContent=(USERNAME?' · '+USERNAME:'')+' · '+label;
  if(!QS.length){ renderEmpty(); return; }
  render();
}
function renderEmpty(){
  const map={wrong:'🎯 暂无错题 —— 继续保持！',srs:'🔁 暂无到期题，先做「常规」积累',fav:'⭐ 还没有收藏题目，答题时点 ☆ 试试',default:'该科目暂无题目'};
  $('#quiz').innerHTML=`<div class="qcard"><p class="hint"><span class="bigemo">📭</span>${map[MODE]||map.default}</p></div>`;
  $('#foot').innerHTML=`<button onclick="goDashboard()">← 返回首页</button>`;
  renderPCside();
}
function backToSubject(){ MODE='normal'; loadQuiz(); }

/* ---------- 状态切换（未刷/已刷/全部） ---------- */
function renderStatusbar(){
  if(MOCK_ACTIVE||TARGETED) return '';
  return `<div class="quizstatus">${STATUSES.map(([id,label])=>`<button class="${QSTATUS===id?'active':''}" onclick="setStatus('${id}')">${label}</button>`).join('')}</div>`;
}
function setStatus(s){
  if(QSTATUS===s) return;
  QSTATUS=s;
  if(VIEW==='quiz') loadQuiz();
  else showDashboard();
}

/* ---------- 渲染题目 ---------- */
function diffBadge(d){
  if(!d) return '';
  const c=d>=5?'#e5484d':d>=4?'#f76b15':'#3b82f6';
  return `<span class="diffbadge" style="background:${c}1a;color:${c}">难度${'★'.repeat(d)}${'☆'.repeat(5-d)}</span>`;
}
function renderMaterial(t){
  if(!t) return '';
  const lines=String(t).split('\n');
  let out=''; let i=0;
  while(i<lines.length){
    const line=lines[i]; const tr=line.trim();
    if(!tr){ i++; continue; }
    let m=tr.match(/^!\[([^\]]*)\]\(([^)]+)\)/);
    if(m){ out+=`<img class="matimg" src="${esc(m[2])}" alt="${esc(m[1]||'')}" loading="lazy">`; i++; continue; }
    if(/^<img[^>]+src=/i.test(tr)){ out+=tr; i++; continue; }
    if(/^\|.*\|$/.test(tr) && i+1<lines.length && /^\|[\s:|-]+\|$/.test(lines[i+1].trim())){
      const rows=[tr]; i+=2;
      while(i<lines.length && /^\|.*\|$/.test(lines[i].trim())){ rows.push(lines[i].trim()); i++; }
      out+=renderTable(rows); continue;
    }
    out+=`<div class="matline">${esc(tr)}</div>`; i++;
  }
  return out;
}
function renderTable(rows){
  let h='<table class="mattable">';
  rows.forEach((r,idx)=>{
    const cells=r.replace(/^\|/,'').replace(/\|$/,'').split('|').map(c=>c.trim());
    const tag=idx===0?'th':'td';
    h+='<tr>'+cells.map(c=>`<${tag}>${esc(c)}</${tag}>`).join('')+'</tr>';
  });
  return h+'</table>';
}
function renderReference(r){
  if(!r) return '';
  const s=String(r).trim();
  if(/^https?:\/\/\S+\.(png|jpe?g|gif|webp|svg)(\?\S*)?$/i.test(s) || /^<img/i.test(s)){
    return /^<img/i.test(s)? s : `<img class="refimg" src="${esc(s)}" alt="参考图" loading="lazy">`;
  }
  return `<div class="matline reftext">${esc(s)}</div>`;
}
function render(){
  if(!QS||!QS.length){ renderEmpty(); return; }
  if(IDX<0) IDX=0; if(IDX>=QS.length) IDX=QS.length-1;
  const cur=QS[IDX]; if(!cur){ renderEmpty(); return; }
  const q=QS[IDX], no=IDX+1, isFav=!!favs()[q.id];
  const pct=Math.round(IDX/QS.length*100);
  const subj=SUBJECTS.find(s=>s.id===CUR);
  let h=renderStatusbar()+`<div class="qcard">
    <div class="qmod"><span>${esc(q.module)}${q.type==='multi'?'（多选）':''} ${diffBadge(q.difficulty)}</span>
      <span class="no">第 ${no} / ${QS.length} 题
        <button class="toolbtn ${isFav?'on':''}" onclick="toggleFav('${q.id}')">${isFav?'⭐':'☆ 收藏'}</button>
        <button class="toolbtn" onclick="openReport('${q.id}')">⚠️ 纠错</button>
      </span></div>
    <div class="progress"><i style="width:${pct}%"></i></div>
    ${MOCK_ACTIVE&&MOCK?`<div class="mocktimer">⏱ 剩余 <b id="mcount">${fmtRemain(MOCK.deadline_ts)}</b></div>`:''}
    ${TARGETED?`<div class="tarnote">🎯 ${esc(TARGET_NOTE)}</div>`:''}
    <div class="stem">${esc(q.stem)}</div>`;
  if(q.material) h+=`<div class="material">${renderMaterial(q.material)}</div>`;
  if(q.reference && !MOCK_ACTIVE) h+=`<div class="reference">${renderReference(q.reference)}</div>`;
  if(isEssay()){
    const r=RESULTS[q.id];
    h+=`<textarea id="essay_${q.id}" placeholder="在此作答，写完点「查看参考答案」对比">${esc(ANSWERS[q.id]||'')}</textarea>
      <button class="btn ghost" onclick="submitEssay('${q.id}')">${r?'重新查看参考答案':'查看参考答案'}</button>`;
    if(r) h+=`<div class="feedback ok"><div class="fhead">参考答案 / 评分提示</div><div class="fans">${esc(r.reference||'（无）')}</div></div>`;
  }else{
    h+=`<div class="opts">${q.options.map((o,i)=>optHTML(q,o,i)).join('')}</div>`;
    if(!isMock()&&q.type==='multi'&&ANSWERS[q.id]&&!RESULTS[q.id]) h+=`<button class="btn" id="multibtn" onclick="submitMulti('${q.id}')">确定提交</button>`;
    if(!isMock()&&RESULTS[q.id]) h+=feedbackHTML(q);
  }
  h+=`</div>
    <div class="crumbs" style="padding-top:0;opacity:.85">🏠 <b onclick="goDashboard()">首页</b> › <b onclick="backToSubject()">${esc(subj?subj.name:'')}</b> › <span class="here">${esc(q.module)}</span></div>`;
  $('#quiz').innerHTML=h;
  bindOpts(q); bindEssay(q); renderFoot(no); renderPCside();
  window.scrollTo({top:0,behavior:'smooth'});
}
function optHTML(q,o,i){
  const key=o[0], r=RESULTS[q.id]; let cls='';
  if(r){ const right=r.right||'';
    if(right&&right.includes(key)) cls='ok';
    else if((ANSWERS[q.id]||'').includes(key)) cls='bad';
    else cls='lock';
  }else if((ANSWERS[q.id]||'').includes(key)) cls='sel';
  return `<label class="${cls}" data-q="${q.id}" data-k="${key}"><span class="letter">${key}</span><span>${esc(o.slice(2))}</span></label>`;
}
function feedbackHTML(q){
  const r=RESULTS[q.id];
  const open=isMock()?'show':'';
  const head=r.correct?'✓ 答对啦':'✗ 答错了，别急，记住这一个';
  const fans=`你的答案：<b>${esc(r.your||'（未答）')}</b>　正确答案：<b>${esc(r.right||'')}</b>`;
  return `<div class="feedback ${r.correct?'ok':'bad'}">
    <div class="fhead">${head}</div>
    <div class="fans">${fans}</div>
    ${r.explain?`<div class="explain-toggle" onclick="this.nextElementSibling.classList.toggle('show')">💡 ${open?'隐藏':'展开'}解析 ▾</div>
    <div class="explain-body ${open}">${esc(r.explain)}</div>`:''}
  </div>`;
}
function bindOpts(q){
  if(isEssay()||!q.options) return;
  $$('.opts label').forEach(lb=>{
    const key=lb.dataset.k;
    if(RESULTS[q.id]&&!isMock()){ lb.classList.add('lock'); return; }
    lb.onclick=()=>{
      if(isMock()){
        let arr=ANSWERS[q.id]||[];
        if(q.type==='multi'){ arr=arr.includes(key)?arr.filter(x=>x!==key):[...arr,key]; ANSWERS[q.id]=arr; lb.classList.toggle('sel',arr.includes(key)); }
        else { ANSWERS[q.id]=key; $$('.opts label').forEach(x=>x.classList.toggle('sel',x.dataset.k===key)); }
        return;
      }
      if(q.type==='multi'){
        let arr=ANSWERS[q.id]||[];
        arr=arr.includes(key)?arr.filter(x=>x!==key):[...arr,key];
        ANSWERS[q.id]=arr; lb.classList.toggle('sel',arr.includes(key));
        return;
      }
      ANSWERS[q.id]=key;
      submitOne(q.id,key);
    };
  });
}
function bindEssay(q){
  if(!isEssay()) return;
  const ta=$('#essay_'+q.id);
  if(ta) ta.oninput=()=>{ ANSWERS[q.id]=ta.value; };
}
function renderFoot(no){
  if(!QS.length){ $('#foot').innerHTML=''; return; }
  const q=QS[IDX], first=no<=1, last=no>=QS.length;
  const graded=!!RESULTS[q.id], answered=ANSWERS[q.id]!==undefined&&ANSWERS[q.id]!=='';
  let left=`<button onclick="go(-1)" ${first?'disabled':''}>← 上一题</button>`;
  let right=`<button onclick="go(1)" ${last?'disabled':''}>下一题 →</button>`;
  let main='';
  if(q.type==='essay'){
    main=`<button class="main" onclick="submitEssay('${q.id}')">${graded?'重新对照参考答案':'查看参考答案'}</button>`;
  }else if(isMock()){
    main=`<button class="main" onclick="submitMock()">⏱ 交卷并看成绩</button>`;
  }else if(q.type==='multi'){
    main=graded?`<button class="main" onclick="go(1)" ${last?'disabled':''}>下一题 →</button>`:`<button class="main" id="mb" onclick="submitMulti('${q.id}')" ${answered?'':'disabled'}>确定提交</button>`;
  }else{
    if(!graded){
      main=`<button class="main" onclick="nudge()" disabled>选一个答案吧</button>`;
    }else{
      if(MODE==='wrong') main=`<button class="main" onclick="markLearned('${q.id}')">✓ 标记为已懂</button>`;
      else if(MODE==='fav') main=`<button class="main" onclick="toggleFav('${q.id}');go(1)" ${last?'disabled':''}>下一道 ⭐</button>`;
      else if(last) main=`<button class="main" onclick="showStats()">完成，看成绩 🎉</button>`;
      else main='';
    }
  }
  $('#foot').innerHTML=`${left}${main}${right}`;
}
function nudge(){ toast('先点上面的选项哦～选完立刻出解析'); }
function markLearned(qid){ const wb=wrongBank(); delete wb[qid]; saveJ(LS_WRONG,wb); updateBell(); toast('✓ 已从错题本移除'); go(1); }
function go(d){ if(!QS.length) return; IDX=Math.max(0,Math.min(QS.length-1,IDX+d)); localStorage.setItem('shuati_last_index',IDX); render(); }
window.addEventListener('keydown',e=>{
  if(VIEW!=='quiz'||!$('#quiz').offsetParent) return;
  if(e.key==='ArrowRight'||e.key==='ArrowDown') go(1);
  if(e.key==='ArrowLeft'||e.key==='ArrowUp') go(-1);
});

/* ---------- 答题（真实后端 /api/check） ---------- */
async function apiCheck(payload){
  payload.date=todayStr();
  const r=await api('/api/check',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(payload)});
  return r.json();
}
async function submitOne(qid,val){
  try{
    const d=await apiCheck({answers:{[qid]:val}});
    if(d.results&&d.results[0]){ RESULTS[qid]=d.results[0]; if(d.results[0].correct!==undefined) markGrade(qid,d.results[0].correct); }
  }catch(e){ toast('网络错误，请重试'); }
  render();
}
async function submitMulti(qid){
  const v=ANSWERS[qid]||[];
  if(!v.length){ toast('请先选择选项'); return; }
  await submitOne(qid,v.join(''));
}
async function submitEssay(qid){
  const v=ANSWERS[qid]||($('#essay_'+qid)&&$('#essay_'+qid).value)||'';
  ANSWERS[qid]=v;
  if(!v.trim()){ toast('请先写下你的作答'); return; }
  await submitOne(qid,v);
}

/* ---------- PC 热力图答题卡 ---------- */
function renderPCside(){
  const inner=$('#pcsideinner'); if(!inner) return;
  if(!PCMODE||VIEW!=='quiz'){ inner.innerHTML=''; return; }
  if(!QS.length){ inner.innerHTML=''; return; }
  const done=Object.keys(ANSWERS).filter(k=>ANSWERS[k]!==undefined&&ANSWERS[k]!=='').length;
  let segs=[],streak=0;
  QS.forEach((q,i)=>{
    const r=RESULTS[q.id];
    if(r&&r.correct===true){ streak++; }
    else{
      if(streak>0){ segs.push({len:streak,type:'ok'}); streak=0; }
      if(r&&r.correct===false) segs.push({len:1,type:'bad'});
      else if(ANSWERS[q.id]!==undefined&&ANSWERS[q.id]!=='') segs.push({len:1,type:'done'});
      else segs.push({len:1,type:'none'});
    }
  });
  if(streak>0) segs.push({len:streak,type:'ok'});
  const color={ok:'#22c55e',bad:'#ef4444',done:'#93c5fd',none:'#e5e9f0'};
  const legend={ok:'对',bad:'错',done:'已答',none:'未答'};
  let grid='',c=0;
  segs.forEach(s=>{for(let j=0;j<s.len;j++){const i=c++;const cur=i===IDX;
    grid+=`<button class="pcgrid" style="background:${color[s.type]};${s.type==='none'?'color:#9aa4b2':'color:#fff'};${cur?'outline:2.5px solid #111;outline-offset:1px;transform:scale(1.08)':''}" onclick="go(${i-IDX})">${i+1}</button>`;}});
  const sum=t=>segs.filter(s=>s.type===t).reduce((a,s)=>a+s.len,0);
  inner.innerHTML=`<div class="pcside-head">答题卡 <span class="pc-progress">${done}/${QS.length}</span></div>
    <div class="pcgrid-wrap">${grid||'<p class="empty-hint">暂无题目</p>'}</div>
    <div class="pc-stats">
      <span><i class="dot" style="background:#22c55e"></i>对 ${sum('ok')}</span>
      <span><i class="dot" style="background:#ef4444"></i>错 ${sum('bad')}</span>
      <span><i class="dot" style="background:#93c5fd"></i>未批 ${sum('done')}</span>
      <span><i class="dot" style="background:#e5e9f0;border:1px solid #d8dde6"></i>剩 ${sum('none')}</span>
    </div>
    <button class="btn ghost" style="margin-top:14px;padding:10px;font-size:13.5px" onclick="openSheet()">📋 逐题跳转</button>`;
}
function openSheet(){
  let g='';
  QS.forEach((q,i)=>{
    const r=RESULTS[q.id];
    const cls=r?(r.correct?'ok':'bad'):(ANSWERS[q.id]!==undefined&&ANSWERS[q.id]!==''?'done':'');
    g+=`<button class="${cls}" onclick="go(${i});closeSheet()">${i+1}</button>`;
  });
  $('#sheetbody').innerHTML=`<div class="sheethead">答题卡 · ${Object.keys(RESULTS).length} 已答 <button class="closebtn" onclick="closeSheet()">✕</button></div><div class="sheetgrid">${g}</div>`;
  $('#sheetmask').classList.add('open');
}
function closeSheet(){ $('#sheetmask').classList.remove('open'); }

/* ---------- 统计弹层（/api/stats + /api/daily-stats） ---------- */
async function showStats(){
  const body=$('#statbody');
  body.innerHTML='<div class="empty-hint">加载中…</div>';
  $('#statmask').classList.add('open');
  let d={modules:[],overall:0,total:0,correct:0,weak:[]}, dl={days:[],today:null};
  try{ d=await (await api('/api/stats')).json(); }catch(e){}
  try{ dl=await (await api('/api/daily-stats')).json(); }catch(e){}
  const nm={}; SUBJECTS.forEach(x=>nm[x.id]=x.name);
  let dailyHtml='<div class="statadvice" style="margin-top:10px;text-align:left">📅 每日刷题</div>';
  if(!dl.days||!dl.days.length){
    dailyHtml+='<div class="empty-hint" style="font-size:12px;padding:4px 0">还没有每日记录。做题后这里按天统计。</div>';
  }else{
    const last7=dl.days.slice(0,7);
    dailyHtml+=last7.map(x=>{
      const pct=x.pct||0;
      return `<div class="dailyrow"><span class="dd">${x.date===(dl.today&&dl.today.date)?'今天':(x.date||'')}</span>
        <span class="bar"><i class="${pct<60?'weak':''}" style="width:${Math.min(pct,100)}%"></i></span>
        <span class="pct">${pct}%</span><span class="d">${x.c}/${x.t}</span></div>`;
    }).join('');
    if(dl.days.length>7) dailyHtml+=`<div class="empty-hint" style="font-size:12px;padding:4px 0">…共 ${dl.days.length} 天记录</div>`;
  }
  const wbN=Object.keys(wrongBank()).length, dueN=Object.keys(srs()).filter(isDue).length;
  const mods=d.modules||[];
  if(!mods.length){
    body.innerHTML=`<div class="sheethead">📊 我的数据 <button class="closebtn" onclick="closeStats()">✕</button></div>
      <div class="statoverall">${d.overall||0}%</div>
      <div style="text-align:center;color:var(--mut);font-size:13px;margin-bottom:6px">还没有成绩记录，做题后按模块统计正确率</div>
      <div class="statadvice">📕 错题本 ${wbN} 题 · 🔁 待复习 ${dueN} 题</div>${dailyHtml}`;
    return;
  }
  const rows=mods.map(m=>{
    const weak=m.pct<60?'weak':'';
    return `<div class="statrow"><span class="mod">${esc(nm[m.subject]||m.subject)}·${esc(m.module)}</span>
      <span class="bar"><i class="${weak}" style="width:${Math.min(m.pct,100)}%"></i></span>
      <span class="pct">${m.pct}%</span><span class="d">${m.correct}/${m.total}</span></div>`;
  }).join('');
  const weakTxt=(d.weak||[]).map(w=>`${nm[w.subject]||w.subject}·${w.module}（${w.pct}%）`).join('、');
  const adv=(d.weak&&d.weak.length)?`⚡ 最薄弱：${esc(weakTxt)}——用「先攻弱项」补强。`:`✅ 整体不错，可进阶套题/限时。`;
  body.innerHTML=`<div class="sheethead">📊 我的数据 <button class="closebtn" onclick="closeStats()">✕</button></div>
    <div class="statoverall">${d.overall||0}%</div>
    <div style="text-align:center;color:var(--mut);font-size:13px;margin-bottom:14px">累计 ${d.correct||0} / ${d.total||0} 题已判 · 当前科目：${esc((SUBJECTS.find(s=>s.id===CUR)||{}).name||'—')}</div>
    <div>${rows}</div>
    <div class="statadvice">${adv}</div>
    <div class="statadvice">📕 错题本 ${wbN} 题 · 🔁 待复习 ${dueN} 题 <button class="btn ghost" style="margin-top:6px" onclick="closeStats();MODE='wrong';loadQuiz()">去刷错题</button> <button class="btn ghost" style="margin-top:6px" onclick="closeStats();MODE='srs';loadQuiz()">去复习</button></div>
    ${dailyHtml}`;
}
function closeStats(){ $('#statmask').classList.remove('open'); }

/* ---------- 筛选弹层 ---------- */
function openFilter(){
  const subj=SUBJECTS.find(s=>s.id===CUR)||SUBJECTS[0]||{};
  const mods=(subj.modules||[]).filter(Boolean);
  $('#filterbody').innerHTML=`
    <div class="sheethead">⚙️ 指定范围刷题 <button class="closebtn" onclick="closeFilter()">✕</button></div>
    <div class="frow"><label>科目</label><select id="fs" onchange="renderFilterMods()">
      ${SUBJECTS.map(s=>`<option value="${s.id}" ${s.id===CUR?'selected':''}>${esc(s.name)}</option>`).join('')}</select></div>
    <div class="frow"><label>模块</label><select id="fm"><option value="">全部模块</option>${mods.map(m=>`<option ${FILTER.module===m?'selected':''}>${esc(m)}</option>`).join('')}</select></div>
    <div class="frow"><label>最低难度（${FILTER.difficulty||'不限'} 星起）</label><select id="fd" onchange="FILTER.difficulty=+this.value">
      ${[0,3,4,5].map(d=>`<option value="${d}" ${FILTER.difficulty===d?'selected':''}>${d?d+' 星及以上':'不限'}</option>`).join('')}</select></div>
    <button class="btn" onclick="applyFilterAndGo()">开始刷这组题 →</button>
    <button class="btn ghost" onclick="FILTER={module:'',difficulty:0};applyFilterAndGo()">重置筛选</button>`;
  $('#filtermask').classList.add('open');
}
function renderFilterMods(){
  const s=SUBJECTS.find(x=>x.id===$('#fs').value);
  $('#fm').innerHTML='<option value="">全部模块</option>'+((s&&s.modules)||[]).filter(Boolean).map(m=>`<option>${esc(m)}</option>`).join('');
}
function applyFilterAndGo(){
  CUR=$('#fs').value; FILTER.module=$('#fm').value; FILTER.difficulty=+($('#fd').value||0);
  closeFilter(); MODE='normal'; loadQuiz();
}
function closeFilter(){ $('#filtermask').classList.remove('open'); }

/* ---------- 截止日（/api/exam-deadline） ---------- */
function daysUntil(dateStr){
  if(!dateStr) return null;
  const d=new Date(dateStr+'T00:00:00'), now=new Date(); now.setHours(0,0,0,0);
  return Math.round((d-now)/86400000);
}
function openDeadline(){
  const g=daysUntil(EXAMS.guokao), s=daysUntil(EXAMS.shengkao);
  $('#dlbody').innerHTML=`
    <div class="sheethead">⏰ 考试倒计时 <button class="closebtn" onclick="closeDeadline()">✕</button></div>
    <div class="frow"><label>国考笔试</label><input type="date" id="dlg" value="${esc(EXAMS.guokao||'')}"></div>
    <div class="frow"><label>广东省考笔试</label><input type="date" id="dls" value="${esc(EXAMS.shengkao||'')}"></div>
    <div style="text-align:center;padding:10px 0">
      <div style="font-size:44px;font-weight:800;color:var(--blue);font-variant-numeric:tabular-nums">${g==null&&s==null?'—':(g!=null&&s!=null?`国${g<0?'已过':g+'天'} · 省${s<0?'已过':s+'天'}`:(g!=null?(g<0?'已过':g+'天后国考'):(s<0?'已过':s+'天后省考')))}</div>
      <div style="color:var(--mut);font-size:13px">距开考剩余天数</div>
    </div>
    <button class="btn" onclick="saveDeadline()">保存</button>
    <button class="btn ghost" onclick="clearDeadline()">清除</button>`;
  $('#dlmask').classList.add('open');
}
async function saveDeadline(){
  const g=$('#dlg').value, s=$('#dls').value;
  try{
    const r=await api('/api/exam-deadline',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({guokao:g,shengkao:s})});
    const d=await r.json();
    if(d.ok){ EXAMS={guokao:d.guokao||'',shengkao:d.shengkao||''}; toast('✅ 已保存'); }
  }catch(e){ toast('保存失败'); }
  closeDeadline();
}
async function clearDeadline(){
  try{
    await api('/api/exam-deadline',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({guokao:'',shengkao:''})});
    EXAMS={guokao:'',shengkao:''}; toast('已清除');
  }catch(e){ toast('清除失败'); }
  closeDeadline();
}
async function loadExamDeadline(){
  try{
    const r=await api('/api/exam-deadline');
    if(r.status===200){ EXAMS=(await r.json())||{}; }
  }catch(e){}
}
function closeDeadline(){ $('#dlmask').classList.remove('open'); }

/* ---------- 学习报告（/api/report-dates + /api/report-view） ---------- */
function closeReportView(){ $('#reportmask').classList.remove('open'); }
async function showReportView(date){
  $('#reportmask').classList.add('open');
  const body=$('#reportviewbody');
  body.innerHTML='<div class="rv-empty">加载中…</div>';
  let dates=[];
  try{ const r=await api('/api/report-dates'); if(r.status===200){ dates=(await r.json()).dates||[]; } }catch(e){}
  let d=date||dates[0]||'';
  if(!d){
    body.innerHTML=`<div class="reportview"><h2>📋 学习报告</h2><div class="rv-empty">还没有报告。每晚 21:00 自动生成，完成练习后次日可查看。</div><div class="rv-nav"><button class="btn" onclick="closeReportView()">关闭</button></div></div>`;
    return;
  }
  try{
    const r=await api('/api/report-view?date='+encodeURIComponent(d));
    if(r.status===200){ const txt=await r.text(); body.innerHTML=`<div class="reportview"><h2>📋 学习报告</h2><div class="rv-date">${esc(d)}</div><div class="rv-md">${renderMd(txt)}</div>${reportNav(dates,d)}</div>`; }
    else { const e=await r.json().catch(()=>({})); body.innerHTML=`<div class="reportview"><h2>📋 学习报告</h2><div class="rv-empty">${esc(e.err||'该日暂无报告')}</div>${reportNav(dates,d)}</div>`; }
  }catch(e){ body.innerHTML=`<div class="reportview"><h2>📋 学习报告</h2><div class="rv-empty">加载失败</div></div>`; }
}
function reportNav(dates,cur){
  if(!dates.length) return '<div class="rv-nav"><button class="btn" onclick="closeReportView()">关闭</button></div>';
  return '<div class="rv-nav">'+dates.map(x=>`<button class="btn ${x===cur?'ghost':''}" onclick="showReportView('${x}')">${x}</button>`).join('')+'<button class="btn" onclick="closeReportView()">关闭</button></div>';
}
function renderMd(t){
  let lines=t.split('\n'); let out=[], inTable=false, inList=false, hasRow=false;
  for(const raw of lines){
    const line=raw.replace(/\r$/,'');
    if(/^\|/.test(line)){
      if(!inTable){ out.push('<table>'); inTable=true; hasRow=false; }
      const cells=line.trim().slice(1,-1).split('|').map(c=>c.trim());
      if(!cells.some(c=>/^-+$/.test(c))){ const tag=hasRow?'td':'th'; out.push('<tr>'+cells.map(c=>`<${tag}>${c}</${tag}>`).join('')+'</tr>'); hasRow=true; }
      continue;
    } else if(inTable){ out.push('</table>'); inTable=false; }
    if(/^\s*[-*] /.test(line)){ if(!inList){ out.push('<ul>'); inList=true; } out.push('<li>'+line.replace(/^\s*[-*] /,'')+'</li>'); continue; }
    else if(inList){ out.push('</ul>'); inList=false; }
    if(/^#{1,3}\s/.test(line)){ const lv=line.match(/^#+/)[0].length; out.push(`<h${lv}>`+line.replace(/^#+\s*/,'')+`</h${lv}>`); continue; }
    if(/^\s*$/.test(line)){ continue; }
    if(/^\*\*.*\*\*$/.test(line.trim())){ out.push('<p><b>'+line.trim().slice(2,-2)+'</b></p>'); continue; }
    out.push('<p>'+line+'</p>');
  }
  if(inTable) out.push('</table>'); if(inList) out.push('</ul>');
  return out.join('');
}

/* ---------- 模考（/api/mock/*） ---------- */
function fmtRemain(deadline){
  const ms=(deadline*1000)-Date.now();
  if(ms<=0) return '00:00';
  const s=Math.floor(ms/1000);
  return String(Math.floor(s/60)).padStart(2,'0')+':'+String(s%60).padStart(2,'0');
}
function startMockTimer(){
  stopMockTimer();
  if(!MOCK) return;
  MOCK_TIMER=setInterval(()=>{
    const el=$('#mcount'); if(el) el.textContent=fmtRemain(MOCK.deadline_ts);
    if(Date.now()>MOCK.deadline_ts*1000){ toast('⏰ 时间到，自动交卷'); submitMock(); }
  },1000);
}
function stopMockTimer(){ if(MOCK_TIMER){ clearInterval(MOCK_TIMER); MOCK_TIMER=null; } }
function startMock(){
  $('#mockbody').innerHTML=`
    <div class="sheethead">📝 模拟考试 <button class="closebtn" onclick="closeMock()">✕</button></div>
    <p style="color:var(--mut);font-size:13px;margin:0 0 10px">选一套卷子，按真实时长倒计时，到点自动交卷。</p>
    <div class="mocksel">${PAPERS.map(p=>`<button onclick="launchMock('${p.type}','${p.paper}')">${p.emoji} ${p.label}<br><small style="color:var(--mut);font-weight:400">${p.count} 题 · ${p.min} 分钟</small></button>`).join('')}</div>
    <button class="btn ghost" onclick="showMockHistory()">📜 模考历史</button>`;
  $('#mockmask').classList.add('open');
}
function closeMock(){ $('#mockmask').classList.remove('open'); }
async function launchMock(type,paper){
  closeMock(); toast('正在组卷…');
  let d;
  try{ d=await (await api('/api/mock/generate?type='+type+'&paper='+paper)).json(); }
  catch(e){ toast('网络错误，请重试'); return; }
  if(!d.ok){ toast((d.notes&&d.notes[0])||d.err||'组卷失败'); return; }
  MOCK=d; MOCK_ACTIVE=true; TARGETED=false; TARGET_NOTE=''; CUR='mock';
  RAW_QS=d.questions; QS=d.questions; ANSWERS={}; RESULTS={}; IDX=0;
  MOCK_DEADLINE=d.deadline_ts;
  $('#subname').textContent=`📝 ${PAPER_NAME(type,paper)} · ${d.count}题 · ${d.duration_min}分钟`;
  if(d.notes&&d.notes.length) toast('⚠️ '+d.notes.join('；'));
  startMockTimer(); enterQuiz();
}
async function submitMock(){
  if(!MOCK) return;
  const payload={};
  QS.forEach(q=>{ if(ANSWERS[q.id]!==undefined&&ANSWERS[q.id]!=='') payload[q.id]=ANSWERS[q.id]; });
  const answered=Object.keys(payload).length;
  if(!answered){ toast('还没有作答'); return; }
  if(answered<QS.length && !confirm(`还有 ${QS.length-answered} 题未答，确定交卷？`)) return;
  let d;
  try{
    d=await (await api('/api/mock/submit',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({paper_id:MOCK.paper_id,answers:payload,date:todayStr()})})).json();
  }catch(e){ toast('网络错误，请重试'); return; }
  if(!d.ok){ toast(d.err||'交卷失败'); return; }
  stopMockTimer(); MOCK_ACTIVE=false;
  (d.results||[]).forEach(r=>{ RESULTS[r.id]=r; if(r.correct!==undefined) markGrade(r.id,r.correct); });
  showScorecard(d);
  const mtype=MOCK.type, mpaper=MOCK.paper; MOCK=null;
  $('#subname').textContent='📝 '+PAPER_NAME(mtype,mpaper)+' · 已交卷';
}
function showScorecard(d){
  const obj=d.objective_total||0, oc=d.objective_correct||0;
  $('#quiz').innerHTML=`<div class="qcard scorecard">
    <div class="big">${d.score}分</div>
    <div class="sub">客观题 ${oc}/${obj} 正确 · 共 ${d.total} 题</div>
    <button class="btn" onclick="openSheet()">查看答题卡</button>
    <button class="btn ghost" onclick="goDashboard()">返回首页</button>
  </div>`;
  $('#foot').innerHTML=`<button class="main" onclick="openSheet()">查看答题卡</button><button onclick="goDashboard()">返回首页</button>`;
  renderPCside();
}
async function showMockHistory(){
  let d={history:[]};
  try{ d=await (await api('/api/mock/history')).json(); }catch(e){}
  const list=d.history||[];
  let h='<div class="sheethead">📜 模考历史 <button class="closebtn" onclick="closeMock()">✕</button></div>';
  if(!list.length) h+='<p class="hint">还没有模考记录。先做一份模考吧。</p>';
  else h+='<div class="mockhist">'+list.map(m=>`<div class="mockhistrow"><span class="mtype">${m.type==='shengkao'?'省':'国'}</span><span class="mname">${esc(PAPER_NAME(m.type||'guokao',m.paper||'xingce'))}</span><span class="mdate">${esc(m.date||'')}</span><b>${m.score}分</b><small>${m.objective_correct||0}/${m.objective_total||0}</small></div>`).join('')+'</div>';
  $('#mockbody').innerHTML=h;
  $('#mockmask').classList.add('open');
}

/* ---------- 针对性刷题（/api/targeted） ---------- */
async function startTargeted(){
  toast('正在分析薄弱项…');
  let d;
  try{ d=await (await api('/api/targeted?count=20')).json(); }
  catch(e){ toast('网络错误，请重试'); return; }
  if(!d.ok||!d.questions||!d.questions.length){ toast(d.err||'暂无题目'); return; }
  MOCK_ACTIVE=false; MOCK=null; TARGETED=true; stopMockTimer(); CUR='targeted';
  RAW_QS=d.questions; QS=d.questions; ANSWERS={}; RESULTS={}; IDX=0;
  TARGET_NOTE=d.note||'';
  $('#subname').textContent='🎯 针对性刷题 · '+QS.length+'题';
  enterQuiz();
}

/* ---------- PC 布局 ---------- */
function initPC(){
  const saved=localStorage.getItem('shuati_pc');
  PCMODE = saved===null ? (window.innerWidth>=1024) : (saved==='1');
  applyPC();
}
function togglePC(){ PCMODE=!PCMODE; try{localStorage.setItem('shuati_pc',PCMODE?'1':'0');}catch(e){} applyPC(); }
function applyPC(){
  document.body.classList.toggle('pc',PCMODE);
  renderPCside();
}

window.addEventListener('error',e=>console.log('[ERR] '+e.message+' @ '+(e.filename||'')+':'+(e.lineno||'')));
/* ---------- 启动 ---------- */
initPC();
bootAuth();
