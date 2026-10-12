# WT Tactical Command（内部）

War Thunder 战术指挥系统源码仓库：8111 数据采集端、接收服务器、数据分析、Canvas 战术 Web 地图，以及规划与调研文档。

> 私有仓库，仅内部使用。不含任何录制数据、二进制、token / 密钥。

当前规则：实时观测超过 **2 秒**直接丢弃，不归档、不补传、不复盘；静态底图、地图元数据及房间标绘保留。旧历史文件保留。

实施与协议见 [docs/REALTIME_V2.md](docs/REALTIME_V2.md)，验收见 [docs/ACCEPTANCE_V2.md](docs/ACCEPTANCE_V2.md)。

## 目录结构

| 路径 | 内容 |
|---|---|
| `wt-collect/` | Rust workspace（`common` / `collector` / `receiver` / `analyzer`），另含 `mock/`（8111 模拟器、延迟代理）、`deploy/`（HK 服务器部署脚本与 systemd/Caddy 配置）、`scripts/`（本地 receiver 管理、e2e 测试）、`test/`（测试脚本）、`dist/pkg/`（Windows 发行包模板） |
| `web-proto/` | Canvas 战术 Web 地图（Vite + React + TypeScript + Canvas 2D），`src/data/` 为回放/符号数据，`shots/` 为各分辨率截图，`tools/` 为数据提取脚本 |
| `analysis/` | Python 分析模块（`analyze.py`、`enemy/` 敌方标记跟踪及测试、绘图）与分析报告 `*.md`、结果 JSON、图表 |
| `docs/plan/` | 方案文档（v1.1 / v1.2 的 md / html / pdf）、8111 API 规格、文档构建脚本 `build/` |
| `docs/research/` | 调研笔记与截图；第三方参考项目见 [`REFERENCES.md`](docs/research/REFERENCES.md)（未纳入仓库） |
| `receiver.env.example` | 接收端环境变量模板（占位值） |
| `scripts/sync-from-workspace.sh` | 从 `/workspace` 下的工作目录重新同步到本仓库（统一排除规则 + token 脱敏） |

## 组件

- **wt-collector**（Windows 采集端）：轮询游戏本地接口 `127.0.0.1:8111`（新版客户端可能为 9222，`game_port = "auto"` 自动探测），按端点采样率采集 `map_obj` / `indicators` / `state` / `map_info` / `mission` / `hudmsg` / `gamechat` 和地图图片，仅发送各端点最新有效样本；2.0.0 使用稳定设备身份、独立地图上传通道和服务器时钟同步。
- **wt-receiver**（axum）：`POST /api/ingest`、`/api/image`（Bearer token），`GET /api/status`、`/api/time`、`/healthz`、状态页 `/`；每端点采样水位、2 秒期限、地图上下文校验、单房间状态任务和共享标绘版本校验。
- **wt-analyze**：对会话生成 `report.md` 与各类 CSV。
- **analysis/**：真实数据分析、敌方标记识别与轨迹（Python）。
- **web-proto/**：《武装突击 3》地图风格的独立陆空双图，战争雷霆原生地图符号、金色本人标记、标绘和多人实时协作。

详细说明见 [`wt-collect/README.md`](wt-collect/README.md)。

## 构建 Windows 采集端

在 Linux 上交叉编译（需要 `rustup target add x86_64-pc-windows-gnu` 和 `mingw-w64`，链接器配置见 `wt-collect/.cargo/config.toml`，静态 CRT）：

```bash
cd wt-collect
cargo build --release -p wt-collector --target x86_64-pc-windows-gnu
# 产物：target/x86_64-pc-windows-gnu/release/wt-collector.exe
```

打包发行 zip（exe + toml + 中文 README + diag.bat）：

```bash
deploy/package.sh https://wt.zuens2020.top   # 生成 dist/wt-collector-win64.zip
```

打包支持显式传入 exe 路径：`deploy/package.sh https://wt.zuens2020.top /path/to/wt-collector.exe`。token 通过 `WT_PACKAGE_TOKEN_FILE` 指向的文件注入；没有此变量时保留占位符。不要修改仓库模板或提交真实口令。

## 部署接收端（HK 服务器，https://wt.zuens2020.top）

服务器：腾讯云轻量 `wt-tactical-hk`（ap-hongkong），Caddy 反代 + systemd。

```bash
cd wt-collect
cargo build --release -p wt-receiver          # Linux x86_64
# 在服务器上（ubuntu 用户，sudo）：
sudo useradd --system --home /var/lib/wt-receiver --shell /usr/sbin/nologin wt-receiver
sudo install -d -o wt-receiver -g wt-receiver /var/lib/wt-receiver/data
sudo install -D -m 755 wt-receiver /opt/wt-receiver/bin/wt-receiver
sudo install -m 600 receiver.env /opt/wt-receiver/receiver.env   # 由 receiver.env.example 填写
sudo cp deploy/server/wt-receiver.service /etc/systemd/system/
sudo cp deploy/server/journald-wt.conf /etc/systemd/journald.conf.d/
sudo systemctl daemon-reload && sudo systemctl enable --now wt-receiver
# Caddy（自动 HTTPS）：
sudo cp deploy/server/Caddyfile.https /etc/caddy/Caddyfile && sudo systemctl reload caddy
curl https://wt.zuens2020.top/healthz
```

- receiver 只监听 `127.0.0.1:18111`，公网经 Caddy（`wt.zuens2020.top` 与公网 IP 的 HTTP）。
- `deploy/wt-dns-setup.py` / `deploy/txapi.py`：DNSPod 解析 + 启用 HTTPS 的幂等脚本，腾讯云凭据从 `~/.secrets/` 读取（不在仓库内）。
- `deploy/migrate-data.sh` / `deploy/final-sync.sh`：把旧接收端数据迁移/补传到 HK 服务器。

## Web 原型

```bash
cd web-proto
npm ci
npm run dev      # 本地开发
npm run build    # 输出 dist/（不提交）
```

## 更新仓库

各组件仍在 `/workspace/wt-collect`、`/workspace/wt-proto`、`/workspace/wt-analysis`、`/workspace/wt-plan`、`/workspace/wt-research` 中开发。同步并提交：

```bash
scripts/sync-from-workspace.sh --dry-run   # 预览
scripts/sync-from-workspace.sh
gitleaks dir . --no-banner                 # 提交前做密钥扫描
git add -A && git commit -m "sync from workspace" && git push
```

排除内容：`target/`、`node_modules/`、`dist/` 构建产物、`*.zip` / `*.exe` / `*.deb`、日志、录制数据（`data/`、spool、`*.jsonl`、测试会话目录）、> 2 MB 的图片 / 数据（plan 文档 PDF 上限 10 MB）、`__pycache__`、`.venv`、`*.env`、token / 密钥 / SSH key。
