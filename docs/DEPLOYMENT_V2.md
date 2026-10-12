# v2 部署记录（2026-10-11）

用户明确指示“部署”，已完成上线。Windows 尚未完成的原生 DPI、托盘、自启及窗口恢复验收仍保留在 [验收记录](ACCEPTANCE_V2.md) 中。本次没有开启前台窗口、置顶或自启。

- 网页：https://wt.zuens2020.top/
- 旧入口：https://wt.zuens2020.top/proto/
- 公开 Windows 安装包：https://wt.zuens2020.top/downloads/wt-collector-win64.zip
- 校验文件：https://wt.zuens2020.top/downloads/wt-collector-win64.zip.sha256
- 当前发布标识：`20261011T101357Z-map-symbols`
- 首次 v2 发布标识：`20261011T090309Z-v2.0.0`

## 已部署内容

接收端 `/opt/wt-receiver/bin/wt-receiver` 已切换；`WT_MAX_SKEW_MS=2000`、`WT_ACCEPT_LEGACY=off`，上传口令和房间口令保持原值。服务健康检查通过。静态网页通过 Linux 原子目录交换切换至 `/opt/wt-receiver/releases/20261011T090309Z-v2.0.0/web`，Caddy 配置没有改动。

公开安装包保留 `CHANGE_ME_WT_TOKEN`，不包含上传凭据。成员需在配置中填写管理员提供的上传口令。Windows 本机已通过 SSH 私有传输安装配置好的内部包：

```text
C:\Users\22595\WT-Tactical-Command\2.0.0
桌面快捷方式：WT Tactical Command 2.0
```

安装在非交互 Session 0 中执行，仅运行 `--version` 校验。程序没有启动，HKCU 自启项没有改动。配置文件 ACL 限制为当前用户、SYSTEM 和管理员；传输用的压缩包已删除。

| 产物 | SHA256 |
| --- | --- |
| Linux receiver | `28a98fc64a5c640dc931064fc646bc42ea8d964a7396363c7d357107910af80e` |
| Windows exe | `77e45c9aea722a99ef5fda03026733764c30265da4a4cab1591ed861e7804fdc` |
| 公开 ZIP | `4d29b7cf2cf6c2452e7e05907fe71ff93d5204c6886a5a319ec38c2eccd2ee23` |
| 网页 index.html | `1d90a372c3f0a356ba952d165cabec3c5dc935938db7da7d7b358c0cbdfd9559` |

## 线上验证

`/healthz`、网页和 `/proto/`、新 JS、官方类别图标及公开 ZIP 均返回 200；线上文件与本地构建哈希一致，下载 ZIP 的配置确认不含上传口令。

真实 WebSocket 确认协议 2，连续 8 条状态序号递增；不带操作身份和版本的旧标绘请求返回 `upgrade_required`、只读状态及刷新提示，没有修改标绘。1440×900 和 390×844 的无头 Chromium 检查通过，包括连接、双图八层 Canvas、设置输入、缩小可视高度后的布局及正式包排除开发入口。证据：`shots/deploymentcheck-result.json`。

接收端和 Caddy 均为 active。主房间及两个已有测试房间的当前标绘与部署前一致。历史录制文件没有删除；实时运行不新增观测或事件归档。

## 备份与恢复

原二进制、环境文件、Caddy 配置、静态网页和停服后的房间状态：

```text
/opt/wt-receiver/backups/20261011T090309Z-v2.0.0
```

原网页目录：`/var/www/wt-live.previous-20261011T090309Z-v2.0.0`。发布清单：`/opt/wt-receiver/releases/20261011T090309Z-v2.0.0/manifest.json`。备份目录权限 700，环境文件 600。

安装脚本 `wt-collect/deploy/install-live.py` 在部署异常时恢复原二进制、环境、网页和房间状态。后续人工回退前应先备份当前房间状态，再停止接收端、还原所需文件、切回原网页并重新检查服务健康；保留发布后标绘以便恢复。

发布用临时 SSH 公钥已从服务器移除，本机私钥、临时上传口令/房间口令文件和服务器上传暂存目录均已清理。源码和发布脚本保留在当前工作区，未推送 Git。

## 原生地图符号、金色本人和开局底图修复

修复版 `20261011T101357Z-map-symbols` 已发布。网页替换为游戏原生地图字形（攻击机为小扁菱形），本人方向箭头固定为金色，并按房间绑定稳定采集设备。底图比较和重绘包含图片地址、鉴权变化、地图上下文与图片就绪状态；切换新一局和图片暂时加载失败均能自动恢复。

接收端的手动新一局保留已就绪的静态地图，在没有新样本时也能继续显示底图；实时位置、成员归属和标绘不带入新一局。新鲜采样到达后正常替换地图上下文。修复版 Linux ELF 在服务器的隔离端口/目录通过健康检查后，再切换接收端和网页。

| 修复版产物 | SHA256 |
| --- | --- |
| Linux receiver | `632c78d3abff550fb22f3f03e32c36c0b52aad9ff6500cd6ea06ad6ad6181d8b` |
| 网页 index.html | `345450a6ac5164d0a9041a2013a31aba5300bef8520e05939c9dd98dc3ab7924` |
| web.tar.gz | `28eb28175f992a5d711d577d71a31436c7071f26f6dc9084ec90a774556f4d1e` |

网页目录为 `/opt/wt-receiver/releases/20261011T101357Z-map-symbols/web`；修复前二进制、网页链接目标及停服后当前房间状态备份在 `/opt/wt-receiver/backups/20261011T101357Z-map-symbols`。没有修改服务器凭据、Caddy、历史文件、公开 Windows 包、已安装的 Windows 程序或自启设置。

新增后台回归：48 项原字体比对、桌面/手机各 10 个地图与本人身份场景、双浏览器真实设置按钮开始新一局并共享切换/保留陆空底图/不复活位置；44 项 Rust 工作区测试、18 个视口/DPI组合、触控、正式包和 11 个协作场景通过。另补跑 8 采集端/8 网页端 10Hz 的 20 秒协议检查，失败 0；完整 30 分钟证据仍为首次 v2 验收结果，不宣称本修复重新做过 30 分钟实测。

HTTPS、13 项产物哈希、`/proto/` 原生攻击机图标、WebSocket v2 连续 8 条状态、旧网页只读提示及桌面/手机正式页面检查通过。Windows 公开包校验值保持原值。

复现：`node web-proto/tools/roundtest.mjs` 使用隔离测试接收端；地图与身份回归用 `tools/mapregression.mjs`，原字形比对用 `WT_NATIVE_FONT=/path/to/game/icons.ttf node web-proto/tools/icontest.mjs`。Linux 构建采用 cargo-zigbuild（GNU/glibc 2.31），执行方式参考其[官方说明](https://github.com/rust-cross/cargo-zigbuild)。补丁安装工具为 `wt-collect/deploy/install-map-patch.py`，只更新接收端和网页，包含 Linux 实际运行预检、备份和自动回退。
