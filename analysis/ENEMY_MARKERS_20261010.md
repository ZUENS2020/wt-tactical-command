# 敌方地图标记（map_obj 红色对象）分析与解析模块（2026-10-10）

> 时间均为北京时间（UTC+8）。数据：`/workspace/wt-collect/data/2026-10-10/` 全部 4 个会话（截至 18:56）。
> 脚本：`analyze_enemy.py`（结果 `enemy_results.json`）、`plot_enemy.py`（PNG）、模块 `enemy/`（23 个单元测试全部通过）。

## 0. 结论速览

| # | 结论 | 级别 |
|---|---|---|
| 1 | **目前没有任何一段实战（RB）数据。**有对象的只有 2 段，都在会话 `b6a7`：陆战**试车** 18:31:37–18:31:59（Tiger H1，靶场）和空战**试飞** 18:32:09–18:32:30（Pucará，丹麦图）。`9bcb`、`5a10` 两个会话全部是 timeout（当时还在连 8111），`962f` 全程在机库。 | 已证实 |
| 2 | 所以 **(A) 队友侦察** 和 **(B) 被命中短暂暴露** 的字段差异，**现有数据无法给出任何结论**。下文关于 A/B 的内容都是假设，要靠 §6 的实测来验证。 | 已证实（没有数据） |
| 3 | 红色共 2 种色调，**色调和 blink 一一绑定**：`#fa0C00` (250,12,0) 一定是 blink 0，`#f00C00` (240,12,0) 一定是 blink 2。全部 7,531 次敌方红色观测中没有 `fa/2` 或 `f0/0`。blink 1 从未出现。 | 已证实（仅限试车/试飞） |
| 4 | 地面单位的 `#f00C00/2` 是**带固定计时器的临时高亮状态**：<br>- 陆战试车 6 次完整高亮时长 11.13–11.23 s，另 1 次 6.70 s<br>- 空战试飞 2 次时长 5.01 s / 4.99 s<br>- 结束后**一律回到 `#fa0C00/0`**（2→0），没有一次是 2→消失<br>- 高亮期间位置不会冻结：移动靶 #7 在高亮期间 76% 的帧位置有更新 | 已证实（试车）；触发原因未知 |
| 5 | 敌机（aircraft）始终是 `#f00C00/2`，带 dx/dy，17–18 s 内从不切换，92% 的帧位置有更新。 | 已证实（试飞） |
| 6 | 被击毁的目标**下一帧就从 map_obj 消失**：T-26 在 18:31:55.579 最后一次出现，hudmsg 击毁消息在 55.657 到达，`/indicators.speed=-1` 的后坐力跳变在 55.486。消失前它是 `#fa0C00/0`，**命中/击毁本身不会触发 blink 2**。没有“残影/最后位置”对象，last-known 要靠客户端自己保留。 | 已证实（试车，n=1） |
| 7 | hudmsg 历史（以前的实战）里有一组**侦察相关的奖励**：<br>- 给侦察车的“战场引导×N”<br>- 给击杀者的“密接支援×N”<br>3/3 次都在队友击杀后 ≤1 s 内成对出现，获得“战场引导”的都是有侦察能力的车（M41A1、M18、鼬鼠 1A4）。<br>对应 Wiki 的 Intelligence / According to Intelligence 奖励。可以用它来锚定“被侦察目标被击毁”的时刻。 | 强推断（3/3） |
| 8 | 文档里**都没有** `#f00C00` 和 `#fa0C00` 的语义说明。blink 的说明只有“0 不闪、1 闪、2 强闪”这种渲染层描述，没有一份资料把它和侦察/命中联系起来。 | 已查证（§5） |

---

## 1. 片段判定（实战还是试车）

| 片段 | map_info | hudmsg（实时，不含 lastDmg=0 回放的历史） | 友军单位/帧 | 占领点/重生点 | mission.json | 判定 |
|---|---|---|---|---|---|---|
| b6a7 18:31:37.1–59.5 | gen 26，0..4096，hud_type 1，地图 dHash = `firing_range_tankmap` | 只有 1 条：“…（东线虎）击毁了 T-26”（受害者没有玩家名 → AI 靶车） | 0 | 无 | objectives null | **陆战试车** |
| b6a7 18:32:08.9–29.5 | gen 27，±65536，hud_type 0，`air_denmark_map` | 无 | 0 | 无（只有轰炸点、本方机场） | objectives null | **空战试飞** |
| 9bcb / 5a10 | 全部 timeout（端口 8111） | — | — | — | — | 没有数据 |
| 962f | `valid:false` | — | — | — | — | 机库 |

