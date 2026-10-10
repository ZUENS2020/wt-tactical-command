## 5. 8111 采集与双地图同步

### 5.1 使用的本地接口（全量）

完整字段、示例与来源见**附录 A《8111 接口规范》**。Agent 对全部端点轮询并全部上传，服务器负责解析、融合与存储：

| Endpoint | Agent 轮询 | 上传形式 | 服务器用途 |
|---|---|---|---|
| `GET /map_obj.json` | 10Hz | 观测帧（全部对象量化）+ 原始批次 | 本人位置 / 朝向、友军、敌军、目标、机场、重生点；融合与策略过滤 |
| `GET /map_info.json` | 1Hz（切换期 2Hz） | 观测帧头部 + 原始批次 | 坐标变换、视图切换检测、战局识别 |
| `GET /map.img?gen=` | generation 变化或首次有效 | HTTPS PUT（原始字节 + 哈希） | 双图底图、dHash 地图识别、陆战中空战视图识别 |
| `GET /indicators` | 10Hz | 原始批次（关键字段进观测帧：army、type、compass） | 本人兵种 / 载具、朝向交叉校验、诊断 |
| `GET /state` | 10Hz（`valid:false` 时 1Hz） | 原始批次（高度、速度进观测帧） | 飞机高度 / 速度标签、诊断 |
| `GET /mission.json` | 1Hz | 原始批次 + 状态变化事件 | 战局开始 / 结束（BATTLE-01） |
| `GET /gamechat?lastId=` | 1Hz | 原始批次（增量记录） | 存档；Sim 位置呼叫解析留作后续 |
| `GET /hudmsg?lastEvt=&lastDmg=` | 1Hz | 原始批次（增量记录） | 存档；不做战损产品化 |
| `GET /loc/map/*?fmt=js` | 每局 1 次 | 原始批次 | 存档 |

**Agent 只做三件事：**正确轮询（不重叠、超时、游标持久化）、本地识别本人对象并量化、可靠上传。Agent **不做任何敌我过滤**；阵营判定、融合、显示策略都在服务器完成，便于统一升级与回放复算。

**研究修正的关键事实（详见附录 A）：**

- 空战试飞的 `map.img` 是 128×128 灰色问号占位图（WTP 哈希表中的 `no_map`），不能据此推断实战空战图尺寸。
- 陆战玩家驾驶飞机时，8111 底图换成“陆战中的空战视图”（WTP 收录 52 张），对象坐标整体重新归一化；必须按视图分别换算。
- `map_generation` 是各客户端本地计数；`map_info` 数值可能是数字字符串。
- 敌我只能靠颜色判定；绿色为小队友军；玩家可自定义颜色。
- Air SB 中队友飞机不出现在 map_obj；普通“注意地图”不产生对象。

### 5.2 10Hz 是“有效数据”目标

现有证据：WTM 在一台机器上以约 9Hz 顺序请求 state / indicators / map_obj，平均响应约 3ms、0 错误，飞行中 99/99 次响应都有变化；游戏内置页面仅 2Hz；WP8F 地图采样约 4Hz、L18 为 10Hz。陆战与 8 人实战仍需实测。

- **Agent 轮询：**快组 100ms，慢组 1s；同一端点请求不重叠；每个端点独立记录状态、最后成功时间与错误。
- **Agent → Server：**观测帧最多 10Hz；若 map_obj 与上一帧完全相同，只发心跳型短帧（保留 sampledAt），不冒充新样本。
- **Server → Web：**10Hz 合并全队快照（经策略过滤）；标绘修改事件立即广播。
- **Web 渲染：**尝试 60FPS，低端机可降至 30FPS；对有效位置做 100–150ms 缓冲插值，过期后停止预测。
- **数据质量：**每个状态带 sampledAt（服务器时间线）、seq、sampleAgeMs；超过 500ms 显示“数据延迟”，超过 2s 停止外推（阈值按实测调整）。

### 5.3 双地图投影与陆空变换

每套底图独立携带：imageHash（SHA-256）、dHash、width/height、map_min/max、hud_type、viewKind（ground / air / air_in_ground）、providerId、capturedAt、battleEpoch。

```code
worldX = mapMinX + x * (mapMaxX - mapMinX)
worldY = mapMinY + y * (mapMaxY - mapMinY)
heading = atan2(dx, -dy)       // 已验证与 compass 一致（附录 A §5.3）
```

