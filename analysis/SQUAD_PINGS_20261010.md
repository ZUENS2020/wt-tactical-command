# 小队成员与地图标点（标点）实测报告 — 2026-10-10

数据：HK 接收端全部会话（8 个，2026-10-10 17:17–20:52 UTC+8 的本地副本，另加重新部署后 21:07 的实时 markers.jsonl）。
实战对局在 `193009-1709`（7 局陆战 RB）和 `203023-df9b`（进行中，已有多局）；其余是试车、试飞和标签测试。
脚本：`squadping/enumerate.py`（组合枚举 → `squadping/combos.json`），`analyze_allies.py`（小队轨迹和标点融合 → `allies_results.json`），代码在 `allies/`。

## 1. 结论速览

| 项目 | 状态 | 依据 |
|---|---|---|
| 小队成员 = **绿色**单位：陆地 `#67D756`（103,215,86），空中 `#39D921`（57,217,33），blink 0 | **已实测确认** | 1709/df9b 共 20 条小队轨迹（每帧最多 1 个），图标覆盖 Light/Medium/HeavyTank、TankDestroyer、SPAA、Fighter；时间和 HUD “团队协作”奖励吻合；小队成员 HuWangTK 发过 2 条聊天 |
| 普通队友 = `#174DFF`（23,77,255） | 已确认 | 地面、空中单位以及友方机场、重生点、占领点 |
| 刚出生的友军短时为暗蓝：陆地 `#1839A7`，空中 `#134AFF` | 已确认（含义是推断） | 每局开局和重生时出现，之后转为 #174DFF 或绿色 |
| 本人 = `Player` + `#faC81E`；**阵亡后 `Player` 变为 `#174DFF`，跟随被观战的队友** | 已确认 | 1709/df9b 中 758 帧 `Player #174DFF`。旧逻辑会把观战对象当成本人 |
| **发信号 = 单位变为 `#043FFF` + blink 2**（本人、队友、小队成员都一样，会盖住原色） | 已确认 | 与聊天一一对应：标点约 4.9 s（3.2–8.2），位置呼叫约 5.9 s（连发时 10.9），“谢谢”约 1.9 s |
| **地图标点不是 map_obj 对象**：全部数据中 `point_of_interest` 为 0，也没有新的 type 或 icon | 已确认（负结论） | 892 种 (type,icon,color,blink) 组合，其中 838 种是 capture_zone 占领渐变色；unknown_icons.json 中 unknown_icons 和 unknown_types 都为空 |
| 队友标点的**目标位置只在 gamechat 文本**里，格式为 `注意战术地图！ [c4]` / `注意该坐标位置！ [b6]` / `注意当前标识区域！ [f8]` | 已确认 | 18 条队友标点，网格都≠发送者所在格（0/18）。标点指向别处，发送者同时闪烁 |
| 位置呼叫（请求掩护/支援，空袭目标请求带高度）`[e6]` = **发送者所在格** | 已确认 10/10 | 网格换算公式见 §3 |
| **本人标点不带网格**（发送者看不到坐标后缀），只有本人 Player 闪烁 | 已确认 | 1709 20:27:51、实时 21:07:56 `注意战术地图！`：precision=none |
| 标点在地图上的显示时长 | **API 不提供**。用 30 s 作为网页显示 TTL（推断值）；信号闪烁时长是实测值 | — |
| 小队成员自己发的标点 | **未观察到**（只有小队成员发的“空袭警报！”）；推断机制相同（绿色单位闪烁 + 聊天） | 需要实测 |
| 非小队队友的名字 ↔ 单位 | 只能在闪烁时与聊天对上（一对一），不发言的队友没有名字 | — |

## 2. 颜色组合（全部会话，主要条目）

