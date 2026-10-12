# v2 验收记录（2026-10-11）

代码已实现，后台检查通过。**已于 2026-10-11 按用户“部署”指令上线**，详见 [部署记录](DEPLOYMENT_V2.md)。用户正在打游戏，Windows 前台验收已停止，测试窗口、临时交互任务和测试自启项已清理；完整原生 DPI、托盘、自启和窗口恢复验收仍待完成。此次发布全程后台操作，不打开窗口、抢焦点或开启置顶。

## 数据和协作

最终 Rust 工作区测试 **44 通过、0 失败、1 个已有诊断测试忽略**：采集端 15、公共库 5、接收端 24。新增回归确认手动开始新一局保留已就绪的静态地图、不复活过期位置，并能接受后续新采样；观战中的 `Player` 图标仍不会凭空推断为中坦或战斗机。

8 个模拟采集端、8 个 WebSocket 网页客户端，以真实墙钟 10 Hz 运行 **1800.004 秒 / 18000 轮**，失败 0。故障注入覆盖重复、乱序、超过 2 秒的延迟、旧会话重传、错误地图上下文、切图、进程重启、短暂断网和网页重连。

| 指标 | 结果 |
| --- | ---: |
| 有效接收样本 | 427421 |
| 拒收重复样本 | 4386 |
| 拒收乱序样本 | 3768 |
| 丢弃过期样本 | 981 |
| 地图上下文冲突 | 557 |
| 队列溢出 | 0 |
| 网页状态消息 | 143962 |
| 完整快照 / 重新同步 | 27 |
| 自身单位检查次数 | 1141776 |

检查包括状态序号严格递增、相同地图代次的位置不回退、设备身份不重复和超过 2 秒的观测不重新出现。测试目录仅新增静态图片缓存与当前房间状态，没有新增原始观测或事件归档。

两端真实浏览器协作的 11 项场景通过：创建、并发编辑、删除竞态、确认丢失、广播遗漏、拒绝撤销队友修改、撤销自己的更新、离线禁用提交、重连、忽略旧连接回调以及和服务器完整快照一致。实测至少 1 次版本冲突拒绝，首次验收两个页面分别触发 3 次重新同步；本修复重跑分别触发 2 / 4 次重新同步。

证据：`shots/loadtest-30min-result.json`、`shots/synctest-result.json`。证据和截图是本地验收产物，不进入 Git。

## 网页、绘制与图标

18 个组合通过：1440×900 / DPR 1、1280×720 / DPR 1.5、1536×864 / DPR 2、390×844 / DPR 3、844×390 / DPR 2、320×568 / DPR 2，分别检查陆战、空战和双图。

验证内容包括无页面溢出、默认收起详情、每张地图四层 Canvas、可见按钮至少 44 px、矩形底图等比例投影、坐标往返、以鼠标为中心缩放、双图独立操作、标绘命中和米制测距。触控测试覆盖单指平移、双指缩放、标绘点击以及禁止将拖动误识别为标点。

用户纠正后移除原先错误选用的 Wiki 剪影，改为游戏内置地图的 10 个原生字形、12 个已核验类别映射。来源清单：`web-proto/public/icons/map/sources.json`。攻击机采用小扁菱形；空字形及未核验类别使用中性符号并保留类别文字。

正式构建通过，并从独立 `file://` 页面验证双图八层 Canvas、全部 10 个字形 SVG 可加载以及开发测试入口不会出现在正式包中。运行时使用内嵌的原生矢量路径，无字体/图标下载等待。

48 项原字体比对通过（12 类 × DPR 1/1.5/2/3），允许字体小尺寸 hinting 带来的 1 个设备像素差异；中性兜底与 128 项缓存上限检查通过。桌面 1440×900、手机 390×844 各 10 个底图/身份场景通过：昵称不同仍能标金色本人、延后补齐图片地址、鉴权变化、网页开始新一局、同尺寸换图、仅上下文变化、图片加载重试、稳定设备重启、手工绑定及观看模式。证据：`shots/icontest-result.json`、`shots/mapregression-result.json`。