判定规则在 `enemy/stream.py: detect_session_kind`：

- 判为**实战**，满足任一条即可：
  - 有友军单位（蓝色或绿色、非 Player）
  - 有 capture_zone / respawn_base_*
  - 实时 hudmsg 里有 “X (载具) 击毁了 Y (载具)” 这种 PvP 句式
- 判为**试车**：以上都没有。

## 2. 变体字段对照（全部真实数据）

| 变体 | type | icon | color / color[] | blink | dx/dy | icon_bg | 观测次数（帧数）/ 轨迹数 | 持续时间 | 可见期间位置 | blink 转换 | 当前解释 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| V1 敌地面·常态 | ground_model | LightTank / MediumTank / HeavyTank / TankDestroyer / Wheeled / SPAA | `#fa0C00` [250,12,0] | 0 | 无 | none | 陆战试车 928 / 8 轨迹；空战试飞 3,524 / 23 轨迹 | 从出现一直到片段结束（16–21 s），或到被击毁 | 实时更新：移动靶 75–80% 的帧有变化（#7 含停车 25%）；静止靶完全不变 | 初始状态；2→0 后回到这里；被击毁直接消失 | 试车：AI 靶车始终显示。RB：可能是“已知、未高亮”的显示状态（**待测**） |
| V2 敌地面·高亮 | ground_model | 同上（实际见到 Light/Medium/Heavy/TD） | `#f00C00` [240,12,0] | 2 | 无 | none | 陆战试车 729 / 7 轨迹；空战试飞 100 / 2 轨迹 | **11.13 / 11.13 / 11.13 / 11.13 / 11.22 / 11.23 s**（陆战）；6.70 s（陆战 #7）；**5.01 / 4.99 s**（空战） | #7 高亮期间 76% 的帧位置有更新（**不冻结**）；空战 #32/#33 高亮 5 s 内位置完全不变，结束后立刻开始移动（单位刚生成在原地停留，还是标记冻结，无法区分） | 0→2：出现后 0.1–1.3 s（#7 是 13.4 s 后）；2→0：计时到期。**没有 2→消失** | 临时“新发现/高亮”状态，有固定计时器。RB 中“被侦察/被命中短暂显示”很可能用这个状态（**假设**） |
| V3 敌机 | aircraft | Bomber / Fighter | `#f00C00` [240,12,0] | 2 | **有**（100%） | none | 1,782 / 10 轨迹 | 整段（17.4–18.1 s） | 92% 的帧有变化 | 从不切换 | 敌机一律用高亮样式（与 WTM 的 JH-7 夹具一致） |
| V4 轰炸点 | bombing_point | bombing_point | `#fa0C00` | 0 | 无 | none | 468（147 帧） | 18:32:14.5 起每 2 s 增加 1 个，共 4 个 | 静止 | — | 任务目标，不是单位（不进入跟踪） |
| 本人 | ground_model / aircraft | Player | `#faC81E` [250,200,30] | 0 | 有 | none | 379 | — | — | 空战出生时 `#043FFF` [4,63,255] **blink 2**，持续 2.0 s（11.84→13.83） | 说明 blink 2 是通用的“提示/闪烁”渲染状态（WTM：发出“注意地图”后，本人也会蓝闪约 5 s） |
| 友方机场 | airfield | none | `#174DFF` | 0 | sx/sy/ex/ey | none | 186 | — | — | — | — |

### 2.1 陆战试车逐目标时间线

“相对车体方位”= 目标方位 − 本人车体朝向（约 70°）。

| # | icon | 距离 | 相对车体方位 | blink 时间线 | 备注 |
|---|---|---|---|---|---|
| 8 | TankDestroyer | 99 m | +9° | 出现即 2（37.763）→ 48.984 变 0（**11.22 s**） | |
| 1 | LightTank（T-26） | 225 m | +21° | 0 → 37.763 变 2 → 48.892 变 0（11.13 s） | 55.579 被击毁后消失 |
| 2 | MediumTank | 398 m | +18° | 0 → 2（37.763）→ 0（48.892，11.13 s） | |
| 3 | MediumTank | 596 m | +16° | 同上（11.13 s） | |
| 4 | HeavyTank | 793 m | +14° | 出现即 2（37.655）→ 0（48.783，11.13 s） | |
| 6 | HeavyTank | 1,190 m | +13° | 0 → 39.051 变 2（**晚 1.3 s**）→ 0（50.284，11.23 s） | |
| 7 | MediumTank（移动靶） | 546 m | +75° | 0（13.4 s）→ 51.182 变 2 → 57.881 变 0（**6.70 s**） | 51.182 正好是本人开始起步（speed 0→1）的时刻 |
| 5 | HeavyTank | 793 m | **+88°** | **始终是 0** | |