| type | icon | color | blink | 帧计数 | 每帧最多 | 会话 |
|---|---|---|---|---|---|---|
| ground_model | Player | #faC81E | 0 | 14524 | 1 | b6a7,1709,df9b |
| aircraft | Player | #faC81E | 0 | 6277 | 1 | b6a7,1709,df9b |
| ground_model | Player | #174DFF | 0 | 758 | 1 | 1709,df9b（观战） |
| aircraft/ground_model | Player | #043FFF | 2 | 140/70 | 1 | 本人信号 |
| ground_model | Heavy/TD/Light/Medium/SPAA | #67D756 | 0 | 6173/2981/2033/1748/1724 | 1 | 1709,df9b（小队） |
| aircraft | Fighter | #39D921 | 0 | 1170 | 1 | 1709（小队飞机） |
| ground_model | Medium/Heavy/TD/Light/SPAA | #174DFF | 0 | 73984…5018 | ≤10 | 队友 |
| ground_model | Medium/Heavy/TD/Light/SPAA | #1839A7 | 0 | 4772…677 | ≤7 | 刚出生的队友 |
| aircraft | Fighter/Assault/Bomber | #134AFF | 0 | 584/223/60 | ≤2 | 刚出生的友机 |
| ground_model/aircraft | Heavy/Medium/TD/Light/Fighter/Assault | #043FFF | 2 | 622/413/287/50/271/111 | 1 | 友军信号 |
| ground_model | 各类 | #fa0C00 / #f00C00 | 0 / 2 | — | — | 敌（不变） |
| capture_zone | capture_zone | #174DFF/#fa0C00/#ffFFFA + 835 种渐变 | 0/1 | 63654 | 3 | 占领进度渐变，不是单位 |
| respawn_base_tank/fighter, airfield, bombing_point | — | 蓝/红 | 0 | — | — | 不是单位 |

map_info：valid=false 7438；陆战 hud_type 1，grid_steps 140/150/170/200/225/250/550；空战 hud_type 0，grid_steps 3200/5500/6500。

## 3. 网格换算（已用 10/10 位置呼叫验证）

列号 n（1 起）、行字母 L（A=1，可以是两个字母）：
`x0 = grid_zero.x + (n-1)·step`，`y0 = grid_zero.y + (L-1)·step`（世界坐标，step = grid_steps），再用 map_min/map_max 归一化。
实现：`allies/grid.py`、`common/src/radio.rs::GridInfo`。

**局限**：陆战中切到飞机视图或在大地图边缘时，算出的格子可能越界（见过 `Z1`、`AE4`、`C32`）。标点位置一律以聊天中的网格为准。

## 4. 数据模型（receiver `markers.jsonl` 和 `/api/markers`，`/api/status` 的 `allies`）

- `ev:"squad"`：小队轨迹，含 track、type、icon、x、y、t0、t1，以及 name（如果聊天中出现）。
- `ev:"signal_start"/"signal_end"`：track、role（self/squad/ally）、icon、xy、cell、dur_ms、msg_ids。
- `ev:"ping"|"location"|"radio"|"signal_only"`：kind（ping_map/ping_grid/ping_marked/need_cover/need_support/air_target_request/thanks/…），sender、own、sender_role，sender_xy 和 sender_cell，grid、area（归一化后的格子边界 [x0,y0,x1,y1]），x、y（格子中心或发送者位置），precision（grid/sender_pos/none），signal_ms，ttl_ms（标点 30 s，推断值），alt_m，text，msg_id，source（chat/map/chat+map）。
- `ev:"map_marker"`：如果将来出现 point_of_interest，原样记录，绝不当作单位。
- 首次 `lastId=0` 拿到的历史聊天（df9b 有 13 条）会被标为 backlog 并忽略。
- 敌我判定不变；naval 过滤器显式跳过 point_of_interest；`Player #174DFF`（观战）不再更新本人位置。

## 5. 需要用户做的游戏内验证

1. 和好友**组队**（不是自动组队也行），让小队成员在地图上打“注意战术地图”标点，同时按 **「小队队友标点」**。用来确认小队成员闪烁时的颜色，以及聊天格式是否带网格。
2. 自己打标点（地图上点选位置）时按 **「我打了标点」**。已确认 API 不给目标位置，按钮用来量化这种情况有多常见。
3. 看到队友标点时按 **「队友标点」**；小队成员在身边时按 **「小队成员在附近」**。
4. 可选：空战 / 陆战中切换飞机时打标点，用来验证网格换算的越界问题。