8 个无头 Chromium 页面进行长时间绘制与堆内存采样，4 个桌面、4 个手机视口，双图、DPR 1/2。运行 1800.761 秒，60 次采样，无页面异常，图标缓存未超过 128。各页最终 GC 后 JS 堆为 4.33–4.54 MB；全部采样中的最大单次绘制耗时为 10.6 ms，最后一个运行段平均绘制耗时为 0.094–0.118 ms。完整测量结果保存在 `shots/browser-soak-result.json`。开发服务器发生过热更新，因此这项结果仅作为分段绘制/内存证据，不能宣称同一 JS 实例连续 30 分钟无泄漏。上面的 30 分钟协议负载测试不受开发服务器热更新影响。

## Windows 和发行包

Windows x64 MSVC release 构建通过，版本输出 `2.0.0`；静态 CRT 构建完成，生成 `wt-collect/dist/wt-collector-win64.zip`。打包工具校验 x64 PE、移除固定个人身份，并保留上传口令占位符。实际发布时从口令文件读取服务端配置。

公开发行 ZIP SHA256：`4d29b7cf2cf6c2452e7e05907fe71ff93d5204c6886a5a319ec38c2eccd2ee23`。公开包保留上传口令占位符；Windows 本机通过私有传输安装了配置好的内部包，临时压缩包已删除。

已有 Windows 中文界面与长文本截图：`shots/windows/ui-evidence/scale-1.png`、`scale-1.5.png`、`scale-2.png` 和对应诊断页。这是应用内缩放 100% / 150% / 200% 的局部视觉检查，**不是 Windows 原生系统 DPI 的完整验收**。托盘、自启、窗口恢复的交互脚本未完成最终汇总；不将其算作通过。

仍待用户不在游戏时完成：Windows 原生 DPI、长错误提示、实际托盘操作、自启及窗口恢复。不要自动安排或启动交互验收。

## 发布状态与复现

Linux x64 receiver release 已部署，保存在 `wt-collect/dist/wt-receiver-linux`，当前修复版 SHA256 `632c78d3abff550fb22f3f03e32c36c0b52aad9ff6500cd6ea06ad6ad6181d8b`。接收端使用 2000 ms 期限和 `WT_ACCEPT_LEGACY=off`；原上传口令、房间口令和历史文件保留。旧二进制、环境、网页和房间状态已备份；临时 SSH 公钥、本机私钥和临时凭据文件已清理。

从项目根目录执行后台检查：

```sh
cargo test --manifest-path wt-collect/Cargo.toml --workspace
npm --prefix web-proto run build
node web-proto/tools/qa.mjs
node web-proto/tools/productiontest.mjs
node web-proto/tools/mapregression.mjs
WT_NATIVE_FONT=/path/to/game/icons.ttf node web-proto/tools/icontest.mjs
node web-proto/tools/touchtest.mjs
node web-proto/tools/synctest.mjs
node web-proto/tools/roundtest.mjs
node web-proto/tools/loadtest.mjs 1800
node web-proto/tools/browser-soak.mjs 1800
```

浏览器脚本需要本地开发服务器 `127.0.0.1:4180`，协议和协作脚本还需要隔离测试 receiver `127.0.0.1:18112`（上传口令 `synthetic-load-upload`，房间口令 `synthetic-load-view`，房间 `load`）。测试 receiver 必须使用独立数据目录，不得连接现网进行故障注入。可通过 `WT_TEST_API` / 脚本 URL 参数改用其他本地端口，`CHROME_PATH` 指定浏览器可执行文件。

用户指示发布后，已按服务器 → 新采集端发行包 → 新网页的顺序部署。线上 HTTPS、资源哈希、官方图标、WebSocket v2、旧网页只读提示及桌面/手机浏览器检查通过。Windows 采集端 2.0.0 已安装并创建桌面快捷方式，没有启动或修改自启设置。
