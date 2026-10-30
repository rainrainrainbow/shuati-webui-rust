// 国考刷题 — 粉笔式一题一卡 + 科学刷题
// 科学方法：薄弱优先 / 错题重练 / 间隔重复(Leitner) / 交错练习；主动回忆=答题后再看答案
let SUBJECTS=[], CUR='', QS=[], IDX=0, ANSWERS={}, RESULTS={}, GRADED=false;
let MODE='normal';
let CAT='全部', SEARCH='';
let PCMODE=false;
const MOCK_IDS=['zt2019','zt2020','zt2021','zt2022','zt2023'];
const LS_WRONG='shuati_wrong', LS_SRS='shuati_srs', LS_FAV='shuati_fav';
const MODES=[['normal','常规'],['weak','薄弱优先'],['wrong','错题重练'],['srs','间隔复习'],['shuffle','交错'],['fav','⭐ 收藏']];
const SRS_DAYS=[0,1,3,7,15,30]; // box1~5 间隔
const $=s=>document.querySelector(s);
const $$=s=>document.querySelectorAll(s);
const isMock=()=>MOCK_IDS.includes(CUR);
const isEssay=()=>QS.length>0 && QS[0].type==='essay';

// ---- 多用户认证 ----
let TOKEN=localStorage.getItem('shuati_token')||'';
let USERNAME=localStorage.getItem('shuati_user')||'';
function api(path,opts){
  opts=opts||{};
  opts.headers=Object.assign({},opts.headers||{});
  if(TOKEN) opts.headers['Authorization']='Bearer '+TOKEN;
  return fetch(path,opts);
}
function requireLogin(){
  if(TOKEN) return;
  $('#loginmask').classList.remove('hide');
}
function onAuthed(name){
  USERNAME=name; localStorage.setItem('shuati_user',name);
  $('#uname').textContent=name;
  $('#loginmask').classList.add('hide');
  $('#subname').textContent=' · '+name;
  loadSubjects();
}
function logout(){
  if(TOKEN){ fetch('/api/logout',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({token:TOKEN})}).catch(()=>{}); }
  TOKEN=''; USERNAME=''; localStorage.removeItem('shuati_token'); localStorage.removeItem('shuati_user');
  $('#uname').textContent=''; $('#subname').textContent='';
  $('#loginmask').classList.remove('hide');
}
async function doLogin(){
  const u=$('#loginuser').value.trim(), p=$('#loginpass').value;
  $('#loginerr').textContent='';
  if(!u||!p){ $('#loginerr').textContent='请输入用户名和密码'; return; }
  try{
    const r=await fetch('/api/login',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({username:u,password:p})});
    const d=await r.json();
    if(d.ok){ TOKEN=d.token; localStorage.setItem('shuati_token',d.token); onAuthed(d.name||d.username); }
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
    if(d.ok){ TOKEN=d.token; localStorage.setItem('shuati_token',d.token); onAuthed(d.name||d.username); toast('✅ 注册成功，欢迎 '+d.name); }
    else $('#loginerr').textContent=d.err||'注册失败';
  }catch(e){ $('#loginerr').textContent='网络错误'; }
}
// 进入页面：有 token 则验证，否则弹登录
async function bootAuth(){
  if(!TOKEN){ requireLogin(); return; }
  try{
    const r=await api('/api/me');
    if(r.status===200){ const d=await r.json(); onAuthed(d.name||d.username); }
    else { TOKEN=''; localStorage.removeItem('shuati_token'); requireLogin(); }
  }catch(e){ requireLogin(); }
}

