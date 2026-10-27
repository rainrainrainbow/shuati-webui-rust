// shuati-webui-rust — 国考刷题网页（行测/申论/计算机岗）
// tiny_http 单二进制，内嵌前端与题库；服务端判分，答案不下发（真题模考不泄题）。
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
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
}

struct Bank {
    subjects: Vec<Subject>,
    questions: Vec<Q>,
    by_id: HashMap<String, Q>,
}

impl Bank {
    fn load() -> Bank {
        let raw: Value = serde_json::from_str(QUIZ_JSON).expect("questions.json 解析失败");
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
                });
            }
        }
        let by_id = questions
            .iter()
            .map(|q| (q.id.clone(), q.clone()))
            .collect();
        Bank {
            subjects,
            questions,
            by_id,
        }
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
                })
            })
            .collect()
    }
}

fn main() {
    let mut port: u16 = 8085;
    let mut bind: String = "0.0.0.0".to_string();
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" if i + 1 < args.len() => {
                port = args[i + 1].parse().unwrap_or(8085);
                i += 2;
            }
            "--bind" if i + 1 < args.len() => {
                bind = args[i + 1].clone();
                i += 2;
            }
            "-h" | "--help" => {
                println!(
                    "shuati-webui-rust — 国考刷题网页 (行测/申论/计算机岗)\n \
                     --port <N>   listen port (default 8085)\n \
                     --bind <IP>  bind address (default 0.0.0.0)"
                );
                return;
            }
            _ => i += 1,
        }
    }

    // ---- 统计状态（按 subject|module 累计正确率，持久化到文件） ------------
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let dir = PathBuf::from(&home).join(".shuati-webui-rust");
    let _ = std::fs::create_dir_all(&dir);
    let stats_path = dir.join("stats.json");
    let stats = Arc::new(Mutex::new(load_stats(&stats_path)));

    let bank = Arc::new(Bank::load());
    let server = Server::http((bind.as_str(), port)).expect("绑定端口失败");
    println!("shuati-webui 已启动 http://{bind}:{port}  (题目 {} 道)",
        bank.questions.len());

    for request in server.incoming_requests() {
        let bank = bank.clone();
        let stats = stats.clone();
        let stats_path = stats_path.clone();
        std::thread::spawn(move || {
            let _ = handle(request, &bank, &stats, &stats_path);
        });
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct ModStat {
    t: usize,
    c: usize,
}

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

fn text_response(code: u16, content_type: &str, body: String) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(body.into_bytes())
        .with_status_code(code)
        .with_header(
            Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap(),
        )
}

fn json_response(code: u16, v: &Value) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    text_response(code, "application/json; charset=utf-8", v.to_string())
}