- 所有自定义标点、线路和区域只保存同一世界坐标系，避免双图重复存储和投影漂移。
- **自动陆空变换：**全量上传后，服务器同一时刻同时拥有陆战视图提供者与空战视图提供者看到的同一批地面对象（占领点、重生点、坦克）。服务器按 icon / 颜色匹配并以最小二乘拟合 `x_air = a·x_ground + b`、`y_air = c·y_ground + d`（借鉴 WTP：至少 3 点、缩放上限 0.5、残差上限 0.01），作为世界坐标换算的交叉校验；若两图的 map_min/max 已能直接对齐，则拟合结果用于验证误差。
- 陆战玩家切入“陆战中空战视图”时，其观测帧标注 `viewKind=air_in_ground`，服务器用该视图的范围或拟合变换换算后再参与融合，不覆盖陆战底图。
- 空战地图更大，地面标点可能变得密集；空战总览中按缩放级别隐藏次要标签，但保留图形。
- 地图超出范围的标记不强行画到边缘；在世界范围外时直接不显示，或者使用明确的“范围外”提示。
- 每场首次联机前，使用同一已知位置至少 3 个校准点做地空互投验证，输出投影误差（MAP-02），与自动拟合结果对比。

### 5.4 数据来源切换与战局识别

1. 所有 Agent 在 generation 变化时上传地图图片；服务器按哈希去重，计算 dHash 并与 WTP 地图哈希表（229 条）匹配出地图名与 viewKind。
2. 主陆战 / 空战底图只能由 viewKind、战局身份、范围 / 图片校验一致的提供者确定；`no_map` 占位图永不作为底图。
3. 提供者失联时保持最后一张已确认属于当前战局的地图；友军过期状态单独处理。
4. 房间新战局开始时创建新 battleEpoch，上一战局标绘默认归档，不直接覆盖到下一局。

**战局识别（BATTLE-01，独立 Issue）：**

- **自动检测：**以各 Agent 的 mission.json 状态转换与 map_info.valid 变化为触发信号（失效后 20s 宽限期，借鉴 WTP），再用地图哈希与 map_min/max 在多个提供者之间交叉匹配，确认同一张地图、同一场战局后才创建新 battleEpoch。核弹导致的“陆战图 → 同名空战图”切换不视为新战局。
- **提供者不一致：**状态转换、图像哈希或范围互相矛盾时不自动切换；网页弹出简短的人工确认提示（新战局 / 仍为当前战局），任何房间成员均可确认。
- **确认前：**继续显示上一份已确认的地图与标绘，顶栏提示“战局待确认”。
- 识别规则与阈值以原始批次实测为准，原始批次直接作为回归测试夹具。

### 5.5 “本人”状态与缺失处理

本人 = `icon=="Player"`；本人对象的 `type`（`ground_model` / `aircraft`）与 `/indicators.army` 直接给出当前兵种。

| 状态 | 典型场景（判定规则以 DATA-04 实测为准） | Agent 行为 | 网页显示 |
|---|---|---|---|
| `alive` | 可识别本人图标，视图与当前底图一致 | 正常上报位置 / 朝向 | 蓝色实时友军图标 + 昵称 |
| `dead_or_spawning` | 本人图标消失；重生 / 选车界面 | 上报状态 + 最后有效位置时间戳 | 灰色最后已知位置 + 数据年龄 |
| `spectating` | 阵亡后观战，地图对象可能来自其他视角 | 上报状态；不得把观战对象误当成本人 | 同上 |
| `in_air_vehicle` | 陆战玩家驾驶飞机（Player.type=aircraft，视图切为陆战中空战视图） | 上报状态与 viewKind；不再作为陆战底图提供者 | 按换算结果显示；无法换算时灰显 |
| `unknown` | 无法判定 | 上报 unknown，而不是不发消息 | 灰色最后已知位置 + 年龄 |

- 状态变化立即上报；没有有效位置时仍保持低频心跳，使服务器能区分“Agent 在线但本人缺失”和“Agent 失联”（超过 2s 无消息）。
- 本人在“注意地图”后约 5s 变为蓝色闪烁，属于正常现象，不改变状态。
- 本人状态只用于正确显示友军图标，不生成死亡标记、不做战损。

### 5.6 服务器融合

1. **解析：**每个观测帧按该 Agent 的视图参数换算为世界坐标；颜色按附录 A §4.5 规则判定阵营（支持按 Agent 校准调色板）。
2. **本人轨迹：**每个 Agent 的本人对象形成带身份（昵称、载具 type、兵种）的友军轨迹，优先级最高。
3. **友军去重：**其他 Agent 看到的友军对象若与某 Agent 本人轨迹在世界坐标上足够接近（阈值按陆 / 空分别设定，实测确定），视为同一人，不重复显示；其余友军作为匿名友军轨迹（例如未运行 Agent 的队友）。
4. **敌军与目标：**多来源观测按“阵营 + 类别 + icon + 世界坐标最近邻 + 速度门限”关联为轨迹，使用观测时间计算速度；占领点、机场、重生点作为静态对象合并。
5. **消失处理：**目标从所有来源消失后，轨迹保留最后位置与年龄；`fused` 策略下飞机可生成 Neo 式预测椭圆（15s 衰减），地面目标仅淡出。
6. **输出：**每 100ms 生成 FusedSnapshot，按 `enemyPolicy` 过滤后下发；原始观测永不因策略而丢弃。

### 5.7 敌情显示策略（enemyPolicy）