// ---- 错题本 + SRS (localStorage) ----
function loadJ(k){ try{return JSON.parse(localStorage.getItem(k))||{};}catch(e){return {};} }
function saveJ(k,v){ try{localStorage.setItem(k,JSON.stringify(v));}catch(e){} }
const wrongBank=()=>loadJ(LS_WRONG);
const srs=()=>loadJ(LS_SRS);
function markGrade(qid,correct,subject){
  // 错题本
  const wb=wrongBank();
  if(correct){ delete wb[qid]; }
  else {
    const q=QS.find(x=>x.id===qid);
    wb[qid]={stem:q?q.stem.slice(0,60):qid,module:q?q.module:'',subject,n:(wb[qid]&&wb[qid].n||0)+1,at:Date.now()};
  }
  saveJ(LS_WRONG,wb);
  // SRS Leitner
  const s=srs(), rec=s[qid]||{box:0,next:0};
  if(correct){ rec.box=Math.min(rec.box+1,5); rec.next=Date.now()+SRS_DAYS[rec.box]*86400000; }
  else { rec.box=0; rec.next=Date.now(); }
  s[qid]=rec; saveJ(LS_SRS,s);
}
function isDue(qid){
  const r=srs()[qid]; return !r || r.box===0 || r.next<=Date.now();
}

// ---- 收藏 + 题目纠错反馈 ----
const favs=()=>loadJ(LS_FAV);
function toggleFav(qid){
  const f=favs(); const q=QS.find(x=>x.id===qid);
  if(f[qid]){ delete f[qid]; toast('已取消收藏'); }
  else { f[qid]={stem:q?q.stem.slice(0,60):qid,module:q?q.module:'',at:Date.now()}; toast('⭐ 已收藏'); }
  saveJ(LS_FAV,f); render(); renderPCside();
}
function openReport(qid){
  const q=QS.find(x=>x.id===qid);
  const reasons=['答案解析有误','答案有误','题目不完整/缺项','选项有误','题干有误','其他'];
  const opts=reasons.map((r,i)=>`<label class="rep-opt"><input type="radio" name="rep" value="${r}" ${i===0?'checked':''}> ${r}</label>`).join('');
  const m=$('#reportmask');
  $('#reportbody').innerHTML=`
    <div class="sheethead">纠错反馈 <button class="x" onclick="closeReport()">✕</button></div>
    <div class="rep-stem">${esc((q&&q.stem||'').slice(0,120))}</div>
    <div class="rep-opts">${opts}</div>
    <textarea id="repnote" class="rep-note" placeholder="补充说明（可选）：为什么错了？正确应是什么？"></textarea>
    <button class="btn" onclick="submitReport('${qid}')">提交反馈</button>`;
  m.classList.add('show');
}
function closeReport(){ $('#reportmask').classList.remove('show'); }
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

// ---- 模式栏 ----
function renderModebar(){
  const mb=$('#modebar'); mb.innerHTML='';
  MODES.forEach(([id,label])=>{
    const b=document.createElement('button');
    b.className='modebtn'+(MODE===id?' active':'');
    b.textContent=label;
    b.onclick=()=>{ MODE=id; loadQuiz(); };
    mb.appendChild(b);
  });
}
function shuffleArr(a){ const x=a.slice(); for(let i=x.length-1;i>0;i--){const j=Math.floor(Math.random()*(i+1));[x[i],x[j]]=[x[j],x[i]];} return x; }

// ---- PC 模式（宽屏两栏：左题目，右侧答题卡面板）----
function initPC(){
  const saved=localStorage.getItem('shuati_pc');
  PCMODE = saved===null ? (window.innerWidth>=1024) : (saved==='1');
  applyPC();
}
function togglePC(){ PCMODE=!PCMODE; try{localStorage.setItem('shuati_pc',PCMODE?'1':'0');}catch(e){} applyPC(); }
function applyPC(){
  document.body.classList.toggle('pc',PCMODE);
  const t=$('#pctoggle'); if(t) t.textContent=PCMODE?'📱 移动':'💻 PC';
  renderPCside();
}
function renderPCside(){
  const inner=$('#pcsideinner'); if(!inner) return;
  if(!PCMODE){ inner.innerHTML=''; return; }
  const done=Object.keys(ANSWERS).length;
  let grid='';
  QS.forEach((q,i)=>{
    const r=RESULTS[q.id];
    const cls=r?(r.correct?'ok':'bad'):(ANSWERS[q.id]!==undefined?'done':'');
    grid+=`<button class="pcgrid ${cls}${i===IDX?' cur':''}" onclick="goPC(${i})">${i+1}</button>`;
  });
  inner.innerHTML=`<div class="pcside-head">答题卡 <span class="pc-progress">${done}/${QS.length}</span></div>
    <div class="pcgrid-wrap">${grid||'<p class="hint">暂无题目</p>'}</div>
    <button class="btn ghost" style="margin-top:12px;width:100%" onclick="showStats()">📊 成绩分析</button>`;
}
function goPC(i){ IDX=i; render(); }