规律：

- 车体正前方 ±21° 内的 6 个静止靶，在出生后 0.1–1.4 s 内进入高亮，并在约 11.1–11.2 s 后同时恢复。最远的 #6 晚了 1.3 s。
- 侧方 88° 的 #5 从未高亮。
- 侧方 75° 的移动靶 #7 在本人起步时才高亮，而且只持续了 6.7 s。

这符合“本车乘员视野内发现 → 闪烁若干秒”的假设，但 `/indicators` 没有炮塔/相机朝向，**无法证实**。另外空战试飞里，2 辆新生成的轻坦（#32 在 3.1 km、#33 在 3.0 km，比其他地面单位都近得多）出现 0.1 s 后高亮，正好 5.0 s；其余 7–28 km 外的单位从未高亮。说明“新生成/新发现 → 高亮 N 秒”这一规律在空战中也成立，N 依模式不同（陆战约 11 s，空战 5 s）。

图：`enemy_tracks_ground_testdrive.png`、`enemy_tracks_air_testflight.png`。左图为地图上的轨迹，橙色 = fa0C00/0，品红 = f00C00/2；右图为逐轨迹 blink 时间线，标出了位置变化帧、hudmsg 击毁和 speed=-1 后坐力。

![陆战](enemy_tracks_ground_testdrive.png)
![空战](enemy_tracks_air_testflight.png)

### 2.2 与 hudmsg / indicators 的时间相关

| 事件 | 时刻 | 与地图对象的关系 |
|---|---|---|
| `/indicators.speed` 0→−1→0（静止开炮的后坐力） | 18:31:55.486 | 下一帧（55.579）是 T-26 的最后一次出现；之后 T-26 消失 |
| hudmsg id 1375 “…（东线虎）击毁了 T-26” | 18:31:55.657（hudmsg 2 Hz 轮询） | 在消失后 78 ms 到达 |
| `speed=-1` | 18:31:41.565 | 刹车回溜或开炮（无法区分）；地图对象没有任何变化 |
| 0→2 切换 | 37.66 / 37.76 / 39.05 / 51.18 | 附近没有 hudmsg，也没有开炮迹象 → **高亮不是命中触发的** |

陆战 `/indicators` 没有弹药计数（first_stage_ammo = −1），也没有开火标志。“玩家开火”只能用 speed 的 −1 跳变弱推断。**hudmsg 只报告击毁、起火、重创和致命攻击，不报告普通命中**，所以 B 类（命中暴露）不能完全靠 hudmsg 锚定。

## 3. 关于 A（侦察）和 B（命中暴露），现在能说什么

**能确定的（试车数据）：**

1. 红色地面单位只有 V1（fa0C00/0）和 V2（f00C00/2）两种表现；V2 有固定计时器，结束后回到 V1。
2. map_obj 中没有任何字段直接标注“被侦察”或“被命中”：`icon_bg` 恒为 none，没有额外键，地面单位没有 dx/dy。如果游戏在 RB 中区分 A/B，**只可能体现在**：
   - 颜色 / blink
   - 出现时长（A：Wiki 写明 30 s；B：“短暂”）
   - 位置是否更新（A：Wiki 写“周期性更新”；B：大概率冻结在命中时的位置）
   - 消失方式
3. 击毁会让对象立即消失，不会留下最后位置。

**假设（需要 §6 实测）：**