| 策略 | 下发给网页的自动敌情 | 适用 |
|---|---|---|
| `off`（默认） | 不下发任何由 8111 推导的敌方对象；人工敌情标绘不受影响 | 首次实战测试、无标记模式、正式比赛 |
| `team_visible` | 只下发**当前**至少一名 Agent 的 8111 中可见的敌方对象，不外推、目标消失即移除 | 游戏本身在地图上显示敌方标记的模式 |
| `fused` | 融合轨迹 + 消失后的年龄显示与空中目标预测椭圆 | 训练、自定义对局、复盘分析 |

- **模式上限：**服务器根据战局识别结果（地图类型、hud_type、对象可见性特征）给出 `modeCap`；当判断为无标记模式（如模拟战）时，默认上限为 `off`。上限值为服务器配置项，可按需调整。
- **变更可见：**任何成员都能修改策略（与无角色差异一致），变更立即广播、在顶栏徽标显示，并写入 `policy_events`（操作者、时间、旧值、新值）。
- **与存储无关：**策略只影响显示；原始数据与融合结果照常存储，便于事后分析。

## 6. 房间数据与通信协议（建议草案）

### 6.1 自定义标绘模型

```code
{
  "id": "ann-89c1",
  "roomId": "TEAM-07",
  "battleEpoch": "2026-10-10-b2",
  "layerId": "layer-battle",
  "kind": "symbol",
  "sidc": "10061010001211000000",
  "status": "planned",
  "timeLabel": { "mode": "battle", "t": 330 },
  "scope": "both",
  "geometry": { "point": { "x": 1430.2, "y": 2260.1 } },
  "label": "西侧目标",
  "createdBy": "member-03",
  "updatedAt": 1791600000123,
  "version": 1
}
```

kind 取 `symbol | arrow | line | area | circle | ruler | source | text | route_curve(预留)`。`line` 带 `lineType`（phase / boundary / departure / generic）；`area` 带 `areaType`（ao / objective / airstrike / recon / danger）。`status` 为 `present | planned`；`timeLabel.mode` 为 `battle`（对局秒数）或 `clock`（UTC 毫秒，网页显示北京时间）。颜色和图形由固定映射表与 SIDC 决定，label 只按文本渲染。`version` 由服务器在每次确认编辑时递增。

**进攻箭头（首轮实现）：**

```code
{ "kind": "arrow", "geometry": { "points": [[1200,2600],[1440,2480],[1580,2300]], "width": 120 }, "status": "planned" }
```

**曲线路线（预留格式，P5）：**锚点 p、进入控制点 in、离开控制点 out，两锚点之间三次 Bézier；首轮协议保留结构，服务端可暂不接受。

```code
{ "kind": "route_curve", "geometry": { "segments": [
  { "p": [1200,2600], "in": null, "out": [1280,2560] },
  { "p": [1440,2480], "in": [1360,2500], "out": null } ] } }
```

**攻击来源：**保存受袭点、估计方向角、可选扇区宽度；只代表成员人工判断。

### 6.2 WebSocket 消息（Protobuf `WsEnvelope` oneof）

**Agent 通道 `/ws/agent`：**

| 消息 | 方向 | 用途 |
|---|---|---|
| `JoinRequest` / `JoinResponse` | 双向 | 设备令牌、协议版本、`server_time_ms`（沿用 Neo） |
| `ObservationFrame` | Agent → VPS | 10Hz：seq、observed_at_ms、viewKind、map 参数摘要、本人（x_u16、y_u16、heading×100、type、army、高度、速度）、本人状态、**全部对象**（稳定 local_id、icon 枚举、type 枚举、RGB、blink、坐标、heading）、removed_local_ids |
| `RawBatch` | Agent → VPS | 1Hz：该秒内所有端点的原始响应（端点、采样时间、HTTP 状态、耗时、原始字节），zstd 压缩 |
| `MapImageNotice` | Agent → VPS | generation、SHA-256、尺寸；服务器未持有该哈希时 Agent 再 PUT 原始图片 |
| `AgentStatus` | Agent → VPS | 端点健康、轮询频率、游标、版本 |
| `RequestKeyframe` / `Ack` | VPS → Agent / 双向 | delta 模式下的全量重发（P3 启用） |
| `Ping` / `Pong` | 双向 | NTP 式时钟同步（5–10s 一次，5 样本中值） |

**网页通道 `/ws/web`：**

| 消息 | 方向 | 用途 |
|---|---|---|
| `WebJoin` / `RoomSnapshot` | 双向 | 加入后全量同步：标绘、图层、地图元数据、策略、成员 |
| `FusedSnapshot` | VPS → Web | 10Hz：带身份友军、匿名友军、（按策略）敌方轨迹与 ROI、静态目标、数据年龄 |
| `MapChanged` / `BattleConfirm` | VPS ↔ Web | 底图变更；提供者不一致时请求 / 提交人工确认 |
| `AnnotationOp`（create / update / delete / undo） | Web → VPS | 标绘编辑，带 operationId |
| `AnnotationApplied` | VPS → Web | 服务器定序确认并广播新版本号 |
| `LayerOp` / `LayerApplied` | 双向 | 图层增删改、锁定、隐藏（P2） |
| `PolicySet` / `PolicyChanged` | 双向 | 敌情策略变更与广播 |
| `PresenterView` | 双向 | 演示者视口广播与跟随（P2） |
| `Ping` / `Pong` | 双向 | 心跳与延迟监测 |