// ---- 模式应用 ----
async function applyMode(qs){
  const isObj=qs.length>0 && qs[0].type!=='essay';
  if(MODE==='shuffle') return shuffleArr(qs);
  if(MODE==='weak' && isObj){
    let st={};
    try{ const d=await (await api('/api/stats')).json(); (d.modules||[]).forEach(m=>{ if(m.subject===CUR) st[m.module]=m.pct; }); }catch(e){}
    const wk=qs.filter(q=>st[q.module]!==undefined && st[q.module]<60);
    const rest=qs.filter(q=>!(st[q.module]!==undefined && st[q.module]<60));
    if(wk.length) toast('薄弱优先：'+wk.length+' 道薄弱题提前');
    return wk.concat(rest);
  }
  if(MODE==='wrong'){
    const wb=wrongBank(); const arr=qs.filter(q=>wb[q.id]);
    return arr;
  }
  if(MODE==='fav'){
    const f=favs(); const arr=qs.filter(q=>f[q.id]);
    if(!arr.length) toast('还没有收藏题目（答题页点 ☆）');
    return arr;
  }
  if(MODE==='srs' && isObj){
    const arr=qs.filter(q=>isDue(q.id));
    return arr;
  }
  return qs;
}

async function loadSubjects(){
  const d=await (await api('/api/subjects')).json();
  SUBJECTS=d.subjects;
  renderCatbar();
  renderSubjects();
  renderModebar();
  CUR=SUBJECTS[0].id; loadQuiz();
}
function cats(){ return ['全部',...new Set(SUBJECTS.map(s=>catOf(s)))]; }
function catOf(s){
  if(s.category) return s.category;
  const id=s.id||'';
  if(id==='jisuanji') return '专业课';
  if(/^zx/.test(id)) return '专项';
  if(/^zt/.test(id)) return '金管局真题';
  if(/^(xingce|yanyu|panduan|ziliao|changshi|shuliang|zhengzhi)$/.test(id)) return '国考行测';
  if(/^slzt/.test(id)) return '申论真题';
  if(/^sl/.test(id)||id==='shenlun') return '国考申论';
  return '其他';
}
function renderCatbar(){
  const cb=$('#catbar'); cb.innerHTML='';
  cats().forEach(c=>{
    const b=document.createElement('button');
    b.className='catbtn'+(CAT===c?' active':'');
    const n=CATCOUNT(c);
    b.textContent=n>0?`${c}${n>0&&c!=='全部'?` ${n}`:''}`:c;
    b.onclick=()=>{ CAT=c; renderCatbar(); renderSubjects(); };
    cb.appendChild(b);
  });
}
function CATCOUNT(c){
  if(c==='全部') return SUBJECTS.length;
  return SUBJECTS.filter(s=>catOf(s)===c).length;
}
function onSearch(v){ SEARCH=(v||'').toLowerCase().trim(); renderSubjects(); }
function filteredSubjects(){
  let arr=SUBJECTS;
  if(CAT!=='全部') arr=arr.filter(s=>catOf(s)===CAT);
  if(SEARCH) arr=arr.filter(s=>s.name.toLowerCase().includes(SEARCH)||s.id.toLowerCase().includes(SEARCH));
  return arr;
}
function renderSubjects(){
  const box=$('#subjects'); box.innerHTML='';
  const arr=filteredSubjects();
  if(!arr.length){ box.innerHTML='<p class="hint">无匹配科目</p>'; return; }
  arr.forEach(s=>{
    const b=document.createElement('button');
    b.innerHTML=`${s.name}<small>${s.count}</small>`;
    b.onclick=()=>{ if(CUR!==s.id){CUR=s.id; loadQuiz();} };
    box.appendChild(b);
  });
  $$('.subjects button').forEach((b,i)=>{b.classList.toggle('active', arr[i]&&arr[i].id===CUR);});
}
async function loadQuiz(){
  GRADED=false; ANSWERS={}; RESULTS={}; IDX=0;
  const d=await (await api('/api/questions?subject='+CUR)).json();
  const qs=await applyMode(d.questions);
  QS=qs;
  const sub=SUBJECTS.find(s=>s.id===CUR);
  let label=sub?sub.name+' · '+QS.length+'题':'';
  if(MODE==='wrong') label+=' · 错题重练';
  else if(MODE==='srs') label+=' · 间隔复习';
  else if(MODE==='weak') label+=' · 薄弱优先';
  else if(MODE==='shuffle') label+=' · 交错';
  $('#subname').textContent=label;
  $$('.subjects button').forEach((b,i)=>{b.classList.toggle('active', SUBJECTS[i]&&SUBJECTS[i].id===CUR);});
  $$('#modebar .modebtn').forEach((b,i)=>{b.classList.toggle('active', MODES[i][0]===MODE);});
  render();
}
function render(){
  if(!QS.length){
    let hint='该科目暂无题目';
    if(MODE==='wrong') hint='🎯 暂无错题 —— 继续保持！';
    else if(MODE==='srs') hint='🔁 暂无到期待复习题 —— 先做「常规」积累';
    else if(MODE==='weak') hint='✅ 该科目暂无薄弱模块（或先做几题产生统计）';
    $('#quiz').innerHTML=`<div class="qcard hintcard"><p class="hint">${hint}</p></div>`;
    $('#foot').innerHTML=`<button class="main" onclick="MODE='normal';loadQuiz()">回到常规</button>`;
    return;
  }
  const q=QS[IDX], no=IDX+1;
  const isFav=!!favs()[q.id];
  const pct=Math.round((IDX)/(QS.length)*100);
  let h=`<div class="qcard">
    <div class="qmod"><span>${esc(q.module)}${q.type==='multi'?'（多选）':''}</span><span class="no">第${no}/${QS.length}题 <button class="toolbtn ${isFav?'on':''}" onclick="toggleFav('${q.id}')">${isFav?'⭐':'☆'}收藏</button><button class="toolbtn" onclick="openReport('${q.id}')">⚠️纠错</button></span></div>
    <div class="progress"><i style="width:${pct}%"></i></div>
    <div class="stem">${esc(q.stem)}</div>`;
  if(q.material) h+=`<div class="material">${esc(q.material)}</div>`;
  if(isEssay()){
    h+=`<textarea id="essay_${q.id}" placeholder="在此作答（主观题，写完点「查看参考答案」对比）">${esc(ANSWERS[q.id]||'')}</textarea>
      <button class="btn ghost" id="essaybtn_${q.id}" onclick="submitEssay('${q.id}')">${RESULTS[q.id]?'重新查看参考答案':'查看参考答案'}</button>`;
    if(RESULTS[q.id]) h+=`<div class="feedback ok"><div class="fhead">参考答案</div><div class="fans">${esc(RESULTS[q.id].reference||'（无）')}</div></div>`;
  } else {
    const multi=q.type==='multi';
    h+=`<div class="opts">${q.options.map((o,i)=>optHTML(q,o,i)).join('')}</div>`;
    if(!isMock()&&multi) h+=`<button class="btn" id="multibtn_${q.id}" onclick="submitMulti('${q.id}')" style="${ANSWERS[q.id]? '':'display:none'}">确定</button>`;
    if(!isMock()&&RESULTS[q.id]) h+=feedbackHTML(q);
  }
  h+=`</div>`;
  $('#quiz').innerHTML=h;
  bindOpts(q);
  bindEssay(q);
  renderFoot(no);
  renderPCside();
  window.scrollTo(0,0);
}
function optHTML(q,o,i){
  const key=o[0];
  const res=RESULTS[q.id];
  let cls='';
  if(res&&!isEssay()){
    const right=res.right||'';
    if(right.includes(key)) cls='ok';
    else if((ANSWERS[q.id]||'').includes(key)) cls='bad';
    else cls='lock';
  } else {
    if((ANSWERS[q.id]||'').includes(key)) cls='sel';
  }
  return `<label class="${cls}" data-q="${q.id}" data-k="${key}"><span class="letter">${key}</span><span>${esc(o.slice(2))}</span></label>`;
}
function feedbackHTML(q){
  const r=RESULTS[q.id];
  const head=r.correct?'✓ 正确':'✗ 错误';
  return `<div class="feedback ${r.correct?'ok':'bad'}"><div class="fhead">${head}</div>
    <div class="fans">你的答案：${esc(r.your||'（未答）')}　正确答案：${esc(r.right)}</div>
    ${r.explain?`<div>${esc(r.explain)}</div>`:''}</div>`;
}
function bindOpts(q){
  if(isEssay()||!q.options) return;
  $$('.opts label').forEach(lb=>{
    const key=lb.dataset.k;
    if(RESULTS[q.id]&&!isMock()){ lb.classList.add('lock'); return; }
    lb.onclick=()=>{
      if(isMock()){
        if(q.type==='multi'){
          let arr=ANSWERS[q.id]||[];
          arr=arr.includes(key)?arr.filter(x=>x!==key):[...arr,key];
          ANSWERS[q.id]=arr;
          lb.classList.toggle('sel',arr.includes(key));
        } else {
          ANSWERS[q.id]=key;
          $$(`.opts label[data-q="${q.id}"]`).forEach(x=>x.classList.remove('sel'));
          lb.classList.add('sel');
        }
      } else if(q.type==='multi'){
        let arr=ANSWERS[q.id]||[];
        arr=arr.includes(key)?arr.filter(x=>x!==key):[...arr,key];
        ANSWERS[q.id]=arr;
        lb.classList.toggle('sel',arr.includes(key));
        const mb=$('#multibtn_'+q.id); if(mb) mb.style.display='';
      } else {
        ANSWERS[q.id]=key;
        submitOne(q.id,key);
      }
    };
  });
}
function bindEssay(q){
  if(!isEssay()) return;
  const ta=$('#essay_'+q.id);
  if(ta) ta.oninput=()=>{ ANSWERS[q.id]=ta.value; };
}
function renderFoot(no){
  const first=no<=1, last=no>=QS.length;
  const done=Object.keys(ANSWERS).length;
  let h=`<button ${first?'disabled':''} onclick="go(-1)">上一题</button>
    <button onclick="showSheet()">答题卡<br><small style="font-size:10px">${done}/${QS.length}</small></button>
    <button class="main" onclick="submitAll()">${isMock()?'交卷':'完成'}</button>
    <button ${last?'disabled':''} onclick="go(1)">下一题</button>`;
  $('#foot').innerHTML=h;
}
function go(d){ IDX=Math.max(0,Math.min(QS.length-1,IDX+d)); render(); }
function todayStr(){ const d=new Date(); return d.getFullYear()+'-'+String(d.getMonth()+1).padStart(2,'0')+'-'+String(d.getDate()).padStart(2,'0'); }
async function apiCheck(payload){
  payload.date=todayStr();
  const r=await api('/api/check',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(payload)});
  return r.json();
}
async function submitOne(qid,val){
  const d=await apiCheck({answers:{[qid]:val}});
  if(d.results&&d.results[0]){ RESULTS[qid]=d.results[0]; markGrade(qid,d.results[0].correct,CUR); }
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
async function submitAll(){
  if(!QS.length) return;
  const payload={};
  QS.forEach(q=>{ if(ANSWERS[q.id]!==undefined) payload[q.id]=ANSWERS[q.id]; });
  const answered=Object.keys(payload).length;
  if(!answered){ toast('还没有作答'); return; }
  if(isMock()&&answered<QS.length){
    if(!confirm(`还有 ${QS.length-answered} 题未答，确定交卷？`)) return;
  }
  const d=await apiCheck({answers:payload});
  GRADED=true;
  d.results.forEach(r=>{ RESULTS[r.id]=r; if(r.correct!==undefined) markGrade(r.id,r.correct,CUR); });
  showScorecard(d);
}
function showScorecard(d){
  const obj=d.objective_total||0, oc=d.objective_correct||0;
  $('#quiz').innerHTML=`<div class="qcard scorecard">
    <div class="big">${d.score}分</div>
    <div class="sub">客观题 ${oc}/${obj} 正确 · 共 ${d.total} 题</div>
    <button class="btn" onclick="showSheet()">查看答题卡</button>
    <button class="btn ghost" onclick="render()">返回题目</button>
  </div>`;
  $('#foot').innerHTML=`<button class="main" onclick="render()">继续看题</button><button onclick="loadQuiz()">重做本题库</button>`;
  if($('#statpanel').classList.contains('open')) renderStats();
}
function showSheet(){
  const g=$('#sheetgrid'); g.innerHTML='';
  QS.forEach((q,i)=>{
    const r=RESULTS[q.id];
    const cls=r?(r.correct?'ok':'bad'):(ANSWERS[q.id]!==undefined?'done':'');
    const b=document.createElement('button');
    b.className=cls+(i===IDX?' cur':'');
    b.textContent=i+1;
    b.onclick=()=>{ IDX=i; closeSheet(); render(); };
    g.appendChild(b);
  });
  const answered=Object.keys(ANSWERS).length;
  $('#sheetcnt').textContent=`已答 ${answered}/${QS.length}`;
  $('#answerSheet').classList.add('open');
}
function closeSheet(){ $('#answerSheet').classList.remove('open'); }
async function showStats(){
  $('#statpanel').classList.add('open');
  await renderStats();
}
function closeStats(){ $('#statpanel').classList.remove('open'); }
async function renderStats(){
  const body=$('#statbody');
  const d=await (await api('/api/stats')).json();
  const dl=await (await api('/api/daily-stats')).json();
  const wb=wrongBank(), s=srs();
  const wbN=Object.keys(wb).length;
  const dueN=Object.keys(s).filter(isDue).length;
  // 每日统计块（今天 + 近7天）
  let dailyHtml='<div class="statadvice" style="margin-top:10px;text-align:left">📅 每日刷题</div>';
  if(!dl.days||!dl.days.length){
    dailyHtml+='<div class="hint" style="font-size:12px;padding:4px 0">还没有每日记录。做题后这里按天统计。</div>';
  } else {
    const last7=dl.days.slice(0,7);
    dailyHtml+=last7.map(x=>{
      const pct=x.pct||0;
      return `<div class="dailyrow"><span class="dd">${x.date===(dl.today&&dl.today.date)?'今天':(x.date||'')}</span>
        <span class="bar"><i class="${pct<60?'weak':''}" style="width:${Math.min(pct,100)}%"></i></span>
        <span class="pct">${pct}%</span><span class="d">${x.c}/${x.t}</span></div>`;
    }).join('');
    if(dl.days.length>7) dailyHtml+=`<div class="hint" style="font-size:12px;padding:4px 0">…共 ${dl.days.length} 天记录</div>`;
  }
  if(!d.modules||!d.modules.length){ body.innerHTML=`<h3>📊 成绩分析</h3><p class="hint">还没有成绩记录。做题后这里按模块统计正确率，定位薄弱项。</p><div class="statadvice">错题本 ${wbN} 题 · 待复习 ${dueN} 题</div>${dailyHtml}`; return; }
  const nm={}; SUBJECTS.forEach(x=>nm[x.id]=x.name);
  const rows=d.modules.map(m=>{
    const weak=m.pct<60?'weak':'';
    return `<div class="statrow"><span class="mod">${esc(nm[m.subject]||m.subject)}·${esc(m.module)}</span>
      <span class="bar"><i class="${weak}" style="width:${m.pct}%"></i></span>
      <span class="pct">${m.pct}%</span><span class="d">${m.correct}/${m.total}</span></div>`;
  }).join('');
  const weakTxt=d.weak.map(w=>`${nm[w.subject]||w.subject}·${w.module}（${w.pct}%）`).join('、');
  const adv=d.weak.length?`⚡ 最薄弱：${esc(weakTxt)}——用「薄弱优先」先补强。`:`✅ 整体不错，可进阶套题/限时。`;
  body.innerHTML=`<h3>📊 成绩分析 <small style="color:#8a94a6;font-weight:400">共答 ${d.total} · 对 ${d.correct}</small></h3>
    <div class="statoverall">${d.overall}%</div><div>${rows}</div>
    <div class="statadvice">${adv}</div>
    <div class="statadvice">📕 错题本 ${wbN} 题 · 🔁 待复习 ${dueN} 题 <button class="btn ghost" style="margin-top:6px" onclick="closeStats();MODE='wrong';loadQuiz()">去刷错题</button> <button class="btn ghost" style="margin-top:6px" onclick="closeStats();MODE='srs';loadQuiz()">去复习</button></div>
    ${dailyHtml}`;
}
function toast(msg){ const t=$('#toast'); t.textContent=msg; t.classList.add('show'); setTimeout(()=>t.classList.remove('show'),1600); }
function esc(s){ return String(s||'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])); }

// ---- 每日学习报告（网页查看）----
function closeReportView(){ $('#reportviewmask').classList.remove('open'); }
async function showReport(date){
  $('#reportviewmask').classList.add('open');
  const body=$('#reportviewbody');
  body.innerHTML='<div class="rv-empty">加载中…</div>';
  let dates=[];
  try{ const r=await api('/api/report-dates'); if(r.status===200){ dates=(await r.json()).dates||[]; } }catch(e){}
  let d=date||dates[0]||'';
  if(!d){
    body.innerHTML='<div class="reportview"><h2>📋 学习报告</h2><div class="rv-empty">还没有报告。每晚 21:00 自动生成，完成练习后次日可查看。</div><div class="rv-nav"><button class="btn" onclick="closeReportView()">关闭</button></div></div>';
    return;
  }
  try{
    const r=await api('/api/report-view?date='+encodeURIComponent(d));
    if(r.status===200){ const txt=await r.text(); body.innerHTML='<div class="reportview"><h2>📋 学习报告</h2><div class="rv-date">'+esc(d)+'</div><div class="rv-md">'+renderMd(txt)+'</div>'+reportNav(dates,d)+'</div>'; }
    else { const e=await r.json().catch(()=>({})); body.innerHTML='<div class="reportview"><h2>📋 学习报告</h2><div class="rv-empty">'+(e.err||'该日暂无报告')+'</div>'+reportNav(dates,d)+'</div>'; }
  }catch(e){ body.innerHTML='<div class="reportview"><h2>📋 学习报告</h2><div class="rv-empty">加载失败</div></div>'; }
}
function reportNav(dates,cur){
  if(!dates.length) return '<div class="rv-nav"><button class="btn" onclick="closeReportView()">关闭</button></div>';
  return '<div class="rv-nav">'+dates.map(x=>`<button class="btn ${x===cur?'ghost':''}" onclick="showReport('${x}')">${x}</button>`).join('')+'<button class="btn" onclick="closeReportView()">关闭</button></div>';
}
// 简易 markdown 渲染（标题/表格/列表/粗体）
function renderMd(t){
  let lines=t.split('\n'); let out=[], inTable=false, inList=false;
  for(const raw of lines){
    const line=raw.replace(/\r$/,'');
    if(/^\|/.test(line)){ if(!inTable){ out.push('<table>'); inTable=true; } 
      const cells=line.trim().slice(1,-1).split('|').map(c=>c.trim());
      if(!cells.some(c=>/^-+$/.test(c))) out.push('<tr>'+cells.map((c,i)=>`<${i===0&&!out.join('').match(/<tr>/)?'th':'td'}>${c}</${i===0&&!out.join('').match(/<tr>/)?'th':'td'}>`).join('')+'</tr>');
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

initPC();
bootAuth();
