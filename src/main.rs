// shuati-webui-rust — 国考刷题网页（行测/申论/计算机岗）
// tiny_http 单二进制，内嵌前端与题库；服务端判分，答案不下发。
// v0.2 多用户：账号密码登录（bcrypt）、每用户独立数据目录、每晚 21:00 学习报告（AI 生成，网页查看）。
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tiny_http::{Header, Response, Server};

const QUIZ_JSON: &str = include_str!("../web/questions.json");
const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLE_CSS: &str = include_str!("../web/style.css");

// ---- 安全加固常量（H3/H4/M3/M4/M7）----
// 会话 TTL：7 天，过期惰性清理
const SESSION_TTL: u64 = 7 * 24 * 3600;
// 请求体上限：2 MiB（防超大 body / 内存耗尽）
const MAX_BODY: usize = 2 * 1024 * 1024;
// 登录失败限流：连续失败超过阈值后指数退避
const LOGIN_MAX_FAILS: u32 = 5;
// 全局数据文件写锁：包住所有用户数据文件的 load-modify-save（H4）
static DATA_LOCK: Mutex<()> = Mutex::new(());
// 登录失败记录：key "ip|username" -> (fail_count, last_fail_ts)
static LOGIN_FAILS: OnceLock<Mutex<HashMap<String, (u32, u64)>>> = OnceLock::new();

fn login_fails() -> &'static Mutex<HashMap<String, (u32, u64)>> {
    LOGIN_FAILS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Clone, Serialize)]
