let SUBJECTS = [];
let CUR = 'xingce';
let QS = [];
let ANSWERS = {};   // qid -> "B" (single) | ["A","C"] (multi) | essay text
let GRADED = false;

const $ = (s) => document.querySelector(s);
const $$ = (s) => document.querySelectorAll(s);

async function loadSubjects() {
  const r = await fetch('/api/subjects');
  const d = await r.json();
  SUBJECTS = d.subjects;
  const box = $('#subjects');
  box.innerHTML = '';
  SUBJECTS.forEach(s => {
    const b = document.createElement('button');
    b.innerHTML = `${s.name}<small>${s.count} 题</small>`;
    b.onclick = () => { CUR = s.id; loadQuiz(); };
    box.appendChild(b);
  });
  loadQuiz();
}

async function loadQuiz() {
  GRADED = false; ANSWERS = {};
  const r = await fetch('/api/questions?subject=' + CUR);
  const d = await r.json();
  QS = d.questions;
  renderTabs();
  render();
}

function renderTabs() {
  document.querySelectorAll('.subjects button').forEach((b, i) => {
    b.classList.toggle('active', SUBJECTS[i] && SUBJECTS[i].id === CUR);
  });
}

function render() {
  const q = $('#quiz');
  const essay = CUR === 'shenlun';
  let h = `<div class="toolbar"><span class="cnt">共 ${QS.length} 题 · 全部作答后交卷</span>
    <button class="btn ghost" onclick="resetAll()">清空</button></div>`;
  QS.forEach((item, idx) => {
    const no = idx + 1;
    if (item.type === 'essay') {
      h += `<div class="qcard" id="q${item.id}">
        <span class="qmod">${item.module} · 第${no}题</span>
        ${item.material ? `<div class="material">${esc(item.material)}</div>` : ''}
        <div class="stem">${esc(item.stem)}</div>
        <textarea id="ta_${item.id}" placeholder="在此作答（主观题，交卷后看参考要点）"></textarea>
        ${GRADED ? `<div class="ref">${esc(item.reference || '（无参考）')}</div>` : ''}
      </div>`;
    } else {
      const multi = item.type === 'multi';
      h += `<div class="qcard" id="q${item.id}">
        <span class="qmod">${item.module} · 第${no}题${multi ? '（多选）' : ''}</span>
        <div class="stem">${esc(item.stem)}</div>
        <div class="opts">${item.options.map((o, oi) => {
          const key = o[0];
          return `<label data-q="${item.id}" data-k="${key}">
            <input type="${multi ? 'checkbox' : 'radio'}" name="${item.id}" value="${key}">
            <span>${esc(o)}</span></label>`;
        }).join('')}</div>
        ${GRADED ? `<div class="mark" id="mk${item.id}"></div>
          <div class="explain" id="ex${item.id}"></div>` : ''}
      </div>`;
    }
  });
  h += `<div class="actions"><button class="btn" id="submitBtn" onclick="submit()">交卷判分</button></div>`;
  q.innerHTML = h;

  // 绑定事件
  QS.forEach(item => {
    if (item.type === 'essay') {
      const ta = $('#ta_' + item.id);
      ta.oninput = () => { ANSWERS[item.id] = ta.value; };
      if (ANSWERS[item.id]) ta.value = ANSWERS[item.id];
    } else {
      const multi = item.type === 'multi';
      document.querySelectorAll(`.opts label[data-q="${item.id}"]`).forEach(lb => {
        const inp = lb.querySelector('input');
        const key = lb.dataset.k;
        if (multi) {
          inp.onchange = () => {
            if (!ANSWERS[item.id]) ANSWERS[item.id] = [];
            const arr = ANSWERS[item.id];
            const i = arr.indexOf(key);
            if (inp.checked && i < 0) arr.push(key);
            if (!inp.checked && i >= 0) arr.splice(i, 1);
            lb.classList.toggle('sel', inp.checked);
          };
        } else {
          inp.onchange = () => {
            ANSWERS[item.id] = key;
            document.querySelectorAll(`.opts label[data-q="${item.id}"]`).forEach(x => x.classList.remove('sel'));
            lb.classList.add('sel');
          };
        }
        // 恢复
        const cur = ANSWERS[item.id];
        const checked = multi ? (cur && cur.includes(key)) : (cur === key);
        if (checked) { inp.checked = true; lb.classList.add('sel'); }
      });
    }
  });
}

function esc(s) {
  return String(s || '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
}

function resetAll() {
  ANSWERS = {}; GRADED = false; render();
}

async function submit() {
  // 收集 textarea
  QS.forEach(item => {
    if (item.type === 'essay') {
      const ta = $('#ta_' + item.id);
      if (ta) ANSWERS[item.id] = ta.value;
    }
  });
  const payload = {};
  Object.keys(ANSWERS).forEach(k => {
    const v = ANSWERS[k];
    payload[k] = Array.isArray(v) ? v.join('') : v;
  });
  const r = await fetch('/api/check', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ answers: payload }),
  });
  const d = await r.json();
  GRADED = true;
  // 分数条
  const sc = document.createElement('div');
  sc.className = 'score';
  sc.innerHTML = `<div class="n">${d.score} 分</div>
    <div class="d">客观题 ${d.objective_correct}/${d.objective_total} 正确 · 共 ${d.total} 题</div>`;
  const q = $('#quiz');
  q.insertBefore(sc, q.querySelector('.toolbar'));

  d.results.forEach(res => {
    const card = $('#q' + res.id);
    if (!card) return;
    if (res.type === 'essay') {
      card.querySelector('.ref').innerHTML = esc(res.reference || '（无参考）');
    } else {
      const mk = $('#mk' + res.id);
      if (mk) {
        mk.className = 'mark ' + (res.correct ? 'ok' : 'bad');
        mk.textContent = res.correct ? '✓ 正确' : '✗ 错误';
        mk.textContent += `　你的答案：${res.your || '（未答）'}　正确答案：${res.right}`;
      }
      const ex = $('#ex' + res.id);
      if (ex) ex.textContent = '解析：' + res.explain;
      card.style.borderColor = res.correct ? 'var(--ok)' : 'var(--bad)';
    }
  });
  // 滚动到分数
  sc.scrollIntoView({ behavior: 'smooth' });
}

loadSubjects();