| 假设 | 判别特征 | 依据 |
|---|---|---|
| H-A1：被侦察的敌人以 V2（f00C00/2）出现，约 11 s 后转为 V1，总共可见约 30 s 后消失 | 可见时长 25–35 s；位置阶梯式更新（每隔 N 秒跳一次）或连续更新 | Wiki：RB 侦察 30 s、位置周期性更新；试车中 V2 计时约 11 s |
| H-B1：被命中暴露的敌人以 V2 出现，几秒后**直接消失**，位置冻结 | 可见时长 ≤ 15 s；位置变化 0 次；出现时刻在本人或队友开炮后 ≤ 1 s | Wiki RB：“nearby allies scout or hit → briefly displayed red icon” |
| H-B2：命中暴露只在 V1 出现（不闪） | 同上，但 blink 一直为 0 | 试车中击毁 T-26 时没有任何闪烁 |
| H-C：RB 中敌机也只在被发现后出现 | 敌机可见区间 | 未知 |

**无法回答：**

- 两种红色是否分别对应“侦察”和“命中”
- 侦察标记是否在 30 s 内跟随目标移动
- 命中暴露的时长
- 同一目标先被命中、再被侦察时如何表现

## 4. 解析模块 `enemy/`

- **输入**：map_obj 帧流 + map_info + 本人对象；可选 hudmsg 事件。
- **输出**：`Contact` 列表，以及 `ThreatSummary`。

`Contact` 字段：

- 身份：`contact_id`
- 时间：`first_seen_ms` / `last_seen_ms`
- 揭示类型：`reveal_kind`（scouted | hit_revealed | visible_static | unknown）+ `reveal_rule` + `reveal_confidence`
- 类别：`cls`（由 icon 得出）
- 原始状态：`color` / `rgb` / `blink`，以及 `blink_seq`（转换序列）
- 位置：归一化 `x,y`；世界坐标 `wx,wy`（world = map_min + norm × size，y 向下）
- 运动：`heading_deg`（仅 dx/dy 存在时）；`vel_mps` / `speed_mps` / `course_deg`（3 s 窗口位移）
- 状态：`visible` / `stale` / `age_ms` / `visible_spans`
- 相对本人：`distance_m`、`bearing_deg`（compass = atan2(east, −south)，与 heading = atan2(dx,−dy) 一致）

`ThreatSummary` 字段：8 扇区计数（全部 / 仅可见）、最近威胁（id、距离、方位、扇区）、主方向扇区。

跟踪器要点：

- 关联键：(type, icon)，不含颜色（同一目标会换色）
- 门限：25 m + vmax × 距“位置最后一次变化”的时间（地面 35 m/s，空中 450 m/s）
- 分配：全局贪心
- 生命周期：丢失 400 ms 后转为 stale（保留最后位置）；5 s 内可以重新捕获；60 s 后清除
- 排除：海军 icon、Player、轰炸点、机场

`reveal_kind` 规则（`reveal.py`，按顺序匹配）：

| 规则 | 条件 | 结果 | 置信度 |
|---|---|---|---|
| R0 | 飞机 | unknown | 0.3 |
| R1 | 试车，地面目标 | visible_static | 0.9；出现过 blink 2 的为 0.7 |
| R1s | 消失前后 ±1.5 s 内出现 hudmsg“战场引导/密接支援” | scouted | 0.75 |
| R2 | 首次出现前 1.5 s 到后 2.5 s 内有 hudmsg 击毁/起火/重创/致命，且可见时长 ≤ 15 s | hit_revealed | 0.6（涉及本人）/ 0.5 |
| R3 | 首段可见时长 20–40 s | scouted | 0.55；有阶梯式更新再 +0.15 |
| R4 | 首段可见时长 ≤ 15 s | hit_revealed | 0.5（位置不变）/ 0.35 |
| R5 | 可见 > 45 s | visible_static | 0.5 |
| R9 | 以上都不满足 | unknown | 0 |

这些阈值等 §6 的实测数据到手后再标定。

在本次数据上的结果：陆战 8 个目标全部为 visible_static（R1/R1b）；空战 23 个地面目标为 visible_static，10 架敌机为 unknown（R0）。

单元测试：23 个，全部通过。覆盖范围：

- 录制夹具：8 个目标没有重复轨迹；换色后身份不变；色调与 blink 绑定；T-26 消失时刻与击毁消息相关；移动靶测速；TD 距离 94 m、方位 79.6°；E 扇区为主方向；本人朝向约 70°；空战敌机恒为 blink 2；空战两辆轻坦高亮 5 s
- 合成用例：间断后重新捕获；stale 标记；3 s 冻结后跳 40 m 仍为同一轨迹；同类目标不串轨；R2/R3/R1s 规则

## 5. 资料核查（blink、两种红色、侦察标记）