- 标绘命令携带 operationId 以防断线重试造成重复创建；服务器按到达顺序定序，后写覆盖，被覆盖的修改可撤销恢复。
- 服务端验证坐标范围、字符串长度、最大节点数、消息体大小与频率；标绘 CRUD 与 10Hz 快照分开。
- 协议版本号放在 Join 消息中；proto 文件只允许追加字段，CI 用 `buf breaking` 检查兼容。
- 后端不读取 uiMode 决定权限。

### 6.3 存储

| 表 / 存储 | 保存内容 | 保留 |
|---|---|---|
| `rooms` | 房间配置、口令 argon2 哈希、`enemyPolicy`、创建 / 过期时间 | 长期 |
| `devices` | Agent 设备令牌哈希、昵称、最后在线 | 长期 |
| `battles` | battleEpoch、地图识别结果、开始 / 结束、提供者 | 长期 |
| `map_assets` | 图片 SHA-256、dHash、尺寸、viewKind、范围、提供者、首次出现战局；文件在磁盘按哈希存放 | 长期（去重后体积小） |
| `annotations` / `layers` | 当前标绘与图层 | 长期，按战局归档 |
| `annotation_events` | 编辑 / 删除 / 撤销历史 | 90 天 |
| `policy_events` | 敌情策略变更记录 | 长期 |
| Raw Store | 每个 Agent 每场战局一个分段文件：全部 `ObservationFrame` 与 `RawBatch`（zstd） | 默认 14 天，可配置；手动标记的战局可永久保留 |
| 融合快照 | 每 1s 抽样一份 FusedSnapshot 写入同一战局目录 | 同 Raw Store |

10Hz 实时状态仍以内存为准；原始数据顺序追加写文件，不写 SQLite。

## 7. 香港 VPS 部署方案

```diagram
{{deploy}}
```

### 7.1 建议资源

- 从 2 vCPU / 2–4GB RAM / **60–80GB SSD** 开始（全量原始数据保留需要额外磁盘），Linux，无 GPU。
- 优先香港机房；香港线路到大陆的 RTT / 抖动要用实际连队成员网络测试。
- 本期 8 个 Agent + 若干网页，性能瓶颈更可能来自 8111 有效刷新和公网 RTT，而非 CPU；融合计算为 O(对象数²)，8×40 对象规模可忽略。
- **HTTPS 采用域名方案：**购买域名并解析到香港 VPS（无需 ICP 备案），Caddy 自动签发并续期证书；不使用 IP 地址证书。
- 只开放 443；8111 永远仅由本地 Agent 读取，不得映射公网。

### 7.2 带宽与存储容量（估算，不是实测）

| 项目 | 假设 | 估算 |
|---|---|---|
| 单个观测帧 | 40 个对象 × 约 16B + 头部与本人约 80B | 约 0.7KB |
| 观测上行 | 0.7KB × 10Hz × 8 Agent | 约 56KB/s ≈ 200MB/小时 |
| 原始批次上行 | 每 Agent 每秒原始 JSON 约 60–100KB（map_obj 10 次 + indicators 10 次 + state 10 次 + 慢组），zstd 压缩约 8–12 倍 | 每 Agent 约 6–12KB/s；8 个约 50–100KB/s ≈ 180–360MB/小时 |
| 地图图片 | 每局 2–4 张不同底图 × 约 0.3–1MB，按哈希去重 | 每局 < 4MB |
| 单个 Agent 上行合计 | 观测 + 原始 | 约 15–20KB/s，家庭宽带可忽略 |
| 快照下行 | 约 40 条轨迹 × 20B + 头部 ≈ 1KB × 10Hz × 8–12 个网页 | 约 80–120KB/s ≈ 290–430MB/小时 |
| 磁盘写入 | 观测 + 原始批次（已压缩）约 100–160KB/s | 约 0.4–0.6GB/小时；每天 3 小时对局约 1.2–1.8GB；14 天约 17–25GB |

如实测超出预期：先把原始批次中的 map_obj 降为“仅变化时”或 5Hz，再启用观测帧 delta（只发变化对象 + removed_local_ids），最后缩短保留期。

### 7.3 基础安全与可用性