struct Subject {
    id: String,
    name: String,
    modules: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Q {
    id: String,
    subject: String,
    module: String,
    #[serde(rename = "type")]
    qtype: String,
    stem: String,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    answer: String,
    #[serde(default)]
    explain: String,
    #[serde(default)]
    material: String,
    #[serde(default)]
    reference: String,
    #[serde(default)]
    difficulty: u8,
    #[serde(default)]
    year: String,
}

struct Bank {
    subjects: Vec<Subject>,
    questions: Vec<Q>,
    by_id: HashMap<String, Q>,
}

impl Bank {
    fn load(quiz_path: &Option<PathBuf>) -> Bank {
        let json_str = read_quiz_json(quiz_path);
        let raw: Value = serde_json::from_str(&json_str).expect("questions.json 解析失败");
        let subjects = raw["subjects"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| Subject {
                        id: s["id"].as_str().unwrap_or("").to_string(),
                        name: s["name"].as_str().unwrap_or("").to_string(),
                        modules: s["modules"]
                            .as_array()
                            .map(|m| {
                                m.iter()
                                    .map(|x| x.as_str().unwrap_or("").to_string())
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut questions = Vec::new();
        for (subj, arr) in raw["questions"].as_object().unwrap() {
            for q in arr.as_array().unwrap() {
                questions.push(Q {
                    id: q["id"].as_str().unwrap_or("").to_string(),
                    subject: subj.clone(),
                    module: q["module"].as_str().unwrap_or("").to_string(),
                    qtype: q["type"].as_str().unwrap_or("single").to_string(),
                    stem: q["stem"].as_str().unwrap_or("").to_string(),
                    options: q["options"]
                        .as_array()
                        .map(|o| {
                            o.iter()
                                .map(|x| x.as_str().unwrap_or("").to_string())
                                .collect()
                        })
                        .unwrap_or_default(),
                    answer: q["answer"].as_str().unwrap_or("").to_string(),
                    explain: q["explain"].as_str().unwrap_or("").to_string(),
                    material: q["material"].as_str().unwrap_or("").to_string(),
                    reference: q["reference"].as_str().unwrap_or("").to_string(),
                    difficulty: q["difficulty"].as_u64().unwrap_or(0) as u8,
                    year: q["year"].as_str().unwrap_or("").to_string(),
                });
            }
        }
        let by_id = questions
            .iter()
            .map(|q| (q.id.clone(), q.clone()))
            .collect();
        Bank { subjects, questions, by_id }
    }

    // 供前端练习用：剥掉 answer/explain/reference（服务端判分，不泄题；M6 一律剔除 reference）
    fn public_questions(&self, subject: &str) -> Vec<Value> {
        self.questions
            .iter()
            .filter(|q| q.subject == subject)
            .map(|q| {
                json!({
                    "id": q.id, "module": q.module, "type": q.qtype,
                    "stem": q.stem, "options": q.options, "material": q.material,
                    "reference": Value::Null,
                    "difficulty": q.difficulty,
                })
            })
            .collect()
    }

    // 真题聚合：跨全部真题科目，按 module/year/juan（卷型）过滤，附带 subject+year 供前端分组
    fn zhenti_questions(&self, module: &str, year: &str, juan: &str) -> Vec<Value> {
        self.questions
            .iter()
            .filter(|q| is_zhenti_subject(&q.subject))
            .filter(|q| module.is_empty() || q.module == module)
            .filter(|q| juan.is_empty() || q.module == juan)
            .filter(|q| year.is_empty() || q_year(q) == year)
            .map(|q| {
                json!({
                    "id": q.id, "subject": q.subject, "module": q.module, "type": q.qtype,
                    "stem": q.stem, "options": q.options, "material": q.material,
                    "year": q_year(q), "reference": Value::Null, "difficulty": q.difficulty,
                })
            })
            .collect()
    }
}

// ---- 真题筛选辅助 ----
// 真题科目：金管局真题 zt2019-2023、广东省考行测 gd2016-2026、国考申论真题 slzt2016-2026fs/ds/zx、
// 广东省考申论 gdsl2016-2026、国考行测6模块(changshi/yanyu/shuliang/panduan/ziliao/zhengzhi)、计算机岗 jisuanji
fn is_zhenti_subject(id: &str) -> bool {
    id.starts_with("zt") || id.starts_with("gd") || id.starts_with("slzt")
        || matches!(id, "changshi" | "yanyu" | "shuliang" | "panduan" | "ziliao" | "zhengzhi" | "jisuanji")
}
// 从 subject id 提取年份（zt2019→2019、gd2016→2016、slzt2020fs→2020、gdsl2020→2020）
fn subject_year(id: &str) -> Option<String> {
    let b = id.as_bytes();
    if b.len() < 4 { return None; }
    for i in 0..=b.len() - 4 {
        if b[i..i + 4].iter().all(|c| c.is_ascii_digit()) {
            return Some(id[i..i + 4].to_string());
        }
    }
    None
}
// 题目年份：优先 subject id 内嵌年份，其次题目 year 字段，否则"综合"（国考行测6模块/计算机岗无年份）
fn q_year(q: &Q) -> String {
    if let Some(y) = subject_year(&q.subject) { return y; }
    if !q.year.is_empty() { return q.year.clone(); }
    "综合".to_string()
}
// 白名单校验：^[A-Za-z0-9_\-\u4e00-\u9fa5]+$（module/juan 参数）
fn valid_whitelist(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || ('\u{4e00}'..='\u{9fa5}').contains(&c))
}
// 年份校验：^\d{4}$
fn valid_year(s: &str) -> bool {
    s.len() == 4 && s.chars().all(|c| c.is_ascii_digit())
}
// URL 百分号解码（中文 module/juan 参数需解码，subject/status 为 ASCII 无需）
fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        if b[i] == b'+' { out.push(b' '); } else { out.push(b[i]); }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

// ---- 题库加载 ----
fn read_quiz_json(explicit: &Option<PathBuf>) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(p) = explicit { cands.push(p.clone()); }
    if let Ok(p) = std::env::var("QUIZ_JSON_PATH") { cands.push(PathBuf::from(p)); }
    cands.push(PathBuf::from("./questions.json"));
    cands.push(PathBuf::from(&home).join(".shuati-webui-rust/questions.json"));
    for p in &cands {
        if p.exists() {
            if let Ok(s) = std::fs::read_to_string(p) {
                eprintln!("题库从文件加载: {} ({} 字符)", p.display(), s.len());
                return s;
            }
        }
    }
    eprintln!("题库文件未找到，使用内嵌题库");
    QUIZ_JSON.to_string()
}

// ---- 用户账号 ----
#[derive(Clone, Serialize, Deserialize)]
struct User {
    username: String,
    hash: String,   // bcrypt 哈希（含盐）
    name: String,
    created: u64,
    #[serde(default)]
    admin: bool,    // 管理员（首用户注册时置 true）
}
type UsersMap = HashMap<String, User>;

fn data_root() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(&home).join(".shuati-webui-rust")
}
fn users_path() -> PathBuf { data_root().join("users.json") }

fn load_users() -> UsersMap {
    std::fs::read_to_string(users_path())
        .ok()
        .and_then(|s| serde_json::from_str::<UsersMap>(&s).ok())
        .unwrap_or_default()
}
fn save_users(m: &UsersMap) {
    if let Ok(s) = serde_json::to_string_pretty(m) {
        let _ = std::fs::write(users_path(), s);
    }
}
// H2：确保至少有一个管理员——若既存用户均非 admin，把最早创建者提为管理员（幂等）
fn ensure_admin() {
    let mut users = load_users();
    if users.is_empty() || users.values().any(|u| u.admin) { return; }
    if let Some((_oldest, u)) = users.iter_mut().min_by_key(|(_, u)| u.created) {
        u.admin = true;
    }
    save_users(&users);
}
fn user_dir(username: &str) -> PathBuf {
    let d = data_root().join("users").join(username);
    let _ = std::fs::create_dir_all(&d);
    d
}
fn user_file(username: &str, name: &str) -> PathBuf { user_dir(username).join(name) }

// 首用户注册时迁移旧的共享数据（保留当前进度）
fn migrate_legacy(username: &str) {
    let root = data_root();
    let ud = user_dir(username);
    for f in ["stats.json", "daily.json", "reports.json"] {
        let src = root.join(f);
        let dst = ud.join(f);
        if src.exists() && !dst.exists() {
            let _ = std::fs::copy(&src, &dst);
            eprintln!("迁移旧 {} → {}", f, dst.display());
        }
    }
}

// ---- 会话（内存）：token -> (username, expires_ts) ----
type Sessions = Arc<Mutex<HashMap<String, (String, u64)>>>;

fn new_token() -> String {
    // 从 /dev/urandom 读 24 字节 → hex
    let mut buf = [0u8; 24];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read as _;
        let _ = f.read_exact(&mut buf);
    } else {
        // 兜底：时间戳 + 计数器
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        buf[..16].copy_from_slice(&t.to_le_bytes());
        buf[16..].copy_from_slice(&[0xAB; 8]);
    }
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

// 惰性清理过期会话（M3）
fn cleanup_sessions(sessions: &Sessions) {
    let now = now_secs();
    let mut s = sessions.lock().unwrap();
    s.retain(|_, (_, exp)| *exp > now);
}
fn auth_user(request: &tiny_http::Request, sessions: &Sessions) -> Option<String> {
    // M3：仅接受 Authorization: Bearer ***（去 query token）
    let mut tok = String::new();
    for h in request.headers() {
        if h.field.equiv("Authorization") {
            let v = h.value.as_str().to_string();
            if let Some(rest) = v.strip_prefix("Bearer ") {
                tok = rest.trim().to_string();
            }
        }
    }
    if tok.is_empty() { return None; }
    cleanup_sessions(sessions);
    sessions.lock().unwrap().get(&tok).map(|(u, _)| u.clone())
}
fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
fn is_admin(users: &UsersMap, username: &str) -> bool {
    users.get(username).map(|u| u.admin).unwrap_or(false)
}

// ---- 输入白名单校验（H3）----
fn valid_username(u: &str) -> bool {
    let n = u.len();
    if n < 2 || n > 20 { return false; }
    u.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
fn valid_date(d: &str) -> bool {
    let b: Vec<char> = d.chars().collect();
    b.len() == 10
        && b.iter().all(|c| c.is_ascii_digit() || *c == '-')
        && b[4] == '-' && b[7] == '-'
}
fn valid_paper_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
// 校验 name 无路径分隔/..，且拼接后位于 base 下（H3 路径遍历防护）
fn safe_child(base: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    let p = base.join(name);
    let bc = std::fs::canonicalize(base).ok()?;
    let pc = std::fs::canonicalize(&p).ok().unwrap_or_else(|| p.clone());
    if pc.starts_with(&bc) { Some(p) } else { None }
}

// ---- 请求体读取：限长（M4）----
fn read_body_limit(request: &mut tiny_http::Request, max: usize) -> String {
    use std::io::Read as _;
    let mut buf = Vec::with_capacity(4096);
    if let Ok(_n) = request.as_reader().take(max as u64).read_to_end(&mut buf) {
        // 已限长
    }
    String::from_utf8_lossy(&buf).to_string()
}

// ---- 登录失败限流（M7）----
fn login_penalty(ip: &str, username: &str) -> u64 {
    let key = format!("{}|{}", ip, username);
    let m = login_fails().lock().unwrap();
    match m.get(&key) {
        Some((f, last)) if *f >= LOGIN_MAX_FAILS => {
            let delay = 1u64 << (*f - LOGIN_MAX_FAILS).min(4); // 1,2,4,8,16 秒
            let elapsed = now_secs().saturating_sub(*last);
            if elapsed >= delay { 0 } else { delay - elapsed }
        }
        _ => 0,
    }
}
fn note_login_fail(ip: &str, username: &str) {
    let key = format!("{}|{}", ip, username);
    let mut m = login_fails().lock().unwrap();
    let e = m.entry(key).or_insert((0, now_secs()));
    e.0 += 1; e.1 = now_secs();
}
fn reset_login_fail(ip: &str, username: &str) {
    login_fails().lock().unwrap().remove(&format!("{}|{}", ip, username));
}
fn client_ip(request: &tiny_http::Request) -> String {
    request.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default()
}

// ---- 判分公共函数（L1/M1/M6）----
// 归一化答案：去空白/逗号 + 大写
fn norm_ans(s: &str) -> Vec<char> {
    s.chars().filter(|c| !c.is_whitespace() && *c != ',').map(|c| c.to_ascii_uppercase()).collect()
}
// 集合比较：字符排序后相等（多选顺序无关）
fn ans_eq(right: &str, your: &str) -> bool {
    let mut a = norm_ans(right);
    let mut b = norm_ans(your);
    a.sort_unstable();
    b.sort_unstable();
    !b.is_empty() && a == b
}
type GradeOut = (usize, usize, usize, Vec<Value>); // total, obj_total, obj_correct, results
fn grade_answers(bank: &Bank, answers: &serde_json::Map<String, Value>) -> GradeOut {
    let mut total = 0usize;
    let mut obj_total = 0usize;
    let mut obj_correct = 0usize;
    let mut results = Vec::new();
    for (qid, ans_val) in answers {
        let q = match bank.by_id.get(qid) { Some(q) => q, None => continue };
        let ans = ans_val.as_str().unwrap_or("");
        if q.qtype == "essay" {
            total += 1;
            results.push(json!({"id": qid, "type": "essay", "correct": Value::Null,
                "your": ans, "right": Value::Null, "explain": q.explain, "reference": q.reference}));
        } else {
            obj_total += 1; total += 1;
            let right = q.answer.to_uppercase();
            let your: String = norm_ans(ans).into_iter().collect();
            let ok = ans_eq(&q.answer, &ans);
            if ok { obj_correct += 1; }
            results.push(json!({"id": qid, "type": q.qtype, "correct": ok,
                "your": your, "right": right, "explain": q.explain, "reference": Value::Null}));
        }
    }
    (total, obj_total, obj_correct, results)
}
// 累计统计 + 每日 + 已刷（H4：DATA_LOCK 包住 load-modify-save）
fn apply_grade_stats(username: &str, answers: &serde_json::Map<String, Value>, bank: &Bank, date: &str) {
    let _g = DATA_LOCK.lock().unwrap();
    let stats_path = user_file(username, "stats.json");
    let daily_path = user_file(username, "daily.json");
    {
        let mut m = load_stats(&stats_path);
        for (qid, ans_val) in answers {
            let q = match bank.by_id.get(qid) { Some(q) => q, None => continue };
            if q.qtype == "essay" { continue; }
            let ans = ans_val.as_str().unwrap_or("");
            let ok = ans_eq(&q.answer, &ans);
            let key = format!("{}|{}", q.subject, q.module);
            let e = m.entry(key).or_default(); e.t += 1; if ok { e.c += 1; }
        }
        save_stats(&stats_path, &m);
    }
    if !date.is_empty() && valid_date(date) {
        let mut dl = load_daily(&daily_path);
        let d = dl.entry(date.to_string()).or_default();
        for (qid, ans_val) in answers {
            let q = match bank.by_id.get(qid) { Some(q) => q, None => continue };
            if q.qtype == "essay" { continue; }
            let ans = ans_val.as_str().unwrap_or("");
            let ok = ans_eq(&q.answer, &ans);
            let key = format!("{}|{}", q.subject, q.module);
            d.t += 1; let e = d.by.entry(key).or_default(); e.t += 1;
            if ok { d.c += 1; e.c += 1; }
        }
        save_daily(&daily_path, &dl);
    }
    mark_done(username, answers, bank);
}

// ---- 统计（每用户独立，按 subject|module）----
#[derive(Default, Clone, Serialize, Deserialize)]
struct ModStat { t: usize, c: usize }
type StatsMap = HashMap<String, ModStat>;

fn load_stats(path: &std::path::Path) -> StatsMap {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<StatsMap>(&s).ok())
        .unwrap_or_default()
}
fn save_stats(path: &std::path::Path, m: &StatsMap) {
    if let Ok(s) = serde_json::to_string(m) {
        let _ = std::fs::write(path, s);
    }
}

// ---- 每日统计 ----
#[derive(Default, Clone, Serialize, Deserialize)]
struct DayStat { t: usize, c: usize, by: HashMap<String, ModStat> }
type DailyMap = HashMap<String, DayStat>;

fn load_daily(path: &std::path::Path) -> DailyMap {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<DailyMap>(&s).ok())
        .unwrap_or_default()
}
fn save_daily(path: &std::path::Path, m: &DailyMap) {
    if let Ok(s) = serde_json::to_string(m) {
        let _ = std::fs::write(path, s);
    }
}

// ---- 已刷题目（每用户 done.json：已作答客观题 qid 数组）----
fn load_done(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}
fn save_done(path: &std::path::Path, v: &[String]) {
    if let Ok(s) = serde_json::to_string(v) {
        let _ = std::fs::write(path, s);
    }
}
// 判分后把作答的客观题 qid 追加进 done.json（去重）
fn mark_done(username: &str, answers: &serde_json::Map<String, Value>, bank: &Bank) {
    let path = user_file(username, "done.json");
    let mut done = load_done(&path);
    let mut changed = false;
    for qid in answers.keys() {
        if let Some(q) = bank.by_id.get(qid) {
            if q.qtype == "essay" { continue; }
            if !done.contains(qid) { done.push(qid.clone()); changed = true; }
        }
    }
    if changed { save_done(&path, &done); }
}

// ---- 申论 AI 评分：提交/查询/评分 ----
// 每用户数据目录 users/<u>/shenlun/<submission_id>.json
// 记录：{id,qid,text,file,submitted_at,status:pending|graded,score?,feedback?,graded_at?}
fn shenlun_dir(username: &str) -> PathBuf {
    let d = user_dir(username).join("shenlun");
    let _ = std::fs::create_dir_all(&d);
    d
}
fn read_shenlun_submissions(username: &str) -> Vec<Value> {
    let dir = user_dir(username).join("shenlun");
    let mut list: Vec<Value> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.path().extension().map(|x| x == "json").unwrap_or(false) {
                if let Ok(s) = std::fs::read_to_string(e.path()) {
                    if let Ok(v) = serde_json::from_str::<Value>(&s) { list.push(v); }
                }
            }
        }
    }
    list.sort_by(|a, b| b["submitted_at"].as_u64().unwrap_or(0).cmp(&a["submitted_at"].as_u64().unwrap_or(0)));
    list
}
// 跨用户查找提交（供评分 cron 用）：返回 (username, 文件路径, 记录)
fn find_shenlun_global(sub_id: &str) -> Option<(String, PathBuf, Value)> {
    let users_dir = data_root().join("users");
    let rd = std::fs::read_dir(&users_dir).ok()?;
    for e in rd.flatten() {
        let u = e.file_name().to_string_lossy().to_string();
        let sp = e.path().join("shenlun").join(format!("{}.json", sub_id));
        if sp.exists() {
            if let Ok(s) = std::fs::read_to_string(&sp) {
                if let Ok(v) = serde_json::from_str::<Value>(&s) {
                    return Some((u, sp, v));
                }
            }
        }
    }
    None
}
// 列出所有 pending 提交（跨用户，供 AI 评分 cron）
fn list_shenlun_pending() -> Vec<Value> {
    let users_dir = data_root().join("users");
    let mut out: Vec<Value> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&users_dir) {
        for e in rd.flatten() {
            let u = e.file_name().to_string_lossy().to_string();
            let sl = e.path().join("shenlun");
            if let Ok(sd) = std::fs::read_dir(&sl) {
                for f in sd.flatten() {
                    if f.path().extension().map(|x| x == "json").unwrap_or(false) {
                        if let Ok(s) = std::fs::read_to_string(f.path()) {
                            if let Ok(v) = serde_json::from_str::<Value>(&s) {
                                if v["status"].as_str().unwrap_or("") == "pending" {
                                    let mut rec = v.clone();
                                    rec["username"] = json!(u);
                                    out.push(rec);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b["submitted_at"].as_u64().unwrap_or(0).cmp(&a["submitted_at"].as_u64().unwrap_or(0)));
    out
}

fn text_response(code: u16, content_type: &str, body: String) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(body.into_bytes())
        .with_status_code(code)
        .with_header(Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap())
}
fn json_response(code: u16, v: &Value) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    text_response(code, "application/json; charset=utf-8", v.to_string())
}

// ---- 随机辅助（xorshift64，无额外依赖）----
static RNG_STATE: AtomicU64 = AtomicU64::new(0x9E3779B97F4A7C15);
fn rng_next() -> u64 {
    let mut x = RNG_STATE.load(Ordering::Relaxed);
    if x == 0 {
        x = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15) | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RNG_STATE.store(x, Ordering::Relaxed);
    x
}
fn shuffle_take(pool: &mut Vec<Q>, n: usize) -> Vec<Q> {
    let len = pool.len();
    for i in (1..len).rev() {
        let j = (rng_next() % (i as u64 + 1)) as usize;
        pool.swap(i, j);
    }
    pool.truncate(n.min(len));
    pool.to_vec()
}
// 按 module 配比抽题：每个 module 抽 target 道（缺题少抽并记录 note）
fn draw_by_module(pool: &mut Vec<Q>, targets: &[(String, usize)]) -> (Vec<Q>, Vec<String>) {
    let mut picked = Vec::new();
    let mut notes = Vec::new();
    for (module, n) in targets {
        let mut sub: Vec<Q> = pool.iter()
            .filter(|q| &q.module == module && q.qtype != "essay" && !q.options.is_empty())
            .cloned().collect();
        let take = (*n).min(sub.len());
        if take < *n { notes.push(format!("{} 缺题（需{}，仅{}）", module, n, sub.len())); }
        picked.extend(shuffle_take(&mut sub, take));
    }
    (picked, notes)
}

// ---- 模考组卷：按实际 subject+module 结构配比抽题（H1）----
// 国考行测：按中文模块名配比（xingce 内部模块 + 专类科目池），20常识/40言语/15数量/40判断/20资料=135
fn build_guokao_xingce(bank: &Bank, count: usize) -> (Vec<Q>, Vec<String>) {
    // (专类科目池 id, 中文模块名, 目标题数)
    let base: [(&str, &str, f64); 5] = [
        ("changshi", "常识判断", 20.0),
        ("yanyu", "言语理解与表达", 40.0),
        ("shuliang", "数量关系", 15.0),
        ("panduan", "判断推理", 40.0),
        ("ziliao", "资料分析", 20.0),
    ];
    let scale = if count > 0 { count as f64 / 135.0 } else { 1.0 };
    let mut picked: Vec<Q> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for (pool_id, module, bn) in base {
        let n = (bn * scale).round().max(1.0) as usize;
        // 从专类科目池 或 xingce 的该中文模块 抽
        let mut sub: Vec<Q> = bank.questions.iter()
            .filter(|qq| {
                if qq.qtype == "essay" || qq.options.is_empty() { return false; }
                (&qq.subject == pool_id) || (&qq.subject == "xingce" && &qq.module == module)
            })
            .cloned().collect();
        let take = n.min(sub.len());
        if take < n { notes.push(format!("{} 缺题（需{}，仅{}）", module, n, sub.len())); }
        picked.extend(shuffle_take(&mut sub, take));
    }
    (picked, notes)
}
// 省考行测：按年份（gd20xx）等额，年份内按模块配比（省考真题结构）
fn build_shengkao_xingce(bank: &Bank, want: usize) -> (Vec<Q>, Vec<String>) {
    let mut years: Vec<String> = bank.questions.iter()
        .filter(|q| q.subject.starts_with("gd") && !q.subject.starts_with("gdsl"))
        .map(|q| q.subject.clone()).collect();
    years.sort(); years.dedup();
    if years.is_empty() { return (Vec::new(), vec!["题库不足，无法组卷".to_string()]); }
    let per = ((want as f64 / years.len() as f64).round().max(1.0)) as usize;
    let mut picked: Vec<Q> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for yr in &years {
        let mut pool: Vec<Q> = bank.questions.iter()
            .filter(|q| &q.subject == yr && q.qtype != "essay" && !q.options.is_empty())
            .cloned().collect();
        if pool.is_empty() { continue; }
        let mut modcnt: HashMap<String, usize> = HashMap::new();
        for q in &pool { *modcnt.entry(q.module.clone()).or_default() += 1; }
        let total = pool.len();
        let mut targets: Vec<(String, usize)> = Vec::new();
        for (m, c) in &modcnt {
            let t = ((*c as f64 * per as f64 / total as f64).round()) as usize;
            if t > 0 { targets.push((m.clone(), t)); }
        }
        let (p, mut n) = draw_by_module(&mut pool, &targets);
        picked.extend(p); notes.append(&mut n);
    }
    if picked.len() < want {
        let full: Vec<Q> = bank.questions.iter()
            .filter(|q| q.subject.starts_with("gd") && !q.subject.starts_with("gdsl")
                && q.qtype != "essay" && !q.options.is_empty())
            .cloned().collect();
        fill_to(want, &mut picked, &full);
    }
    if picked.is_empty() { notes.push("题库不足，无法组卷".to_string()); }
    (picked, notes)
}
fn build_subject_paper(bank: &Bank, subj_ids: &[&str], want: usize) -> (Vec<Q>, Vec<String>) {
    let mut picked: Vec<Q> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    if subj_ids.is_empty() { notes.push("未指定科目".to_string()); return (picked, notes); }
    let per = ((want as f64 / subj_ids.len() as f64).round().max(1.0)) as usize;
    for sid in subj_ids {
        let mut pool: Vec<Q> = bank.questions.iter()
            .filter(|qq| &qq.subject == sid && qq.qtype != "essay" && !qq.options.is_empty())
            .cloned().collect();
        if pool.is_empty() { continue; }
        let mut modcnt: HashMap<String, usize> = HashMap::new();
        for q in &pool { *modcnt.entry(q.module.clone()).or_default() += 1; }
        let total = pool.len();
        let mut targets: Vec<(String, usize)> = Vec::new();
        for (m, c) in &modcnt {
            let t = ((*c as f64 * per as f64 / total as f64).round()) as usize;
            if t > 0 { targets.push((m.clone(), t)); }
        }
        let (p, mut n) = draw_by_module(&mut pool, &targets);
        picked.extend(p); notes.append(&mut n);
    }
    if picked.len() < want {
        let full: Vec<Q> = bank.questions.iter()
            .filter(|qq| subj_ids.contains(&qq.subject.as_str()) && qq.qtype != "essay" && !qq.options.is_empty())
            .cloned().collect();
        fill_to(want, &mut picked, &full);
    }
    if picked.is_empty() { notes.push("题库不足，无法组卷".to_string()); }
    (picked, notes)
}
// 补足到 want：从剩余池随机补（按 id 去重，L3 配比精确）
fn fill_to(want: usize, picked: &mut Vec<Q>, pool: &[Q]) {
    let have: std::collections::HashSet<String> = picked.iter().map(|q| q.id.clone()).collect();
    let mut extra: Vec<Q> = pool.iter().filter(|q| !have.contains(&q.id)).cloned().collect();
    let need = want.saturating_sub(picked.len());
    if need > 0 { picked.extend(shuffle_take(&mut extra, need)); }
}
// 申论（essay 题）抽题：slzt* + slgongwen/sldanyi/slwenzhang/slzonghe
fn build_shenlun(bank: &Bank, want: usize) -> (Vec<Q>, Vec<String>) {
    let mut pool: Vec<Q> = bank.questions.iter()
        .filter(|qq| qq.qtype == "essay"
            && (qq.subject.starts_with("slzt") || qq.subject.starts_with("sl") || qq.subject == "shenlun"))
        .cloned().collect();
    if pool.is_empty() { return (Vec::new(), vec!["暂无申论题".to_string()]); }
    (shuffle_take(&mut pool, want), Vec::new())
}

fn main() {
    let mut port: u16 = 8085;
    let mut bind: String = "127.0.0.1".to_string(); // M2：默认仅本机；部署 systemd 显式传 --bind 0.0.0.0
    let mut quiz_path: Option<PathBuf> = None;
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" if i + 1 < args.len() => { port = args[i + 1].parse().unwrap_or(8085); i += 2; }
            "--bind" if i + 1 < args.len() => { bind = args[i + 1].clone(); i += 2; }
            "--questions" if i + 1 < args.len() => { quiz_path = Some(PathBuf::from(&args[i + 1])); i += 2; }
            "-h" | "--help" => {
                println!("shuati-webui-rust — 国考刷题网页 (行测/申论/计算机岗) 多用户版\n --port <N>  --bind <IP>  --questions <F>");
                return;
            }
            _ => i += 1,
        }
    }

    let _ = std::fs::create_dir_all(data_root());
    let _ = std::fs::create_dir_all(data_root().join("users"));
    ensure_admin(); // H2：确保至少一个管理员（幂等迁移）
    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
    let bank = Arc::new(Bank::load(&quiz_path));
    let server = Server::http((bind.as_str(), port)).expect("绑定端口失败");
    println!("shuati-webui 多用户版已启动 http://{bind}:{port}  (题目 {} 道)", bank.questions.len());

    for request in server.incoming_requests() {
        let bank = bank.clone();
        let sessions = sessions.clone();
        std::thread::spawn(move || {
            let _ = handle(request, &bank, &sessions);
        });
    }
}

fn handle(mut request: tiny_http::Request, bank: &Bank, sessions: &Sessions) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("/").to_string();

    // 静态资源 + 公开接口
    match (method.as_str(), path.as_str()) {
        (_, "/") => { let _ = request.respond(text_response(200, "text/html; charset=utf-8", INDEX_HTML.to_string())); return; }
        (_, "/app.js") => { let _ = request.respond(text_response(200, "application/javascript; charset=utf-8", APP_JS.to_string())); return; }
        (_, "/style.css") => { let _ = request.respond(text_response(200, "text/css; charset=utf-8", STYLE_CSS.to_string())); return; }
        ("GET", "/api/subjects") => {
            if auth_user(&request, sessions).is_none() {
                let _ = request.respond(text_response(401, "text/plain; charset=utf-8", "unauthorized".to_string()));
                return;
            }
            let subs: Vec<Value> = bank.subjects.iter().map(|s| json!({
                "id": s.id, "name": s.name, "modules": s.modules,
                "count": bank.questions.iter().filter(|q| q.subject == s.id).count(),
            })).collect();
            let _ = request.respond(json_response(200, &json!({"subjects": subs})));
            return;
        }
        ("GET", "/api/questions") => {
            // L4：鉴权一次
            let user = match auth_user(&request, sessions) {
                Some(u) => u,
                None => {
                    let _ = request.respond(text_response(401, "text/plain; charset=utf-8", "unauthorized".to_string()));
                    return;
                }
            };
            let mut subject = "xingce".to_string();
            let mut status = "all".to_string();
            let mut module = String::new();
            let mut year = String::new();
            let mut juan = String::new();
            for kv in request.url().split('?').nth(1).unwrap_or("").split('&') {
                let mut it = kv.split('=');
                let k = it.next().unwrap_or(""); let v = it.next().unwrap_or("");
                if k == "subject" { subject = v.to_string(); }
                if k == "status" { status = v.to_string(); }
                if k == "module" { module = url_decode(v); }
                if k == "year" { year = url_decode(v); }
                if k == "juan" { juan = url_decode(v); }
            }
            // 真题筛选参数白名单校验（H3 白名单扩展）
            if (!module.is_empty() && !valid_whitelist(&module))
                || (!year.is_empty() && !valid_year(&year))
                || (!juan.is_empty() && !valid_whitelist(&juan)) {
                let _ = request.respond(json_response(400, &json!({"ok": false, "err": "参数不合法（module/juan 限字母数字_-中文，year 为4位数字）"})));
                return;
            }
            let mut qs = if subject == "zhenti" {
                // 真题聚合视图：跨全部真题科目按 module/year/juan 过滤
                bank.zhenti_questions(&module, &year, &juan)
            } else {
                let base = bank.public_questions(&subject);
                // 单科目也支持 module/year 过滤
                let sy = subject_year(&subject);
                base.into_iter()
                    .filter(|q| module.is_empty() || q["module"].as_str() == Some(module.as_str()))
                    .filter(|q| {
                        if year.is_empty() { return true; }
                        let y = match &sy { Some(y) => y.clone(), None => "综合".to_string() };
                        y == year
                    })
                    .collect()
            };
            if status == "undone" || status == "done" {
                let done = load_done(&user_file(&user, "done.json"));
                let done_ids: std::collections::HashSet<String> = done.into_iter().collect();
                if status == "undone" {
                    qs = qs.into_iter().filter(|q| {
                        let id = q["id"].as_str().unwrap_or("");
                        !done_ids.contains(id)
                    }).collect();
                } else {
                    qs = qs.into_iter().filter(|q| {
                        let id = q["id"].as_str().unwrap_or("");
                        done_ids.contains(id)
                    }).collect();
                }
            }
            let _ = request.respond(json_response(200, &json!({"questions": qs})));
            return;
        }
        ("POST", "/api/register") => {
            let body = read_body_limit(&mut request, MAX_BODY);
            let p: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let username = p["username"].as_str().unwrap_or("").trim().to_string();
            let password = p["password"].as_str().unwrap_or("").to_string();
            let name = p["name"].as_str().unwrap_or("").trim().to_string();
            // H3：用户名限 [A-Za-z0-9_\-]{2,20}
            if !valid_username(&username) || password.len() < 4 {
                let _ = request.respond(json_response(400, &json!({"ok": false, "err": "用户名需2-20位字母/数字/_-，密码≥4位"})));
                return;
            }
            let token = new_token();
            let is_first: bool;
            let display_name: String;
            {
                // H4：锁住 users.json 的 load-modify-save
                let _g = DATA_LOCK.lock().unwrap();
                let mut users = load_users();
                if users.contains_key(&username) {
                    let _ = request.respond(json_response(400, &json!({"ok": false, "err": "用户名已存在"})));
                    return;
                }
                let hash = bcrypt::hash(&password, bcrypt::DEFAULT_COST).unwrap_or_default();
                let now = now_secs();
                is_first = users.is_empty();
                // H2：首个用户为管理员
                let admin = is_first;
                users.insert(username.clone(), User {
                    username: username.clone(), hash,
                    name: if name.is_empty() { username.clone() } else { name.clone() },
                    created: now, admin,
                });
                save_users(&users);
                display_name = users[&username].name.clone();
                if is_first { migrate_legacy(&username); }
            }
            user_dir(&username);
            sessions.lock().unwrap().insert(token.clone(), (username.clone(), now_secs() + SESSION_TTL));
            let _ = request.respond(json_response(200, &json!({"ok": true, "token": token, "username": username, "name": display_name})));
            return;
        }
        ("POST", "/api/login") => {
            let body = read_body_limit(&mut request, MAX_BODY);
            let p: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let username = p["username"].as_str().unwrap_or("").trim().to_string();
            let password = p["password"].as_str().unwrap_or("").to_string();
            let ip = client_ip(&request);
            // M7：连续失败限流（指数退避）
            let pen = login_penalty(&ip, &username);
            if pen > 0 {
                let _ = request.respond(json_response(429, &json!({"ok": false, "err": format!("尝试过于频繁，请 {} 秒后再试", pen)})));
                return;
            }
            let mut ok = false;
            let mut name_display = String::new();
            {
                let _g = DATA_LOCK.lock().unwrap();
                let users = load_users();
                if let Some(u) = users.get(&username) {
                    if bcrypt::verify(&password, &u.hash).unwrap_or(false) {
                        ok = true;
                        name_display = u.name.clone();
                    }
                }
            }
            if !ok {
                note_login_fail(&ip, &username);
                let _ = request.respond(json_response(401, &json!({"ok": false, "err": "用户名或密码错误"})));
                return;
            }
            reset_login_fail(&ip, &username);
            let token = new_token();
            sessions.lock().unwrap().insert(token.clone(), (username.clone(), now_secs() + SESSION_TTL));
            let _ = request.respond(json_response(200, &json!({"ok": true, "token": token, "username": username, "name": name_display})));
            return;
        }
        ("POST", "/api/logout") => {
            let body = read_body_limit(&mut request, MAX_BODY);
            let p: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let tok = p["token"].as_str().unwrap_or("").to_string();
            sessions.lock().unwrap().remove(&tok);
            let _ = request.respond(json_response(200, &json!({"ok": true})));
            return;
        }
        _ => {}
    }

    // ---- 以下接口需要登录 ----
    let username = match auth_user(&request, sessions) {
        Some(u) => u,
        None => {
            let _ = request.respond(json_response(401, &json!({"ok": false, "err": "未登录"})));
            return;
        }
    };

    match (method.as_str(), path.as_str()) {
        ("GET", "/api/me") => {
            let users = load_users();
            let name = users.get(&username).map(|u| u.name.clone()).unwrap_or_else(|| username.clone());
            let _ = request.respond(json_response(200, &json!({"ok": true, "username": username, "name": name})));
        }
        ("GET", "/api/stats") => {
            let m = load_stats(&user_file(&username, "stats.json"));
            let mut mods: Vec<Value> = m.iter().filter(|(_, v)| v.t > 0).map(|(k, v)| {
                let mut it = k.split('|');
                let subject = it.next().unwrap_or("").to_string();
                let module = it.next().unwrap_or("").to_string();
                let pct = (v.c as f64 * 100.0 / v.t as f64 * 10.0).round() / 10.0;
                json!({"subject": subject, "module": module, "total": v.t, "correct": v.c, "pct": pct})
            }).collect();
            mods.sort_by(|a, b| a["pct"].as_f64().unwrap_or(0.0).partial_cmp(&b["pct"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
            let total_t: usize = m.values().map(|v| v.t).sum();
            let total_c: usize = m.values().map(|v| v.c).sum();
            let overall = if total_t > 0 { (total_c as f64 * 100.0 / total_t as f64 * 10.0).round() / 10.0 } else { 0.0 };
            let weak: Vec<Value> = mods.iter().take(3).cloned().collect();
            let _ = request.respond(json_response(200, &json!({"modules": mods, "weak": weak, "total": total_t, "correct": total_c, "overall": overall})));
        }
        ("GET", "/api/daily-stats") => {
            let dl = load_daily(&user_file(&username, "daily.json"));
            let mut days: Vec<Value> = dl.iter().map(|(d, s)| json!({
                "date": d, "t": s.t, "c": s.c,
                "pct": if s.t > 0 { (s.c as f64 * 100.0 / s.t as f64 * 10.0).round() / 10.0 } else { 0.0 }
            })).collect();
            days.sort_by(|a, b| b["date"].as_str().unwrap_or("").cmp(a["date"].as_str().unwrap_or("")));
            let today = days.first().cloned().unwrap_or(json!({"date":"","t":0,"c":0,"pct":0.0}));
            let _ = request.respond(json_response(200, &json!({"today": today, "days": days})));
        }
        ("POST", "/api/check") => {
            let body = read_body_limit(&mut request, MAX_BODY);
            let payload: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let answers = payload["answers"].as_object().cloned().unwrap_or_default();
            let date = payload["date"].as_str().unwrap_or("").to_string();
            // L1 公共判分 + M6（essay 返回 reference，objective 一律剔除）
            let (total, obj_total, obj_correct, results) = grade_answers(bank, &answers);
            let pct = if obj_total > 0 { (obj_correct as f64 * 100.0 / obj_total as f64 * 10.0).round() / 10.0 } else { 0.0 };
            // H4：统计/每日/已刷 在锁内 load-modify-save
            apply_grade_stats(&username, &answers, bank, &date);
            let _ = request.respond(json_response(200, &json!({
                "total": total, "objective_total": obj_total,
                "objective_correct": obj_correct, "score": pct, "results": results,
            })));
        }
        ("POST", "/api/report") => {
            let body = read_body_limit(&mut request, MAX_BODY);
            let p: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let qid = p["qid"].as_str().unwrap_or("").to_string();
            let q = bank.by_id.get(&qid);
            let rec = json!({
                "ts": now_secs(),
                "qid": qid,
                "subject": q.map(|q| q.subject.clone()).unwrap_or_else(|| p["subject"].as_str().unwrap_or("").to_string()),
                "module": q.map(|q| q.module.clone()).unwrap_or_default(),
                "stem": q.map(|q| q.stem.chars().take(80).collect::<String>()).unwrap_or_default(),
                "answer": q.map(|q| q.answer.clone()).unwrap_or_default(),
                "reason": p["reason"].as_str().unwrap_or("其他").to_string(),
                "note": p["note"].as_str().unwrap_or("").to_string(),
            });
            // H4：reports.json 在锁内 load-modify-save
            let count;
            {
                let _g = DATA_LOCK.lock().unwrap();
                let rp = user_file(&username, "reports.json");
                let mut arr: Vec<Value> = std::fs::read_to_string(&rp).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
                arr.push(rec);
                if let Ok(s) = serde_json::to_string_pretty(&arr) { let _ = std::fs::write(&rp, s); }
                count = arr.len();
            }
            let _ = request.respond(json_response(200, &json!({"ok": true, "count": count})));
        }
        ("GET", "/api/report-view") => {
            let date = url.split('?').nth(1)
                .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("date=").map(|v| v.to_string())))
                .unwrap_or_default();
            let date = if date.is_empty() { today_str() } else { date };
            // H3/L6：date 白名单 ^\d{4}-\d{2}-\d{2}$
            if !valid_date(&date) {
                let _ = request.respond(json_response(400, &json!({"ok": false, "err": "日期格式非法"})));
                return;
            }
            let base = user_dir(&username).join("reports");
            let _ = std::fs::create_dir_all(&base);
            let rp = match safe_child(&base, &format!("{}.md", date)) {
                Some(p) => p,
                None => {
                    let _ = request.respond(json_response(400, &json!({"ok": false, "err": "非法路径"})));
                    return;
                }
            };
            if rp.exists() {
                if let Ok(s) = std::fs::read_to_string(&rp) {
                    let _ = request.respond(text_response(200, "text/plain; charset=utf-8", s));
                    return;
                }
            }
            let _ = request.respond(json_response(404, &json!({"ok": false, "err": "该日暂无报告", "date": date})));
        }
        ("GET", "/api/report-dates") => {
            let rd = user_dir(&username).join("reports");
            let mut dates: Vec<String> = Vec::new();
            if let Ok(rdir) = std::fs::read_dir(&rd) {
                for e in rdir.flatten() {
                    let f = e.file_name().to_string_lossy().to_string();
                    if let Some(d) = f.strip_suffix(".md") { dates.push(d.to_string()); }
                }
            }
            dates.sort();
            dates.reverse();
            let _ = request.respond(json_response(200, &json!({"dates": dates})));
        }
        // ---- 模考组卷 / 交卷 / 历史 ----
        ("GET", "/api/mock/generate") => {
            let qs = url.split('?').nth(1).unwrap_or("");
            let qtype = qs.split('&').find_map(|kv| kv.strip_prefix("type=").map(|v| v.to_string()))
                .unwrap_or_else(|| "guokao".to_string());
            let paper = qs.split('&').find_map(|kv| kv.strip_prefix("paper=").map(|v| v.to_string()))
                .unwrap_or_else(|| "xingce".to_string());
            let count: usize = qs.split('&').find_map(|kv| kv.strip_prefix("count=").and_then(|v| v.parse().ok())).unwrap_or(0);
            // 各卷默认时长
            let duration_min: u64 = if qtype == "shengkao" {
                if paper == "shenlun" { 150 } else { 100 }
            } else {
                if paper == "shenlun" { 180 } else if paper == "zhuanye" { 120 } else { 120 }
            };
            let (picked, notes): (Vec<Q>, Vec<String>) = if qtype == "shengkao" {
                if paper == "shenlun" {
                    // 省考申论：从广东省考申论科抽；暂无则回退国考申论卷并返回 note
                    let gd_pool: Vec<Q> = bank.questions.iter()
                        .filter(|qq| qq.subject.starts_with("gd") && qq.qtype == "essay")
                        .cloned().collect();
                    if gd_pool.is_empty() {
                        let want = if count > 0 { count } else { 4 };
                        let (p, mut n) = build_shenlun(bank, want);
                        n.insert(0, "省考申论题库暂未入库，已回退为国考申论卷".to_string());
                        (p, n)
                    } else {
                        let want = if count > 0 { count } else { 4 };
                        let mut pool = gd_pool;
                        (shuffle_take(&mut pool, want), Vec::new())
                    }
                } else {
                    // 省考行测：按年份（gd20xx）真题结构配比抽题（默认约 100 题）
                    let want = if count > 0 { count } else { 100 };
                    build_shengkao_xingce(bank, want)
                }
            } else if paper == "zhuanye" {
                // 国考专业卷：jisuanji 模块 + zt2019-zt2023 + zx* 专项 等额抽题
                let want = if count > 0 { count } else { 100 };
                let ids: [&str; 14] = ["jisuanji", "zt2019","zt2020","zt2021","zt2022","zt2023",
                    "zxsj","zxdb","zxwl","zxos","zxaq","zxrg","zxzc","zxdx"];
                build_subject_paper(bank, &ids, want)
            } else if paper == "shenlun" {
                // 国考申论：slzt* + slgongwen/sldanyi/slwenzhang/slzonghe（essay 题）
                let want = if count > 0 { count } else { 5 };
                build_shenlun(bank, want)
            } else {
                // 国考行测：常识20+言语40+数量15+判断40+资料20=135题
                build_guokao_xingce(bank, count)
            };
            if picked.is_empty() {
                let _ = request.respond(json_response(200, &json!({"ok": false, "err": "题库不足，无法组卷", "notes": notes})));
                return;
            }
            let now = now_secs();
            let deadline_ts = now + duration_min as u64 * 60;
            // L7：paper_id 加随机后缀避免同秒碰撞
            let paper_id = format!("{}-{}-{}-{:x}", qtype, paper, now, rng_next() as u32);
            let qs_out: Vec<Value> = picked.iter().map(|qq| json!({
                "id": qq.id, "stem": qq.stem, "options": qq.options, "module": qq.module,
                "difficulty": qq.difficulty, "type": qq.qtype, "material": qq.material,
            })).collect();
            let active = json!({
                "ok": true, "paper_id": paper_id, "type": qtype, "paper": paper, "count": picked.len(),
                "duration_min": duration_min, "deadline_ts": deadline_ts,
                "questions": qs_out, "notes": notes,
            });
            let md = user_dir(&username).join("mocks");
            let _ = std::fs::create_dir_all(&md);
            let _ = std::fs::write(md.join("active.json"), serde_json::to_string_pretty(&active).unwrap_or_default());
            let _ = request.respond(json_response(200, &active));
        }
        ("POST", "/api/mock/submit") => {
            let body = read_body_limit(&mut request, MAX_BODY);
            let payload: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let paper_id = payload["paper_id"].as_str().unwrap_or("").to_string();
            let answers = payload["answers"].as_object().cloned().unwrap_or_default();
            let date = payload["date"].as_str().unwrap_or("").to_string();
            // H3：paper_id 白名单（防路径遍历）
            if !paper_id.is_empty() && !valid_paper_id(&paper_id) {
                let _ = request.respond(json_response(400, &json!({"ok": false, "err": "paper_id 非法"})));
                return;
            }
            let mocks_dir = user_dir(&username).join("mocks");
            let _ = std::fs::create_dir_all(&mocks_dir);
            let active_path = mocks_dir.join("active.json");
            let active: Value = std::fs::read_to_string(&active_path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
            let active_paper = active["paper_id"].as_str().unwrap_or("");
            let mtype = active["type"].as_str().unwrap_or("guokao").to_string();
            let mpaper = active["paper"].as_str().unwrap_or("").to_string();
            if !paper_id.is_empty() && !active_paper.is_empty() && active_paper != paper_id {
                let _ = request.respond(json_response(400, &json!({"ok": false, "err": "试卷已变更或已提交"})));
                return;
            }
            // L1 公共判分 + M6（essay 返回 reference，objective 剔除）
            let (total, obj_total, obj_correct, results) = grade_answers(bank, &answers);
            let pct = if obj_total > 0 { (obj_correct as f64 * 100.0 / obj_total as f64 * 10.0).round() / 10.0 } else { 0.0 };
            // H4：统计/每日/已刷 在锁内 load-modify-save
            apply_grade_stats(&username, &answers, bank, &date);
            let now = now_secs();
            let rec = json!({
                "paper_id": paper_id, "type": mtype, "paper": mpaper, "date": date, "score": pct,
                "objective_total": obj_total, "objective_correct": obj_correct,
                "results": results, "graded": true, "count": total, "ts": now,
            });
            {
                let _g = DATA_LOCK.lock().unwrap();
                let fname = if paper_id.is_empty() { format!("paper-{}.json", now) } else { format!("{}.json", paper_id) };
                let _ = std::fs::write(mocks_dir.join(fname), serde_json::to_string_pretty(&rec).unwrap_or_default());
                let _ = std::fs::remove_file(&active_path);
            }
            let _ = request.respond(json_response(200, &json!({
                "ok": true, "score": pct, "objective_total": obj_total, "objective_correct": obj_correct,
                "results": results, "total": total, "paper_id": paper_id,
            })));
        }
        ("GET", "/api/mock/history") => {
            let mocks_dir = user_dir(&username).join("mocks");
            let mut list: Vec<Value> = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&mocks_dir) {
                for e in rd.flatten() {
                    let fname = e.file_name().to_string_lossy().to_string();
                    if fname == "active.json" { continue; }
                    if e.path().extension().map(|x| x == "json").unwrap_or(false) {
                        if let Ok(s) = std::fs::read_to_string(e.path()) {
                            if let Ok(v) = serde_json::from_str::<Value>(&s) { list.push(v); }
                        }
                    }
                }
            }
            list.sort_by(|a, b| b["ts"].as_u64().unwrap_or(0).cmp(&a["ts"].as_u64().unwrap_or(0)));
            let _ = request.respond(json_response(200, &json!({"history": list})));
        }
        // ---- 针对性刷题：薄弱模块（正确率<60%）抽题 ----
        ("GET", "/api/targeted") => {
            let qs = url.split('?').nth(1).unwrap_or("");
            let count: usize = qs.split('&').find_map(|kv| kv.strip_prefix("count=").and_then(|v| v.parse().ok())).unwrap_or(20);
            let n = if count > 0 { count } else { 20 };
            let stats_path = user_file(&username, "stats.json");
            let daily_path = user_file(&username, "daily.json");
            let mut acc: HashMap<String, (usize, usize)> = HashMap::new();
            let m = load_stats(&stats_path);
            for (k, v) in &m {
                if v.t > 0 { acc.insert(k.clone(), (v.t, v.c)); }
            }
            // 每日数据仅补充 stats 缺失的 module（避免重复累计）
            if let Ok(s) = std::fs::read_to_string(&daily_path) {
                if let Ok(dl) = serde_json::from_str::<DailyMap>(&s) {
                    for (_d, day) in dl {
                        for (k, v) in day.by {
                            if v.t > 0 && !acc.contains_key(&k) { acc.insert(k, (v.t, v.c)); }
                        }
                    }
                }
            }
            let weak_keys: Vec<String> = acc.iter()
                .filter(|(_, (t, c))| *t > 0 && (*c as f64 * 100.0 / *t as f64) < 60.0)
                .map(|(k, _)| k.clone()).collect();
            let fallback = weak_keys.is_empty();
            let mut pool: Vec<Q> = Vec::new();
            if !fallback {
                for k in &weak_keys {
                    let (subj, modu) = k.split_once('|').unwrap_or((k.as_str(), ""));
                    pool.extend(bank.questions.iter()
                        .filter(|qq| qq.subject == subj && qq.module == modu && qq.qtype != "essay" && !qq.options.is_empty())
                        .cloned());
                }
            } else {
                pool = bank.questions.iter()
                    .filter(|qq| qq.qtype != "essay" && !qq.options.is_empty())
                    .cloned().collect();
            }
            if pool.is_empty() {
                let _ = request.respond(json_response(200, &json!({"ok": false, "err": "题库不足"})));
                return;
            }
            let take_n2 = n.min(pool.len());
            let picked = shuffle_take(&mut pool, take_n2);
            let qs_out: Vec<Value> = picked.iter().map(|qq| json!({
                "id": qq.id, "stem": qq.stem, "options": qq.options, "module": qq.module,
                "difficulty": qq.difficulty, "type": qq.qtype, "subject": qq.subject,
            })).collect();
            let note = if fallback {
                "暂无薄弱模块（正确率<60%），已从全库随机抽题".to_string()
            } else {
                format!("已从 {} 个薄弱模块抽题（正确率<60%）", weak_keys.len())
            };
            let _ = request.respond(json_response(200, &json!({"ok": true, "questions": qs_out, "weak_count": weak_keys.len(), "fallback": fallback, "note": note})));
        }
        // ---- 考试截止时间 ----
        ("GET", "/api/exam-deadline") => {
            let ed = user_file(&username, "exam.json");
            let v: Value = std::fs::read_to_string(&ed).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
            let _ = request.respond(json_response(200, &json!({
                "guokao": v["guokao"].as_str().unwrap_or("").to_string(),
                "shengkao": v["shengkao"].as_str().unwrap_or("").to_string(),
            })));
        }
        ("POST", "/api/exam-deadline") => {
            let body = read_body_limit(&mut request, MAX_BODY);
            let p: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let g = p["guokao"].as_str().unwrap_or("").to_string();
            let s = p["shengkao"].as_str().unwrap_or("").to_string();
            let v = json!({"guokao": g, "shengkao": s});
            let _g = DATA_LOCK.lock().unwrap();
            let _ = std::fs::write(user_file(&username, "exam.json"), serde_json::to_string_pretty(&v).unwrap_or_default());
            let _ = request.respond(json_response(200, &json!({"ok": true, "guokao": g, "shengkao": s})));
        }
        // ---- 申论 AI 评分 ----
        ("POST", "/api/shenlun/submit") => {
            let body = read_body_limit(&mut request, MAX_BODY);
            let p: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let qid = p["qid"].as_str().unwrap_or("").to_string();
            let text = p["text"].as_str().unwrap_or("").to_string();
            let file = p["file"].as_str().unwrap_or("").to_string();
            // 校验：必须是申论（essay）题
            match bank.by_id.get(&qid) {
                Some(q) if q.qtype == "essay" => {}
                _ => { let _ = request.respond(json_response(400, &json!({"ok": false, "err": "非申论题"}))); return; }
            }
            if text.trim().is_empty() && file.trim().is_empty() {
                let _ = request.respond(json_response(400, &json!({"ok": false, "err": "作答内容不能为空"})));
                return;
            }
            let now = now_secs();
            let sub_id = format!("sl-{}-{:08x}", now, rng_next() as u32);
            let rec = json!({
                "id": sub_id, "qid": qid, "text": text, "file": file,
                "submitted_at": now, "status": "pending",
            });
            {
                let _g = DATA_LOCK.lock().unwrap();
                let _ = std::fs::write(shenlun_dir(&username).join(format!("{}.json", sub_id)),
                    serde_json::to_string_pretty(&rec).unwrap_or_default());
            }
            let _ = request.respond(json_response(200, &json!({"ok": true, "submission_id": sub_id})));
        }
        ("GET", "/api/shenlun/submissions") => {
            let list = read_shenlun_submissions(&username);
            // 附上题干摘要（便于前端展示）
            let out: Vec<Value> = list.iter().map(|s| {
                let qid = s["qid"].as_str().unwrap_or("").to_string();
                let stem = bank.by_id.get(&qid).map(|q| q.stem.chars().take(60).collect::<String>()).unwrap_or_default();
                json!({
                    "id": s["id"], "qid": qid, "stem": stem,
                    "status": s["status"].as_str().unwrap_or("pending"),
                    "score": s.get("score").cloned().unwrap_or(Value::Null),
                    "feedback": s.get("feedback").cloned().unwrap_or(Value::Null),
                    "submitted_at": s["submitted_at"],
                })
            }).collect();
            let _ = request.respond(json_response(200, &json!({"submissions": out})));
        }
        ("GET", "/api/shenlun/pending") => {
            // H2：仅管理员可查看跨用户 pending（AI 评分 cron 用）
            let users = load_users();
            if !is_admin(&users, &username) {
                let _ = request.respond(json_response(403, &json!({"ok": false, "err": "仅管理员可查看"})));
                return;
            }
            let list = list_shenlun_pending();
            // 附上题干/给定资料摘要（供 AI 评分参考）
            let out: Vec<Value> = list.iter().map(|s| {
                let qid = s["qid"].as_str().unwrap_or("").to_string();
                let q = bank.by_id.get(&qid);
                let mut rec = s.clone();
                rec["stem"] = json!(q.map(|qq| qq.stem.clone()).unwrap_or_default());
                rec["material"] = json!(q.map(|qq| qq.material.chars().take(2000).collect::<String>()).unwrap_or_default());
                rec["reference"] = json!(q.map(|qq| qq.reference.clone()).unwrap_or_default());
                rec
            }).collect();
            let _ = request.respond(json_response(200, &json!({"pending": out, "count": out.len()})));
        }
        ("POST", "/api/shenlun/grade") => {
            // H2：仅管理员可评分
            let users = load_users();
            if !is_admin(&users, &username) {
                let _ = request.respond(json_response(403, &json!({"ok": false, "err": "仅管理员可评分"})));
                return;
            }
            let body = read_body_limit(&mut request, MAX_BODY);
            let p: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let sub_id = p["submission_id"].as_str().unwrap_or("").to_string();
            if sub_id.is_empty() {
                let _ = request.respond(json_response(400, &json!({"ok": false, "err": "缺少 submission_id"})));
                return;
            }
            match find_shenlun_global(&sub_id) {
                Some((_u, path, mut rec)) => {
                    let score = p.get("score").cloned().unwrap_or(Value::Null);
                    let feedback = p.get("feedback").cloned().unwrap_or(Value::Null);
                    let now = now_secs();
                    rec["status"] = json!("graded");
                    rec["score"] = score;
                    rec["feedback"] = feedback;
                    rec["graded_at"] = json!(now);
                    let _g = DATA_LOCK.lock().unwrap();
                    let _ = std::fs::write(&path, serde_json::to_string_pretty(&rec).unwrap_or_default());
                    let _ = request.respond(json_response(200, &json!({"ok": true, "submission_id": sub_id, "status": "graded"})));
                }
                None => { let _ = request.respond(json_response(404, &json!({"ok": false, "err": "提交不存在"}))); }
            }
        }
        ("GET", "/api/shenlun/grade") => {
            let sub_id = url.split('?').nth(1)
                .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("submission_id=").map(|v| v.to_string())))
                .unwrap_or_default();
            if sub_id.is_empty() {
                let _ = request.respond(json_response(400, &json!({"ok": false, "err": "缺少 submission_id"})));
                return;
            }
            match find_shenlun_global(&sub_id) {
                Some((u, _path, rec)) => {
                    // H2：仅本人提交或管理员可读
                    let users = load_users();
                    if !is_admin(&users, &username) && u != username {
                        let _ = request.respond(json_response(403, &json!({"ok": false, "err": "无权查看该提交"})));
                        return;
                    }
                    let _ = request.respond(json_response(200, &json!({"ok": true, "submission": rec, "username": u})));
                }
                None => { let _ = request.respond(json_response(404, &json!({"ok": false, "err": "提交不存在"}))); }
            }
        }
        _ => { let _ = request.respond(text_response(404, "text/plain; charset=utf-8", "not found".to_string())); }
    }
}

fn today_str() -> String {
    // 简单 UTC+8 日期（够用于报告文件名）
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let secs = now + 8 * 3600;
    let days = secs / 86400;
    // 用 chrono 更准，但避免新依赖；简单算
    let (yy, mm, dd) = civil_from_days(days as i64);
    format!("{:04}-{:02}-{:02}", yy, mm, dd)
}
// 从天数(1970-01-01=0)算公历日期
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe/1460 + doe/36524 - doe/146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365*yoe + yoe/4 - yoe/100);
    let mp = (5*doy + 2)/153;
    let d = (doy - (153*mp+2)/5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