fn handle(
    mut request: tiny_http::Request,
    bank: &Bank,
    stats: &Arc<Mutex<StatsMap>>,
    stats_path: &std::path::Path,
) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("/").to_string();

    match (method.as_str(), path.as_str()) {
        (_, "/") => { let _ = request.respond(text_response(200, "text/html; charset=utf-8", INDEX_HTML.to_string())); }
        (_, "/app.js") => { let _ = request.respond(text_response(200, "application/javascript; charset=utf-8", APP_JS.to_string())); }
        (_, "/style.css") => { let _ = request.respond(text_response(200, "text/css; charset=utf-8", STYLE_CSS.to_string())); }
        ("GET", "/api/subjects") => {
            let subs: Vec<Value> = bank
                .subjects
                .iter()
                .map(|s| {
                    json!({
                        "id": s.id, "name": s.name, "modules": s.modules,
                        "count": bank.questions.iter().filter(|q| q.subject == s.id).count(),
                    })
                })
                .collect();
            let _ = request.respond(json_response(200, &json!({ "subjects": subs })));
        }
        ("GET", "/api/stats") => {
            let m = stats.lock().unwrap();
            let mut mods: Vec<Value> = m
                .iter()
                .filter(|(_, v)| v.t > 0)
                .map(|(k, v)| {
                    let mut it = k.split('|');
                    let subject = it.next().unwrap_or("").to_string();
                    let module = it.next().unwrap_or("").to_string();
                    let pct = (v.c as f64 * 100.0 / v.t as f64 * 10.0).round() / 10.0;
                    json!({
                        "subject": subject, "module": module,
                        "total": v.t, "correct": v.c, "pct": pct,
                    })
                })
                .collect();
            mods.sort_by(|a, b| a["pct"].as_f64().unwrap_or(0.0).partial_cmp(&b["pct"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
            let total_t: usize = m.values().map(|v| v.t).sum();
            let total_c: usize = m.values().map(|v| v.c).sum();
            let overall = if total_t > 0 {
                (total_c as f64 * 100.0 / total_t as f64 * 10.0).round() / 10.0
            } else { 0.0 };
            let weak: Vec<Value> = mods.iter().take(3).cloned().collect();
            let _ = request.respond(json_response(200, &json!({
                "modules": mods, "weak": weak,
                "total": total_t, "correct": total_c, "overall": overall,
            })));
        }
        ("GET", "/api/questions") => {
            let qs: Vec<Value> = request.url().split('?').nth(1)
                .map(|q| {
                    q.split('&')
                        .find_map(|kv| {
                            let mut it = kv.split('=');
                            let k = it.next().unwrap_or("");
                            let v = it.next().unwrap_or("");
                            if k == "subject" { Some(v) } else { None }
                        })
                })
                .flatten()
                .map(|s| bank.public_questions(s))
                .unwrap_or_else(|| bank.public_questions("xingce"));
            let _ = request.respond(json_response(200, &json!({ "questions": qs })));
        }
        ("POST", "/api/check") => {
            // body: {"answers":{"x01":"B",...,"s1":"<essay>"}}
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let payload: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            let answers = payload["answers"].as_object().cloned().unwrap_or_default();

            let mut total = 0usize;
            let mut obj_total = 0usize;
            let mut obj_correct = 0usize;
            let mut results = Vec::new();

            for (qid, q) in &bank.by_id {
                let ans = answers.get(qid).and_then(|v| v.as_str()).unwrap_or("");
                if q.qtype == "essay" {
                    total += 1;
                    results.push(json!({
                        "id": qid, "type": "essay",
                        "correct": Value::Null,
                        "your": ans, "right": Value::Null,
                        "explain": q.explain, "reference": q.reference,
                    }));
                } else {
                    obj_total += 1;
                    total += 1;
                    let right = q.answer.to_uppercase();
                    let your = ans.to_uppercase().replace([' ', ','], "");
                    let ok = !your.is_empty() && your == right;
                    if ok {
                        obj_correct += 1;
                    }
                    results.push(json!({
                        "id": qid, "type": q.qtype,
                        "correct": ok,
                        "your": your, "right": right,
                        "explain": q.explain,
                        "reference": Value::Null,
                    }));
                }
            }
            let pct = if obj_total > 0 {
                (obj_correct as f64 * 100.0 / obj_total as f64 * 10.0).round() / 10.0
            } else {
                0.0
            };
            // ---- 记录成绩（按 subject|module）----
            {
                let mut m = stats.lock().unwrap();
                for (qid, q) in &bank.by_id {
                    if q.qtype == "essay" {
                        continue;
                    }
                    let ans = answers.get(qid).and_then(|v| v.as_str()).unwrap_or("");
                    let right = q.answer.to_uppercase();
                    let your = ans.to_uppercase().replace([' ', ','], "");
                    let ok = !your.is_empty() && your == right;
                    let key = format!("{}|{}", q.subject, q.module);
                    let e = m.entry(key).or_default();
                    e.t += 1;
                    if ok {
                        e.c += 1;
                    }
                }
                save_stats(stats_path, &m);
            }
            let _ = request.respond(json_response(200, &json!({
                "total": total, "objective_total": obj_total,
                "objective_correct": obj_correct, "score": pct,
                "results": results,
            })));
        }
        _ => { let _ = request.respond(text_response(404, "text/plain; charset=utf-8", "not found".to_string())); }
    }
}