- 房间码只是入口标识；网页用房间口令换取短期会话令牌，Agent 用一次性配对码换取设备令牌；**WebSocket 升级前必须校验令牌**（修正 Neo 缺陷）；口令 argon2id 存储；房间之间数据严格隔离。
- Agent 的 HTTP 目标写死 127.0.0.1:8111；Agent 本机状态页默认只绑定 127.0.0.1（Neo 默认 0.0.0.0:17711，需改）。
- 全量原始数据包含聊天文本与玩家昵称：Raw Store 不提供公开下载接口，只能通过服务器本地工具或带房间令牌的导出接口读取；日志不记录原始负载；到期自动删除。
- 限制标绘更新频率、地图文件大小（默认 8MB，沿用 Neo 配置项）、异常 Protobuf、恶意文本；网页对标签文本转义。
- Caddy 自动 TLS、systemd 自动重启、SQLite 每日备份、Raw Store 与地图缓存定期清理。
- 单实例 VPS 重启后：标绘从 SQLite 恢复、实时位置等待 Agent 重新上报、网页自动重连；Agent 在断线期间本地缓冲最近 60s 原始批次，重连后补传。
- **使用边界：**全量采集只读取游戏自身开放的 8111 接口，不读内存、不注入、不处理画面。自动敌情显示受 `enemyPolicy` 控制，事实依据见第 10 章合规条目。

## 8. 开发路线、排期与依赖

<div class="note" markdown="1">
工期以 1 名熟悉 Rust / React 的开发者为基准，复用 Neo 后估计约 **3.5–5 周**（v1.1 为 4–6 周）：Agent 代理、服务器房间 / WS / 地图接口 / Protobuf 骨架、8111 类型与文档可直接继承；新增工作集中在全量上传、存储、融合修正、敌情策略和 TacOps 式标绘。实测中的陆空坐标、8111 样本率问题仍可能延长排期。
</div>

| 阶段 | 时间（估计） | 任务 | 交付物 / 退出条件 |
|---|---|---|---|
| M0 第一周竖切 | 3–4 天 | fork Neo；Agent = Neo 代理 + Rust 轮询上传（全部对象 + 本人）；服务器 = Neo cloud-tactical-server + WS 鉴权修正 + 地图图片；网页显示陆战图、本人位置与仅点标；部署域名 HTTPS/WSS | 真实对局中本人位置出现在手机浏览器的陆战图上，点标可在手机与 PC 间同步；原始批次已落盘 |
| P0 数据可行性 | 2–3 天（与 M0 后半并行） | 按 WTM 抓包规程覆盖陆 / 空 / 陆战中驾驶飞机 / 阵亡 / 观战；样本率统计；颜色调色板；陆空拟合；地图哈希验证 | 有效样本率报告、映射误差、本人状态判定规则、调色板、附录 A 未决问题逐项结论 |
| P1 地图与标绘内核 | 4–5 天 | V10 UI 正式化；milsymbol 军标；进攻箭头、阶段线、区域、圆、测距尺、来源；present / planned、时间标签；地空投影 | 所有主流尺寸无滚动且 1:1；单图可编辑，双图可查看 |
| P2 全量同步与存储 | 3–4 天 | 8 Agent；Raw Store；SQLite；融合重写（世界坐标、本人身份、友军去重）；自动陆空变换；图层 | 8 Agent + 8 网页持续 30 分钟；同一友军不重复；原始数据可回放 |
| P3 协同、策略与可靠性 | 3–4 天 | 定序 + 撤销、重连恢复、战局识别、enemyPolicy + modeCap、简报 / 演示跟随、输入限制、可选 delta | 断线不丢标绘；不跨战局串图；策略切换即时生效 |
| P4 连队实战测试 | 3–5 天 | 香港网络 RTT / P95、手机横竖屏、空地混合战、带宽与磁盘实测、回滚演练 | 8 人走通一场完整连队战；问题清单收敛 |
| P5 / 下一轮 | 首次实战测试后另行评估 | 钢笔曲线与平滑箭头、军标编辑器、回放 UI、冲突提示、Yjs 协同评估 | 独立评审后排期，不计入上述工期 |

### 8.1 第一周具体 Issue 列表（围绕第一周竖切）

| 编号 | Issue | 优先级 |
|---|---|---|
| FORK-01 | 建立仓库：以 git subtree 引入 Neo 上游；迁移 `src-tauri` → `apps/agent`、`cloud-tactical-server` → `apps/server`、`wt8111.ts` → `apps/web` | P0（首要） |
| SLICE-01 | Agent：在 Neo 代理中加入 Rust 轮询器（快组 10Hz / 慢组 1Hz）与 WS 上传，发送全部对象 + 本人；本机状态页仅绑定 127.0.0.1 | P0（首要） |
| SLICE-02 | 服务器：WS 升级前令牌校验、argon2 口令；陆战地图图片按哈希存储；网页仅点标的创建、广播与重连恢复 | P0（首要） |
| PROTO-01 | 新 proto：在 Neo envelope 上扩展 ObservationFrame（viewKind、本人状态、RGB、icon 枚举）、RawBatch、Annotation、Policy；prost + protobuf-es 生成；`buf breaking` 检查 | P0 |
| INFRA-01 | 域名 DNS + Caddy 自动 TLS、systemd / Docker；Raw Store 目录与清理任务；备份 | P0 |
| DATA-01 | Raw Store 写入与回放工具（按 Agent / 战局读取原始批次，导出 JSONL）——即 8111 原始采集工具 | P0 |
| DATA-02 | 用原始批次统计各端点新样本比例、响应时间分布（移动 / 静止、陆 / 空） | P0 |
| DATA-03 | 验证陆战、空战、陆战中空战视图与 map_info / map.img 的对应关系；记录图片尺寸与 dHash | P0 |
| DATA-04 | 录制本人状态样本：阵亡、重生 / 选车、观战、陆战玩家驾驶飞机、回机库 | P0 |
| MAP-01 | 世界坐标转换、陆空自动拟合与往返测试 | P0 |
| UI-01 | 将 V10 的 1:1、无滚动布局规则写成可复用组件与尺寸测试 | P0 |

