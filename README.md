# 国考刷题网页（shuati-webui-rust）

端口 **8085** · Rust (tiny_http) 单二进制 · 服务端判分（答案不下发，真题模考不泄题）

## 科目与题库（助手自编模拟题，真题留给最后模考）
- **行测** 40 题：判断推理 / 数量关系 / 言语理解与表达 / 资料分析
- **计算机岗** 40 题：数据结构 / 操作系统 / 计算机网络 / 数据库 / 信息安全
- **申论** 6 题（3 类）：归纳概括 / 综合分析 / 提出对策 / 大作文（主观，交卷后看参考要点）

## 构建（本机不编译，走 GitHub Actions 交叉编译 aarch64-musl）
```bash
cargo zigbuild --release --target aarch64-unknown-linux-musl
```

## 运行
```bash
./shuati-webui-rust --port 8085 --bind 0.0.0.0
```
- `GET /` 前端（内嵌）
- `GET /api/subjects` 科目列表
- `GET /api/questions?subject=xingce|jisuanji|shenlun` 练习题（不含答案）
- `POST /api/check` 交卷判分（`{"answers":{"x01":"B",...}}`）

## 题库结构（web/questions.json）
```json
{ "subjects":[{"id":"xingce","name":"行测","modules":[...]}],
  "questions":{"xingce":[{"id":"x01","module":"判断推理","type":"single",
    "stem":"...","options":["A. ...","B. ..."],"answer":"B","explain":"..."}], ...} }
```
- `type`: `single` 单选 / `multi` 多选 / `essay` 申论主观（`material` 材料 + `reference` 参考要点）
- 新增题目：往对应 `questions.<subject>` 数组追加即可，无需改代码
