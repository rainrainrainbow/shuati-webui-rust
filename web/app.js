// 国考刷题 — 粉笔式一题一卡 UI
// 练习模式：点选即判（即时对错+解析）；模考（金管局真题 zt20xx）：交卷统一判分
let SUBJECTS=[], CUR='', QS=[], IDX=0, ANSWERS={}, RESULTS={}, GRADED=false;
const MOCK_IDS=['zt2019','zt2020','zt2021','zt2022','zt2023'];
const $=s=>document.querySelector(s);
const $$=s=>document.querySelectorAll(s);
const isMock=()=>MOCK_IDS.includes(CUR);
const isEssay=()=>QS.length>0 && QS[0].type==='essay';

async function loadSubjects(){
  const d=await (await fetch('/api/subjects')).json();
  SUBJECTS=d.subjects;
  const box=$('#subjects'); box.innerHTML='';
  SUBJECTS.forEach(s=>{
    const b=document.createElement('button');
    b.innerHTML=`${s.name}<small>${s.count}</small>`;
    b.onclick=()=>{ if(CUR!==s.id){CUR=s.id; loadQuiz();} };
    box.appendChild(b);
  });
  CUR=SUBJECTS[0].id; loadQuiz();
}
async function loadQuiz(){
  GRADED=false; ANSWERS={}; RESULTS={}; IDX=0;
  const d=await (await fetch('/api/questions?subject='+CUR)).json();
  QS=d.questions;
  const sub=SUBJECTS.find(s=>s.id===CUR);
  $('#subname').textContent=sub?sub.name+' · '+QS.length+'题':'';
  $$('.subjects button').forEach((b,i)=>{b.classList.toggle('active', SUBJECTS[i]&&SUBJECTS[i].id===CUR);});
  render();
}
function render(){
  if(!QS.length){ $('#quiz').innerHTML='<p class="hint">该科目暂无题目</p>'; return; }
  const q=QS[IDX], no=IDX+1;
  const pct=Math.round((IDX)/(QS.length)*100);
  let h=`<div class="qcard">
    <div class="qmod"><span>${esc(q.module)}${q.type==='multi'?'（多选）':''}</span><span class="no">第${no}/${QS.length}题</span></div>
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
async function apiCheck(payload){
  const r=await fetch('/api/check',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(payload)});
  return r.json();
}
async function submitOne(qid,val){
  const d=await apiCheck({answers:{[qid]:val}});
  if(d.results&&d.results[0]) RESULTS[qid]=d.results[0];
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
  d.results.forEach(r=>{ RESULTS[r.id]=r; });
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
  const d=await (await fetch('/api/stats')).json();
  if(!d.modules||!d.modules.length){ body.innerHTML='<h3>📊 成绩分析</h3><p class="hint">还没有成绩记录。做题后这里按模块统计正确率，定位薄弱项。</p>'; return; }
  const nm={}; SUBJECTS.forEach(s=>nm[s.id]=s.name);
  const rows=d.modules.map(m=>{
    const weak=m.pct<60?'weak':'';
    return `<div class="statrow"><span class="mod">${esc(nm[m.subject]||m.subject)}·${esc(m.module)}</span>
      <span class="bar"><i class="${weak}" style="width:${m.pct}%"></i></span>
      <span class="pct">${m.pct}%</span><span class="d">${m.correct}/${m.total}</span></div>`;
  }).join('');
  const weakTxt=d.weak.map(w=>`${nm[w.subject]||w.subject}·${w.module}（${w.pct}%）`).join('、');
  const adv=d.weak.length?`⚡ 最薄弱：${esc(weakTxt)}——建议先补强这几个模块再做套题。`:`✅ 整体不错，可进阶套题/限时。`;
  body.innerHTML=`<h3>📊 成绩分析 <small style="color:#8a94a6;font-weight:400">共答 ${d.total} · 对 ${d.correct}</small></h3>
    <div class="statoverall">${d.overall}%</div><div>${rows}</div><div class="statadvice">${adv}</div>`;
}
function toast(msg){ const t=$('#toast'); t.textContent=msg; t.classList.add('show'); setTimeout(()=>t.classList.remove('show'),1600); }
function esc(s){ return String(s||'').replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c])); }
loadSubjects();