### 8.2 后续具体 Issue 列表

- **AGENT-01：**本人识别、显式本人状态、稳定 local_id 分配（最近邻关联）、游标持久化与回退检测。
- **AGENT-02：**Windows Agent 分发：图文安装说明（SmartScreen / 杀软），评估代码签名。
- **AGENT-03：**RawBatch 打包与 zstd 压缩、断线 60s 本地缓冲补传、带宽自适应（原始 map_obj 降频）。
- **SERVER-01：**房间、设备配对、会话令牌、WSS 心跳与全量快照。
- **SERVER-02：**10Hz 快照总线与标绘即时事件总线。
- **SERVER-03：**SQLite 标绘持久化、定序与版本号、后写覆盖、软删除与撤销。
- **SERVER-04：**Raw Store 分段写入、保留期清理、受控导出。
- **FUSION-01：**世界坐标融合、本人带身份轨迹、友军去重、敌军关联、`fused` 模式 ROI（修正 heading 单位）。
- **POLICY-01：**enemyPolicy、modeCap、策略徽标与 policy_events。
- **COLOR-01：**阵营判定与按 Agent 调色板校准，覆盖附录 A 颜色表与自定义颜色。
- **BATTLE-01：**战局识别：mission / valid 转换 + dHash（引入 WTP 哈希表）+ 范围匹配；核弹切换识别；不一致时人工确认。
- **MAP-02：**同一战局 3 个位置人工校准，与自动拟合对比。
- **MAP-03：**图片 / 元信息加载、viewKind 状态、地图过期提示。
- **SYM-01：**milsymbol 集成；8111 icon → SIDC 映射；约 30 个常用军标与收藏。
- **DRAW-01：**进攻箭头、阶段线 / 边界、区域（多边形 / 矩形 / 圆）、文字。
- **DRAW-02：**测距尺（分段距离、方位、ETA）与“共享为标绘”。
- **DRAW-03：**present / planned 样式与时间标签开关。
- **UI-02：**单图工具栏：选择 / 军标 / 箭头 / 线 / 区域 / 测距 / 来源。
- **UI-04：**移动端手势冲突处理与触控操作条。
- **UI-05：**友军灰显最后已知位置与数据年龄；自动敌情虚线框与年龄。
- **LAYER-01（P2）：**图层增删改、锁定、隐藏、排序。
- **BRIEF-01（P2）：**简报只读布局、演示视口广播与跟随 / 接管。
- **QA-01：**网络抖动、断线重连、提供者失联、视图切换、本人状态切换、策略切换、横竖屏回归；用 Raw Store 回放做确定性测试。
- **PEN-01（P5）：**钢笔曲线与平滑进攻箭头。

## 9. 验收标准（Definition of Done）

### 9.1 UI

- 页面在 320×568、390×844、844×390、1280×720、1536×698、1440×900 下无网页级横纵滚动；手机浏览器地址栏不计入网页高度。
- 所有地图边长相同方向像素差 ≤1px；图片不变形；可独立缩放 / 平移。
- 电脑单图最大化占据顶栏以下可用高度，右侧只出现当前工具；双图完全没有右操作栏。
- 手机竖屏双图同时存在，横屏允许并排；单图的工具始终可达。
- 军标点、进攻箭头（折线骨架）、阶段线、区域、圆、测距尺与来源标注均可用触屏创建、编辑、撤销、删除；present / planned 与时间标签显示正确；钢笔曲线不属于首个版本的验收范围。
- 切换陆战 / 空战 / 双图或指挥 / 成员 UI，不会丢失地图标记和操作权限。

### 9.2 数据与协同

