// shuati-webui-rust — 国考刷题网页（行测/申论/计算机岗）
// tiny_http 单二进制，内嵌前端与题库；服务端判分，答案不下发。
// v0.2 多用户：账号密码登录（bcrypt）、每用户独立数据目录、每晚 21:00 学习报告（AI 生成，网页查看）。
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Response, Server};

const QUIZ_JSON: &str = include_str!("../web/questions.json");
const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLE_CSS: &str = include_str!("../web/style.css");

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
                });
            }
        }
        let by_id = questions
            .iter()
            .map(|q| (q.id.clone(), q.clone()))
            .collect();
        Bank { subjects, questions, by_id }
    }

    // 供前端练习用：剥掉 answer/explain/reference（服务端判分，不泄题）
    fn public_questions(&self, subject: &str) -> Vec<Value> {
        self.questions
            .iter()
            .filter(|q| q.subject == subject)
            .map(|q| {
                json!({
                    "id": q.id, "module": q.module, "type": q.qtype,
                    "stem": q.stem, "options": q.options, "material": q.material,
                    "difficulty": q.difficulty,
                })
            })
            .collect()
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

// ---- 会话（内存）----
type Sessions = Arc<Mutex<HashMap<String, String>>>; // token -> username

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

fn auth_user(request: &tiny_http::Request, sessions: &Sessions) -> Option<String> {
    // token 来自 Authorization: Bearer <t> 或 ?token=<t>
    let mut tok = String::new();
    for h in request.headers() {
        if h.field.equiv("Authorization") {
            let v = h.value.as_str().to_string();
            if let Some(rest) = v.strip_prefix("Bearer ") {
                tok = rest.trim().to_string();
            }
        }
    }
    if tok.is_empty() {
        if let Some(q) = request.url().split('?').nth(1) {
            for kv in q.split('&') {
                if let Some(v) = kv.strip_prefix("token=") {
                    tok = v.to_string();
                }
            }
        }
    }
    if tok.is_empty() { return None; }
    sessions.lock().unwrap().get(&tok).cloned()
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

fn main() {
    let mut port: u16 = 8085;
    let mut bind: String = "0.0.0.0".to_string();
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
            let subs: Vec<Value> = bank.subjects.iter().map(|s| json!({
                "id": s.id, "name": s.name, "modules": s.modules,
                "count": bank.questions.iter().filter(|q| q.subject == s.id).count(),
            })).collect();
            let _ = request.respond(json_response(200, &json!({"subjects": subs})));
            return;
        }
        ("GET", "/api/questions") => {
            let qs: Vec<Value> = request.url().split('?').nth(1)
                .map(|q| q.split('&').find_map(|kv| {
                    let mut it = kv.split('='); let k = it.next().unwrap_or(""); let v = it.next().unwrap_or("");
                    if k == "subject" { Some(v) } else { None }
                })).flatten()
                .map(|s| bank.public_questions(s))
                .unwrap_or_else(|| bank.public_questions("xingce"));
            let _ = request.respond(json_response(200, &json!({"questions": qs})));
            return;
        }
        ("POST", "/api/register") => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let p: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let username = p["username"].as_str().unwrap_or("").trim().to_string();
            let password = p["password"].as_str().unwrap_or("").to_string();
            let name = p["name"].as_str().unwrap_or("").trim().to_string();
            if username.len() < 2 || username.len() > 20 || password.len() < 4 {
                let _ = request.respond(json_response(400, &json!({"ok": false, "err": "用户名≥2字符、密码≥4位"})));
                return;
            }
            let mut users = load_users();
            if users.contains_key(&username) {
                let _ = request.respond(json_response(400, &json!({"ok": false, "err": "用户名已存在"})));
                return;
            }
            let hash = bcrypt::hash(&password, bcrypt::DEFAULT_COST).unwrap_or_default();
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            // 首个用户迁移旧的共享数据
            let is_first = users.is_empty();
            users.insert(username.clone(), User { username: username.clone(), hash, name: if name.is_empty() { username.clone() } else { name }, created: now });
            save_users(&users);
            user_dir(&username);
            if is_first { migrate_legacy(&username); }
            let token = new_token();
            sessions.lock().unwrap().insert(token.clone(), username.clone());
            let _ = request.respond(json_response(200, &json!({"ok": true, "token": token, "username": username, "name": users[&username].name})));
            return;
        }
        ("POST", "/api/login") => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let p: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let username = p["username"].as_str().unwrap_or("").trim().to_string();
            let password = p["password"].as_str().unwrap_or("").to_string();
            let users = load_users();
            let ok = users.get(&username).map(|u| bcrypt::verify(&password, &u.hash).unwrap_or(false)).unwrap_or(false);
            if !ok {
                let _ = request.respond(json_response(401, &json!({"ok": false, "err": "用户名或密码错误"})));
                return;
            }
            let token = new_token();
            sessions.lock().unwrap().insert(token.clone(), username.clone());
            let _ = request.respond(json_response(200, &json!({"ok": true, "token": token, "username": username, "name": users[&username].name})));
            return;
        }
        ("POST", "/api/logout") => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
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
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let payload: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let answers = payload["answers"].as_object().cloned().unwrap_or_default();

            let mut total = 0usize;
            let mut obj_total = 0usize;
            let mut obj_correct = 0usize;
            let mut results = Vec::new();

            for (qid, ans_val) in &answers {
                let q = match bank.by_id.get(qid) { Some(q) => q, None => continue };
                let ans = ans_val.as_str().unwrap_or("");
                if q.qtype == "essay" {
                    total += 1;
                    results.push(json!({"id": qid, "type": "essay", "correct": Value::Null, "your": ans, "right": Value::Null, "explain": q.explain, "reference": q.reference}));
                } else {
                    obj_total += 1; total += 1;
                    let right = q.answer.to_uppercase();
                    let your = ans.to_uppercase().replace([' ', ','], "");
                    let ok = !your.is_empty() && your == right;
                    if ok { obj_correct += 1; }
                    results.push(json!({"id": qid, "type": q.qtype, "correct": ok, "your": your, "right": right, "explain": q.explain, "reference": Value::Null}));
                }
            }
            let pct = if obj_total > 0 { (obj_correct as f64 * 100.0 / obj_total as f64 * 10.0).round() / 10.0 } else { 0.0 };

            // 每用户独立统计 + 每日
            let stats_path = user_file(&username, "stats.json");
            let daily_path = user_file(&username, "daily.json");
            {
                let mut m = load_stats(&stats_path);
                for (qid, ans_val) in &answers {
                    let q = match bank.by_id.get(qid) { Some(q) => q, None => continue };
                    if q.qtype == "essay" { continue; }
                    let ans = ans_val.as_str().unwrap_or("");
                    let right = q.answer.to_uppercase();
                    let your = ans.to_uppercase().replace([' ', ','], "");
                    let ok = !your.is_empty() && your == right;
                    let key = format!("{}|{}", q.subject, q.module);
                    let e = m.entry(key).or_default(); e.t += 1; if ok { e.c += 1; }
                }
                save_stats(&stats_path, &m);
            }
            {
                let date = payload["date"].as_str().unwrap_or("").to_string();
                if !date.is_empty() {
                    let mut dl = load_daily(&daily_path);
                    let d = dl.entry(date.clone()).or_default();
                    for (qid, ans_val) in &answers {
                        let q = match bank.by_id.get(qid) { Some(q) => q, None => continue };
                        if q.qtype == "essay" { continue; }
                        let ans = ans_val.as_str().unwrap_or("");
                        let right = q.answer.to_uppercase();
                        let your = ans.to_uppercase().replace([' ', ','], "");
                        let ok = !your.is_empty() && your == right;
                        let key = format!("{}|{}", q.subject, q.module);
                        d.t += 1; let e = d.by.entry(key).or_default(); e.t += 1;
                        if ok { d.c += 1; e.c += 1; }
                    }
                    save_daily(&daily_path, &dl);
                }
            }
            let _ = request.respond(json_response(200, &json!({
                "total": total, "objective_total": obj_total,
                "objective_correct": obj_correct, "score": pct, "results": results,
            })));
        }
        ("POST", "/api/report") => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let p: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let qid = p["qid"].as_str().unwrap_or("").to_string();
            let q = bank.by_id.get(&qid);
            let rec = json!({
                "ts": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
                "qid": qid,
                "subject": q.map(|q| q.subject.clone()).unwrap_or_else(|| p["subject"].as_str().unwrap_or("").to_string()),
                "module": q.map(|q| q.module.clone()).unwrap_or_default(),
                "stem": q.map(|q| q.stem.chars().take(80).collect::<String>()).unwrap_or_default(),
                "answer": q.map(|q| q.answer.clone()).unwrap_or_default(),
                "reason": p["reason"].as_str().unwrap_or("其他").to_string(),
                "note": p["note"].as_str().unwrap_or("").to_string(),
            });
            let rp = user_file(&username, "reports.json");
            let mut arr: Vec<Value> = std::fs::read_to_string(&rp).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            arr.push(rec);
            if let Ok(s) = serde_json::to_string_pretty(&arr) { let _ = std::fs::write(&rp, s); }
            let _ = request.respond(json_response(200, &json!({"ok": true, "count": arr.len()})));
        }
        ("GET", "/api/report-view") => {
            let date = url.split('?').nth(1)
                .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("date=").map(|v| v.to_string())))
                .unwrap_or_default();
            let date = if date.is_empty() { today_str() } else { date };
            let rp = user_dir(&username).join("reports").join(format!("{}.md", date));
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
            let count: usize = qs.split('&').find_map(|kv| kv.strip_prefix("count=").and_then(|v| v.parse().ok())).unwrap_or(0);
            let pool_subjects: Vec<String> = if qtype == "shengkao" {
                bank.subjects.iter().filter(|s| s.id.starts_with("guangdong")).map(|s| s.id.clone()).collect()
            } else {
                vec!["zt2019".into(), "zt2020".into(), "zt2021".into(), "zt2022".into(), "zt2023".into(),
                     "changshi".into(), "yanyu".into(), "shuliang".into(), "panduan".into(), "ziliao".into()]
            };
            if qtype == "shengkao" && pool_subjects.is_empty() {
                let _ = request.respond(json_response(200, &json!({"ok": false, "err": "暂无省考题"})));
                return;
            }
            let mut pool: Vec<Q> = bank.questions.iter()
                .filter(|qq| pool_subjects.contains(&qq.subject) && qq.qtype != "essay" && !qq.options.is_empty())
                .cloned().collect();
            if pool.is_empty() {
                let _ = request.respond(json_response(200, &json!({"ok": false, "err": "题库不足，无法组卷"})));
                return;
            }
            let duration_min = if qtype == "shengkao" { 100 } else { 120 };
            let n = if count > 0 { count } else { duration_min as usize };
            let take_n = n.min(pool.len());
            let picked = shuffle_take(&mut pool, take_n);
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let deadline_ts = now + duration_min as u64 * 60;
            let paper_id = format!("{}-{}", qtype, now);
            let qs_out: Vec<Value> = picked.iter().map(|qq| json!({
                "id": qq.id, "stem": qq.stem, "options": qq.options, "module": qq.module,
                "difficulty": qq.difficulty, "type": qq.qtype,
            })).collect();
            let active = json!({
                "paper_id": paper_id, "type": qtype, "count": picked.len(),
                "duration_min": duration_min, "deadline_ts": deadline_ts,
                "questions": qs_out,
            });
            let md = user_dir(&username).join("mocks");
            let _ = std::fs::create_dir_all(&md);
            let _ = std::fs::write(md.join("active.json"), serde_json::to_string_pretty(&active).unwrap_or_default());
            let _ = request.respond(json_response(200, &active));
        }
        ("POST", "/api/mock/submit") => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let payload: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let paper_id = payload["paper_id"].as_str().unwrap_or("").to_string();
            let answers = payload["answers"].as_object().cloned().unwrap_or_default();
            let date = payload["date"].as_str().unwrap_or("").to_string();
            let mocks_dir = user_dir(&username).join("mocks");
            let _ = std::fs::create_dir_all(&mocks_dir);
            let active_path = mocks_dir.join("active.json");
            let active: Value = std::fs::read_to_string(&active_path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
            let active_paper = active["paper_id"].as_str().unwrap_or("");
            let mtype = active["type"].as_str().unwrap_or("guokao").to_string();
            if !paper_id.is_empty() && !active_paper.is_empty() && active_paper != paper_id {
                let _ = request.respond(json_response(400, &json!({"ok": false, "err": "试卷已变更或已提交"})));
                return;
            }
            // 判分（objective 去空格逗号；essay 不算分）
            let mut total = 0usize;
            let mut obj_total = 0usize;
            let mut obj_correct = 0usize;
            let mut results = Vec::new();
            for (qid, ans_val) in &answers {
                let q = match bank.by_id.get(qid) { Some(q) => q, None => continue };
                let ans = ans_val.as_str().unwrap_or("");
                if q.qtype == "essay" {
                    total += 1;
                    results.push(json!({"id": qid, "type": "essay", "correct": Value::Null, "your": ans, "right": Value::Null, "explain": q.explain}));
                } else {
                    obj_total += 1; total += 1;
                    let right = q.answer.to_uppercase();
                    let your = ans.to_uppercase().replace([' ', ','], "");
                    let ok = !your.is_empty() && your == right;
                    if ok { obj_correct += 1; }
                    results.push(json!({"id": qid, "type": q.qtype, "correct": ok, "your": your, "right": right, "explain": q.explain}));
                }
            }
            let pct = if obj_total > 0 { (obj_correct as f64 * 100.0 / obj_total as f64 * 10.0).round() / 10.0 } else { 0.0 };
            // 累计统计 + 每日（复用 /api/check 逻辑，使模考计入每日进度）
            let stats_path = user_file(&username, "stats.json");
            let daily_path = user_file(&username, "daily.json");
            {
                let mut m = load_stats(&stats_path);
                for (qid, ans_val) in &answers {
                    let q = match bank.by_id.get(qid) { Some(q) => q, None => continue };
                    if q.qtype == "essay" { continue; }
                    let ans = ans_val.as_str().unwrap_or("");
                    let right = q.answer.to_uppercase();
                    let your = ans.to_uppercase().replace([' ', ','], "");
                    let ok = !your.is_empty() && your == right;
                    let key = format!("{}|{}", q.subject, q.module);
                    let e = m.entry(key).or_default(); e.t += 1; if ok { e.c += 1; }
                }
                save_stats(&stats_path, &m);
            }
            if !date.is_empty() {
                let mut dl = load_daily(&daily_path);
                let d = dl.entry(date.clone()).or_default();
                for (qid, ans_val) in &answers {
                    let q = match bank.by_id.get(qid) { Some(q) => q, None => continue };
                    if q.qtype == "essay" { continue; }
                    let ans = ans_val.as_str().unwrap_or("");
                    let right = q.answer.to_uppercase();
                    let your = ans.to_uppercase().replace([' ', ','], "");
                    let ok = !your.is_empty() && your == right;
                    let key = format!("{}|{}", q.subject, q.module);
                    d.t += 1; let e = d.by.entry(key).or_default(); e.t += 1;
                    if ok { d.c += 1; e.c += 1; }
                }
                save_daily(&daily_path, &dl);
            }
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let rec = json!({
                "paper_id": paper_id, "type": mtype, "date": date, "score": pct,
                "objective_total": obj_total, "objective_correct": obj_correct,
                "results": results, "graded": true, "count": total, "ts": now,
            });
            let fname = if paper_id.is_empty() { format!("paper-{}.json", now) } else { format!("{}.json", paper_id) };
            let _ = std::fs::write(mocks_dir.join(fname), serde_json::to_string_pretty(&rec).unwrap_or_default());
            let _ = std::fs::remove_file(&active_path);
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
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let p: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let g = p["guokao"].as_str().unwrap_or("").to_string();
            let s = p["shengkao"].as_str().unwrap_or("").to_string();
            let v = json!({"guokao": g, "shengkao": s});
            let _ = std::fs::write(user_file(&username, "exam.json"), serde_json::to_string_pretty(&v).unwrap_or_default());
            let _ = request.respond(json_response(200, &json!({"ok": true, "guokao": g, "shengkao": s})));
        }
        _ => { let _ = request.respond(text_response(404, "text/plain; charset=utf-8", "not found".to_string())); }
    }
}

fn today_str() -> String {
    // 简单 UTC+8 日期（够用于报告文件名）
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let secs = now + 8 * 3600;
    let days = secs / 86400;
    let y = 1970 + (days * 400 + 0) / 146097; // 近似，够用
    let _ = y;
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