| 来源 | 内容 | 对本问题的意义 |
|---|---|---|
| War Thunder Wiki《Scouting》 https://wiki.warthunder.com/mechanics/238-scouting | RB：在双筒望远镜/瞄准镜下把准星放在目标上按侦察键 → 目标上方出现红色小箭头，“updates periodically”，小地图同步更新，持续 **30 s**；30 s 内其他人不能重复侦察；被侦察目标被友军击杀时，侦察者获得 Intelligence 奖励，击杀者获得 According to Intelligence 奖励 | A 类的时长和更新方式的先验；“战场引导/密接支援”很可能就是这两个奖励 |
| War Thunder Wiki《Realistic Battles》 https://wiki.warthunder.com/gamemode/realistic_battles | 地面 RB 中敌人不显示在小地图上，“only a red coloured icon is briefly displayed on the minimap when nearby allies scout or hit a vehicle” | 证实存在 A/B 两种机制；B 类为“短暂显示”；没有提到颜色区分 |
| 官方开发博客 “New opportunities for light vehicles” https://warthunder.com/en/news/5193-development-new-opportunities-for-light-vehicles-en | RB 侦察标记“will be updated for 30 seconds”；RB 侦察失败会进入冷却 | 同上 |
| 官方论坛 “Scouting Through Scout UAVs” https://forum.warthunder.com/t/scouting-through-scout-uavs/260991 | 成功侦察后的冷却：无无人机时 6 s；有无人机在空时 RB 18 s | 测试时要留出冷却时间 |
| 官方论坛 “Blinking Icons on Sim EC Map” https://forum.warthunder.com/t/question-blinking-icons-on-sim-ec-map/224750 | 玩家推测：目标闪烁表示正在遭受攻击（机场被打时会闪） | 闪烁 ≈ 提示/受攻击，属于渲染层含义；不涉及 8111 字段 |
| Steam 讨论 https://steamcommunity.com/app/236390/discussions/0/1836811737987257639/ | RB 中屏幕边缘的 chevron 出现在队友命中敌人的位置附近，是近似位置 | B 类是近似位置，暗示冻结 |
| lucasvmx/WarThunder-localhost-documentation `openapi.yaml`、`MapObjects.md` | 只写了 “blink: Object blinking indicator”，示例全为 0；颜色是旧调色板（敌 `#fa3200`） | 没有语义说明 |
| WT-8111-Neo `WT_8111_API_REFERENCE.md` §Common Fields | “0 no blink, 1 normal blink, 2 heavy blink” | 只是渲染描述，没有证据来源 |
| wt-modern-8111 `docs/localhost-8111-api.md` | 观察到 blink 0/1/2；“注意地图”后本人蓝闪约 5 s；建议按“出现前 / 出现后 / 丢失后”采集对比（Contact-visibility verification）；隐藏对象被省略，不推断不可见目标；`objects.ts` 对 blink≠0 的敌方地面单位画虚线环 | 与我们“blink 2 = 提示态”的观察一致；没有侦察/命中的语义 |
| WTDashboard `wtdb/unit_tracker.py` | 按颜色、类型和归一化距离 0.08 关联；连续 3 帧未见才判为消失；保留 30 s 的“lost”残影并按惯性外推；**完全不使用 blink** | last-known 是客户端自己实现的，游戏不提供 |
| link18 `web/dashboard.js`、`rendering.py` | blink 只用于渲染闪烁；`isFriendlyColor` 只把 fa0c00/ff7e7e/ff0000 判为敌 → **会把 `#f00C00` 误判为友军** | 移植时不要沿用 |
| War-Thunder-Tactical-Map `MapCanvas.tsx` | blink 作为布尔值做透明度动画 | 没有语义说明 |
| 0corexpsx/War-Thunder-Intelligent-UI（GitHub README） | “Ghost markers”：被发现的敌人消失后，在最后位置保留 45 s 渐隐标记 | 同样由客户端实现 |

结论：**没有任何公开资料说明 `#f00C00` 与 `#fa0C00` 的区别，也没有资料说明 blink 2 与侦察/命中的关系**。官方只给出了游戏机制层面的描述（侦察 30 s 周期更新、命中短暂显示）。

## 6. 实测方案（RB 陆战，拿到决定性样本）

**准备：**