- 8 台 Agent 同时全量上传：观测帧包含全部 map_obj 对象，RawBatch 覆盖全部端点；服务器可按时间回放任一 Agent 的原始响应，且与 Agent 本地日志逐条一致。
- 本人位置、朝向在 8 台采集端可用且带昵称；同一名友军不会因为被多个 Agent 看到而重复显示；不会把观战对象或其它对象误当成本人。
- 本人图标缺失或视图切换时，Agent 上报明确状态（alive / dead_or_spawning / spectating / in_air_vehicle / unknown），网页灰显最后已知位置并标注数据年龄。
- 陆空投影误差可量化：自动拟合与人工 3 点校准的误差在约定容差内；图像不匹配或 `no_map` 占位图时阻止叠加。
- 完整单场 8 人测试中，目标 10Hz 网络快照、p95 新有效数据到可见 UI 的时延目标 ≤250ms（受 8111 实际刷新与网络条件约束）。
- Agent 失联超过 2s 不再预测位置；地图提供者断线时不立即删除已校验底图。
- 每次标绘编辑均有服务器定序与版本确认；并发编辑按后写覆盖处理且可撤销；断线重连后与服务端一致；跨战局不会串标记。
- 新战局可自动识别；提供者不一致时网页提示人工确认，确认前不切换地图。
- **敌情策略：**`off` 时 FusedSnapshot 中不含任何自动敌方对象（抓包验证）；`team_visible` 时只含当前可见对象且消失后 1 个快照周期内移除；策略变更 ≤200ms 广播到全部网页并记录 policy_events；modeCap 生效。
- 原始数据按保留期自动清理；无令牌无法读取 Raw Store；日志不含原始负载。

### 9.3 上线 / 回滚

- 通过域名的 HTTPS / WSS 可从至少两种手机网络访问；Caddy 证书自动签发 / 续期已验证，域名与证书到期有监控提醒。
- 单 VPS 重启可恢复房间标绘；静态资源有缓存和更新策略。
- 保存上一版稳定构建；服务器程序更新失败可以快速回滚；proto 变更向后兼容（旧 Agent 可连新服务器）。
- 实测带宽与磁盘增长不超过 7.2 估算的 1.5 倍，或已按 7.2 降级步骤调整。
- Windows Agent 附带一页图文安装说明，队员可按说明处理 SmartScreen / 杀毒软件拦截并完成首次启动。

## 10. 风险与对应策略

<div class="risk" markdown="1">

| 风险 | 影响 | 应对 |
|---|---|---|
| 8111 不产生 10Hz 有效坐标（陆战尤甚） | 位置卡顿 | WTM 空战已测约 9Hz 有效；陆战用 Raw Store 统计；显示真实样本率，插值和降级 |
| 陆战中驾驶飞机导致视图与坐标整体切换 | 位置跳变、底图被替换 | viewKind 标注；按视图换算；WTP 式自动拟合；不让该视图覆盖陆战底图 |
| 空战底图不可得（试飞为占位图）或飞行员不选空战地图 | 缺少空战底图 | dHash 识别 `no_map` 并拒绝；缓存已确认底图；提示需要空战提供者 |
| 地空世界坐标不一致 | 标注位置偏移 | 自动拟合 + 人工校准，误差超限不叠加 |
| 敌我颜色判定错误（自定义颜色、色盲模式、绿色歧义） | 友军被当成敌军或反之 | 服务器端判定 + 按 Agent 调色板校准；原始颜色存档可复算；COLOR-01 实测 |
| Neo 原型代码缺陷（鉴权、融合、schema 漂移） | 安全与正确性问题被继承 | 4.2 清单逐项修正并加测试；只 fork 结构、重写关键模块；proto 代码生成 + CI |
| 上游 Neo 持续变化 | 合并成本 | git subtree 固定版本；只在明确需要时选择性同步 |
| 全量原始数据量超预期 | 带宽 / 磁盘不足 | 7.2 降级步骤；保留期可调；磁盘使用率告警 |
| 原始数据含聊天与昵称 | 隐私与泄露风险 | 不公开下载、令牌导出、到期删除、日志不含负载 |
| 香港网络高抖动 | 位置更新不稳定 | 全队各运营商 RTT / P95 实测；客户端重连 / 插值；Agent 本地缓冲补传 |
| 多人同时修改 | 标记覆盖 / 丢失 | operationId 去重、定序与版本号、后写覆盖、撤销；冲突提示留待下一轮 |
| 战局识别错误 | 标绘或地图串局 | BATTLE-01：状态转换 + dHash + 范围匹配；核弹切换识别；人工确认 |
| 本人状态判定不准 | 友军图标消失或误显示 | 显式状态上报；Player.type 与 army 交叉验证；DATA-04 校准 |
| Sim 模式 map_obj 不含队友飞机 | 匿名友军缺失 | 依赖各 Agent 本人上报；聊天网格解析列为后续选项 |
| 未签名 Agent 被拦截 | 队员无法启动 | AGENT-02 图文说明；评估代码签名 |
| **合规（自动敌情显示）** | 账号处罚风险 | 事实：Gaijin 论坛 2024-05-13 Stona_WT 回复称，一般而言用 localhost 数据显示覆盖层不属于封禁行为；但在无标记模式中以罗盘式覆盖层显示敌方标记会带来优势，“可能被视为 ESP 覆盖层”，官方不认可这种用法（https://forum.warthunder.com/t/tools-using-data-provided-on-port-8111/106664）。应对：全量上传保持不变，显示由 `enemyPolicy` 控制，默认 off，无标记模式 modeCap=off，策略变更留痕 |
| 域名 / DNS 到期或解析错误（次要） | 网页与 WSS 无法访问 | 自动续费；监控域名与证书到期 |

</div>

## 11. 工程仓库结构

```diagram
{{tree}}
```

**从 Neo 迁移的文件对应关系：**`src-tauri/src/server.rs` → `apps/agent/src/proxy.rs`；`cloud-tactical-server/src/*` → `apps/server/src/*`（rooms / relay / viewer / config 保留结构，fusion 移入 `crates/fusion` 重写）；`cloud-tactical-server/protos/*` → `crates/protocol/protos/`（扩展）；`src/lib/wt8111.ts` → `apps/web/src/lib/wt8111.ts`；`WT_8111_API_REFERENCE*.md` → 并入 `docs/8111_API_SPEC.md`。WTM 夹具与 WTP 地图哈希表分别放入 `fixtures/8111-air/`、`fixtures/map-hashes/`。

**最小可上线构成**

```code
1 个 Windows Agent 可执行文件（Neo 代理改造）
1 份 Agent 图文安装说明（SmartScreen / 杀软拦截处理）
1 个香港 VPS 上的服务程序（绑定域名，Caddy 自动 HTTPS）
1 套静态响应式网页
1 个 SQLite 数据库 + 1 个 Raw Store 目录
2 张已校验的当前战局地图图片
```

不为 8 人系统引入微服务、消息队列、跨机数据库和过早的性能优化。

## 12. 基准资产与参考资料

**设计基准：** WT_Tactical_Command_UI_V10_Map_Fill.html（离线模拟 UI）；附录 A《8111 接口规范》（独立文件 `8111_API_SPEC.md`）。

<div class="keep" markdown="1">
**开源项目（代码与文档直接复用）：**

1. WT-8111-Neo — 架构基线：Rust 8111 代理、TS 类型、cloud-tactical-server、Protobuf 协议、API 参考。https://github.com/CreeperUX/WT-8111-Neo （PR #1 / #2 / #3，分支 `codex/cloud-tactical-server`、`codex/cloud-gui-adaptation`）
2. wt-modern-8111 — 最详细的 8111 实测文档、抓包规程与夹具。https://github.com/NolanMullins/wt-modern-8111
3. WarThunder-localhost-documentation — 各端点 OpenAPI。https://github.com/lucasvmx/WarThunder-localhost-documentation
4. wp8f — datalink：UDP StatePacket（116 字节）、session 分池、4Hz 上报、无状态请求 / 应答。https://github.com/matrixsukhoi/wp8f
5. Link18 — 局域网 UDP 广播同步、规划模式航点、APP-6 风格符号、颜色判定。https://github.com/LucasZ3214/link18
6. WarThunder-Tactical-Map — 实测颜色调色板、航迹匹配。https://github.com/unfairbullet/WarThunder-Tactical-Map
7. WarThunder-Plotter — 地图 dHash 识别与哈希表、陆战中空战视图变换、对局宽限期。https://github.com/ElBartt/WarThunder-Plotter
8. WTDashboard — 远程访问设置、丢失目标“幽灵”保留。https://github.com/HYLiangSheng/WTDashboard
</div>

<div class="keep" markdown="1">
**交互与技术参考：**

9. TacOps（Arma Reforger 战术规划）— 标绘交互与军标模型。https://tacops.gg/ 、https://app.tacops.gg
10. milsymbol — APP-6 / MIL-STD-2525 军标渲染库。https://github.com/spatialillusions/milsymbol
11. War Thunder 官方论坛 • Tools using data provided on port 8111。https://forum.warthunder.com/t/tools-using-data-provided-on-port-8111/106664
12. Axum — https://github.com/tokio-rs/axum ；prost — https://github.com/tokio-rs/prost ；protobuf-es — https://github.com/bufbuild/protobuf-es
13. Caddy — https://caddyserver.com/
14. Yjs（下一轮协同评估）— https://github.com/yjs/yjs
</div>

## 13. 最终执行建议

**先 fork Neo 完成 3–4 天的第一周竖切与 P0 实测，再以 V10 UI 和 TacOps 式标绘为固定设计基准进入联机开发。**全量上传让服务器拥有回放、复算和自动陆空校准的能力，但显示什么必须由服务器策略统一决定。第一阶段真正必须做对的是：

1. 两个不同范围的地图（含陆战中空战视图）能够精确叠加同一份世界坐标。
2. 所有参战队员的位置与朝向带身份、不重复、及时地出现在网页。
3. 单图画的每个军标、箭头、阶段线和区域能够立即同步到其他人的单双地图。
4. 地图始终正方形、全屏无滚动，手机上也能完成完整编辑流程。
5. 原始数据完整落盘，敌情策略开关在服务器端可靠生效。

这五件事完成并通过一场 8 人自定义对局验证，就得到真正可用的首个连队战术地图版本。其余需求放入下一轮独立评审。

<div class="appendix" markdown="1">

## 附录 A：8111 接口规范

以下内容与独立文件 `8111_API_SPEC.md` 同源生成。

{{SPEC}}

</div>