1. 采集器升到最新版，运行 `/rate 20`，把 map_obj、indicators、state 提到 20 Hz（加载时 3 s 的卡顿可以接受）。hudmsg 保持 2 Hz 即可。
2. 开 OBS/ShadowPlay 录屏，画面要包含小地图，并叠加 Windows 系统时钟（OBS“文本(GDI+)”读取本地时间，或在屏幕角落显示秒级时钟）。**录像是唯一的地面真值**：哪个图标何时出现、是否闪、是否跟着动。
3. 时间标记有两种方式，选一种：
   - 在采集器窗口输入备注后回车（会写入 `#note`）；
   - 在**小队频道**发短消息（例如 `m1`），会出现在 `/gamechat` 里，有时间戳，并且不用切出游戏。

**测试 A：侦察**（与一名会开侦察车的小队队友配合，正常对局中进行）

1. 自己开普通车，与队友约好：他侦察时在小队频道发 `S`，或者自己看到侦察提示时发 `A1`。
2. 每次侦察后，**不要攻击该目标**，盯住小地图 35 s，记录：
   - 图标第一次出现的时刻
   - 是否闪烁
   - 是否随目标移动（跳动还是连续）
   - 何时消失
3. 至少做 3 次。其中 1 次尽量选**移动中的目标**（区分“周期性更新”和“冻结”）。如果能做到，再选 1 次在 30 s 内被队友击毁的目标，验证 hudmsg 里“战场引导/密接支援”的时刻和图标消失是否一致。
4. 冷却时间：无无人机时 6 s，有无人机在空时 18 s。同一目标 30 s 内不能重复侦察。

**测试 B：命中暴露**

1. 自己命中一个**未被侦察、此前不在小地图上的**敌人，最好是非致命命中（打履带或侧裙）。开炮后立刻在小队频道发 `H1`（或输入备注），然后看小地图 15 s。
2. 再做一次“队友命中、自己没有直视目标”的情况：看到附近队友交火时发 `T1`。
3. 记录图标出现的时刻、时长、是否闪、是否移动、消失方式。至少 3 次。

**测试 C：对照**

1. 静止、没有任何交火 60 s，确认 map_obj 中没有红色地面单位（RB 基线）。
2. 敌机：有敌机出现在小地图上时发 `P1`。

**事后分析**（我来做）：`analyze_enemy.py` 已经会输出每条轨迹的以下信息，与录像和标记对齐后，即可确定 A/B 的字段差异并标定 `reveal.py` 的阈值：

- 出现和消失时刻
- 可见区间
- blink 时间线
- 位置变化率与最长静止时间
- 距离和方位

**判读要点：**

| 看什么 | A（侦察）预期 | B（命中）预期 |
|---|---|---|
| 可见时长 | 约 30 s | 几秒 |
| 位置 | 阶梯式更新（`max_static_s` 为数秒，变化次数 ≥ 2） | 冻结（变化次数 0） |
| blink / 色调 | 起始是否为 f00C00/2、约 11 s 后是否转为 fa0C00/0 | 是否始终 2 或始终 0 |
| 消失方式 | 时间到后消失 | 时间到后消失 |
| hudmsg 锚点 | 被击杀时出现“战场引导”+“密接支援” | 只有在致命/起火/重创时才有消息 |

**请同时记下：**

- 小地图上能看到的友军数量（实战中友军应为蓝色/绿色，在 map_obj 中可直接验证）
- 是否开启色盲模式
- 使用的车辆

## 7. 未决问题

1. RB 中 A/B 在字段上是否可以区分（颜色、blink、时长、位置更新）。
2. 陆战约 11 s、空战 5 s 的高亮计时器是什么含义：“被本车乘员发现”，还是“新生成”？#5（侧面 88°）从未高亮，#7 在本人起步时才高亮，倾向前者。
3. blink 1 何时出现（至今 0 次）。
4. 侦察标记在 30 s 内是否跟随目标；侦察和命中叠加时如何表现。
5. 色盲模式和小队绿色下的颜色。
6. “战场引导/密接支援”的英文对应关系需要用英文客户端核对（假设对应 Intelligence / According to Intelligence）。

## 8. 文件

- `ENEMY_MARKERS_20261010.md`（本报告）
- `enemy_results.json`（逐片段、逐轨迹的结果）
- `enemy_tracks_ground_testdrive.png`、`enemy_tracks_air_testflight.png`
- `analyze_enemy.py`、`plot_enemy.py`
- `enemy/`（模块、README、`tests/test_enemy.py`、`tests/fixtures/*.jsonl.gz`）
