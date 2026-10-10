# War Thunder 本地 8111 接口规范（整合版）

**版本：** 1.0（随项目计划 v1.2 发布）　**日期：** 2026-10-10　**适用：** War Thunder Tactical Command 的 Windows Agent、服务器解析器与测试夹具

本规范把多个开源项目的实测记录、代码实现和文档合并为一份可执行的接口说明。8111 没有官方版本化契约，下列内容全部按“开放 schema”处理：未知字段要保留，已知字段可能缺失。

## 1. 来源与可信度标记

| 标记 | 来源 | 性质 |
|---|---|---|
| 〔Neo〕 | CreeperUX/WT-8111-Neo：`WT_8111_API_REFERENCE(.zh-CN).md`（2026-05-31 校对）、`src/lib/wt8111.ts`、`src-tauri/src/server.rs`、PR #3 `cloud-tactical-server` | 地面实测样本 + 代码 |
| 〔WTM〕 | NolanMullins/wt-modern-8111：`docs/localhost-8111-api.md`（2026-09-01 / 09-13 实测）、`docs/fixtures/`、`internal/` Go 代码 | 最详细的实测（JH-7 试飞、Air SB、20Hz 抓包） |
| 〔LVM〕 | lucasvmx/WarThunder-localhost-documentation：`openapi.yaml` 与各端点 md | 较早的空战样本（数字字符串格式） |
| 〔WP8F〕 | matrixsukhoi/wp8f：`core/`、`datalink/` | 30Hz 飞行数据 + 约 4Hz 地图采样的 HUD 实现 |
| 〔L18〕 | LucasZ3214/link18：`overlay.py`、`network.py`、`web/dashboard.js` | 10Hz 轮询、颜色判定实践 |
| 〔TM〕 | unfairbullet/WarThunder-Tactical-Map：`src/lib/tracking.ts` | 实测颜色调色板 |
| 〔WTP〕 | ElBartt/WarThunder-Plotter：`capture.py`、`map_hashes.py`、`config.py` | 地图图像哈希、陆战中切换飞机时的坐标变换 |
| 〔WTD〕 | HYLiangSheng/WTDashboard：`wtdb/unit_tracker.py`、README | 远程访问设置、丢失目标“幽灵”逻辑 |

文中 **【冲突】** 表示不同来源相互矛盾，**【待测】** 表示必须在真实对局中验证，**【实测 2026-10-10】** 表示用户 PC 首批真实数据（陆战试车 + 空战试飞）的结论，详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`。

## 2. 服务概述

- 游戏运行时在 `8111` 端口提供 HTTP 服务；没有健康检查端点，只能通过请求是否成功、`valid` 字段和 `mission.status` 推断状态〔WTM〕。
- 所有已知端点均为 `GET`。响应头包含 `Access-Control-Allow-Origin: *`、`Access-Control-Allow-Methods: GET, POST, OPTIONS`；`OPTIONS /state` 返回 200 空体；未发现任何有意义的 POST 操作〔Neo〕〔WTM〕。
- **远程访问（0.0.0.0）：**游戏设置“战斗设置 → 允许远程访问（Allow remote access）/ 端口 8111”〔WTD〕〔WTM〕。WTM 在 2026-09-01 实测中观察到服务监听 `0.0.0.0:8111`，从局域网用游戏 PC 的以太网地址访问成功；关闭该选项时是否只绑定回环尚未验证【待测】。因此不能假设 8111 只对本机可见。
- 本项目 Agent 始终访问 `http://127.0.0.1:8111`，不依赖远程访问开关；同时在安装说明中提醒：开启远程访问时同一局域网的任何设备都能读取原始数据，严禁路由器端口映射。

> **【实测 2026-10-10】端口与绑定：**用户 Windows PC 上 `127.0.0.1:8111` 和所有网卡的 8111 都 connect_refused（os error 10061），游戏 API 在 **9222 端口**（collector 0.1.3 自动探测）。9222 在 127.0.0.1 和 4 个局域网/虚拟网卡 IP 上都返回 200，即监听所有网卡。Agent **不得硬编码 8111**，应探测 8111、9222（wp8f 默认就是 `8111,9222`），并上报实际端口。Neo、WTM、link18、WTP、TM、WTD 都把 8111 写死，在这台机器上会失效。远程访问开关的状态未确认（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


## 3. 端点总表

| 端点 | 类型 | 用途 | 建议轮询 | 来源 |
|---|---|---|---|---|
| `/` | `text/html` | 游戏内置战术地图网页 | 不轮询 | Neo、WTM |
| `/indicators` | JSON | 仪表 / 车辆状态、内部载具名 | 10Hz（陆战可 5Hz） | 全部 |
| `/state` | JSON | 飞行物理状态（字段名含单位） | 10Hz；`valid:false` 时降到 1Hz | 全部 |
| `/map_info.json` | JSON | 地图范围、网格、`hud_type`、`map_generation` | 1Hz（状态切换期 2Hz） | 全部 |
| `/map_obj.json` | JSON 数组 | 地图对象（本人、友军、敌军、目标、机场、重生点） | 10Hz | 全部 |
| `/map.img`、`/map.img?gen=<n>` | `image/png` 或 `image/jpeg` | 当前地图底图 | 仅在 `map_generation` 变化或首次有效时 | 全部 |
| `/mission.json` | JSON | 任务状态与目标 | 1Hz | 全部 |
| `/gamechat?lastId=<id>` | JSON 数组 | 增量聊天 | 1Hz | 全部 |
| `/hudmsg?lastEvt=<id>&lastDmg=<id>` | JSON 对象 | 增量 HUD 事件 / 伤害消息 | 1Hz | 全部 |
| `/loc/map/primary_objectives?fmt=js` | JavaScript | 本地化字符串“PRIMARY GOALS” | 每局 1 次 | Neo、WTM |
| `/loc/map/secondary_objectives?fmt=js` | JavaScript | 本地化字符串“SECONDARY GOALS” | 每局 1 次 | Neo、WTM |

**已确认返回 404 的路径**（不要使用）：`/map_info`、`/map_obj`、`/state.json`、`/indicators.json`、`/map`〔Neo〕〔WTM〕。

## 4. 各端点详细说明

### 4.1 `/`（内置网页）

- 创建 640×640 正方形 canvas，把 `/map.img` 拉伸铺满；网格线和字母数字标签由 `map_min`、`map_max`、`grid_steps` 生成，**不使用 `grid_zero`**〔WTM〕。
- 每 500ms 请求 mission、map_obj、map_info、chat、hudmsg、indicators、state，每 25ms 重绘一次缓存地图〔WTM〕。即内置页面本身只有 **2Hz 数据刷新**。
- 本人为带深色描边的白色方向箭头（按 `atan2(dx,-dy)` 旋转）；其它对象颜色直接取自 `map_obj.json`；符号来自自定义字体 `icons.ttf`（588,472 字节，返回 `text/plain` 且无 CORS 头）〔WTM〕〔Neo〕。

### 4.2 `/indicators`

陆战实测样本〔Neo〕：

```code
{ "valid": true, "army": "tank", "type": "tankModels/ussr_t_80bvm",
  "stabilizer": 1.0, "gear": 1.0, "gear_neutral": 1.0, "speed": 0.0,
  "rpm": 1500.0, "lws": -1.0, "ircm": -1.0, "first_stage_ammo": 25.0,
  "crew_total": 3.0, "crew_current": 3.0, "gunner_state": 0.0, "driver_state": 0.0 }
```

空战样本〔WTM〕：

```code
{ "valid": true, "army": "air", "type": "jh_7", "speed": 189.13,
  "altitude_10k": 4936, "aviahorizon_roll": -0.08, "aviahorizon_pitch": -4.28,
  "compass": 355.83, "fuel": 3182.0, "gears": 0.5, "flaps": 0.0,
  "throttle": 0.0, "mach": 0.0, "g_meter": 0.92 }
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `valid` | bool | 是否可用 |
| `army` | string | `tank` / `air`（海战值未采样）；可作为本人兵种判定的辅助信号 |
| `type` | string | 内部载具名；陆战带 `tankModels/` 前缀，空战为裸名（如 `jh_7`） |
| `compass` | number | 空战出现；与 `map_obj` 本人朝向一致（见 §5.3） |
| `speed` 等 | number | 单位因载具而异：JH-7 的 `speed` 实为 m/s〔WTM〕 |
| `lws`、`ircm` 等 | number | `-1` 表示“未装备 / 不可用”的哨兵值，不得当作数据 |
| `crew_total` / `crew_current` | number | 陆战乘员数 |

- 字段集合随载具和座舱变化；同一控制量可能有多个别名（`throttle`、`throttle1`、`throttle_1`、`throttle1_1`）〔WTM〕。缺失字段表示“不可用”，不能视为 0。

> **【实测 2026-10-10】**
> - 陆战（Tiger H1 试车）`/indicators` 有 20 个字段，比 Neo 示例多出 `has_speed_warning`、`driving_direction_mode`、`cruise_control`、`roll_indicators_is_available`、`crew_distance`，`speed` 为整数 km/h，**没有 `compass`**。
> - 空战（IA-58A 试飞）有 80 个字段，含 `compass`/`compass1`、`pedals1…8`、`blister1…16` 等别名。
> - **机库中 `/indicators` 一直是 `valid:true`**，并带当前选中载具的 type；换车时会短暂（约 100–200 ms）变为 `{"valid": false}`。
> - 因此 indicators.valid **不能**用来判断是否在对局中（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


### 4.3 `/state`

- 字段名内嵌单位：`"H, m"`、`"TAS, km/h"`、`"IAS, km/h"`、`"M"`、`"AoA, deg"`、`"AoS, deg"`、`"Ny"`、`"Vy, m/s"`、`"Wx, deg/s"`、`"Mfuel, kg"`、`"Mfuel0, kg"`、`"throttle 1, %"`、`"RPM 1"`、`"manifold pressure 1, atm"`、`"oil temp 1, C"` 等，发动机编号从 1 开始、按实际存在发现〔WTM〕〔Neo〕。
- **陆战通常只返回 `{"valid": false}`**（T-80BVM、ZTZ-96A 均如此），陆战状态应使用 `/indicators` 与地图〔Neo〕〔WTM〕。`valid:false` 不等于“在机库”。

> **【实测 2026-10-10】**
> - 陆战试车中 214/214 次都是 `{ "valid": false }`（18 B），确认上述说法。
> - 空战试飞中 valid:true，37 个字段（双发）。
> - **机库中选中飞机时 `/state` 也是 `valid:true`**（H=32 m、TAS 0、gear 100%），所以 valid:true 同样不等于“在飞行或对局中”（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）

- 空战 JH-7 样本为 613 字节；同一份数据中 `power N, hp` 为 0 但推力非零——数值字段存在不代表适用〔WTM〕。

### 4.4 `/map_info.json`

两种 schema 代际（**类型会变**）：

```code
// 较早格式：数字字符串〔LVM〕（空战）
{ "grid_steps": ["3250.0","3250.0"], "grid_zero": ["-32768.0","32768.0"],
  "map_generation": "1", "map_max": ["32768.0","32768.0"], "map_min": ["-32768.0","-32768.0"] }

// 当前格式：数字〔Neo〕（陆战实测）
{ "grid_size": [1600.0,1600.0], "grid_steps": [225.0,225.0], "grid_zero": [1519.45,2497.25],
  "hud_type": 1, "map_generation": 1, "map_max": [4096.0,4096.0], "map_min": [0.0,0.0], "valid": true }

// 当前格式：数字〔WTM〕（JH-7 空战试飞实测）
{ "grid_size": [57584.11,64194.20], "grid_steps": [6400.0,6400.0], "grid_zero": [-24816.11,31426.20],
  "hud_type": 0, "map_generation": 1, "map_max": [32768.0,32768.0], "map_min": [-32768.0,-32768.0], "valid": true }
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `valid` | bool，可缺失 | 早期格式没有此字段 |
| `map_min` / `map_max` | `[number,number]` 或数字字符串 | 世界坐标范围；所有样本均为正方形 |
| `grid_steps` | 同上 | 网格线间距（世界单位） |
| `grid_zero` | 同上 | 网格原点；内置页面不使用，官方网格标签换算未解决 |
| `grid_size` | 同上，可缺失 | 语义未确认；空战样本中非正方形 |
| `hud_type` | number，可缺失 | 陆战样本为 1、空战样本为 0；推测与 HUD 类型相关，含义未官方确认【待测】 |
| `map_generation` | number 或字符串 | 底图变化计数；**是各客户端本地计数**，不能跨玩家作为战局 ID |

**陆空差异：**陆战范围通常为数千单位（实测 0..4096），空战为 ±32768（65,536 单位见方）。陆战玩家切换到飞机时，本机地图会换成“陆战中的空战视图”（见 §4.6），范围是否随之变化需实测【待测】。解析器必须同时接受数字与数字字符串；范围无效或缺失时输出“地图不可用”，不得补默认值〔WTM〕。

> **【实测 2026-10-10】**
> - 所有字段都是 number（float 数组、int 的 hud_type/map_generation），未见数字字符串。
> - 陆战试车：0..4096、hud_type 1，最终 grid 为 1600/225/[1519.449951171875, 2497.25]（与 Neo 示例逐字相同，Neo 样本就是同一试车靶场）。**但 valid=true 后约 1.3 s 内网格是临时值**（grid_size 4096、grid_steps 550、grid_zero [0,4096]），此时 map_generation 不变 → 解析器必须在每次 map_info 到达时更新网格。
> - **空战试飞（丹麦图）范围是 ±65536（边长 131,072）**，grid_steps 5500，grid_size [52719.40, 55385.30]，grid_zero [6494.30, 19547.5]，与 wp8f 测试夹具逐字相同。所以空战范围**不是固定的 ±32768**。
> - map_generation 为 26 → 27（每加载一次加 1，是本地计数）。
> - 机库中 map_info 为 `{"valid" : false}`（17 B）（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


### 4.5 `/map_obj.json`

```code
[ { "type": "ground_model", "color": "#faC81E", "color[]": [250,200,30], "blink": 0,
    "icon": "Player", "icon_bg": "none", "x": 0.421074, "y": 0.563430, "dx": 0.939689, "dy": -0.342029 },
  { "type": "ground_model", "color": "#fa0C00", "color[]": [250,12,0], "blink": 0,
    "icon": "MediumTank", "icon_bg": "none", "x": 0.518198, "y": 0.559460 },
  { "type": "airfield", "color": "#174DFF", "color[]": [23,77,255], "blink": 0, "icon": "none",
    "icon_bg": "none", "sx": 0.359126, "sy": 0.560636, "ex": 0.359155, "ey": 0.511808 } ]
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `type` | string | 对象类别（枚举见下，**非封闭**） |
| `icon` | string | 符号；本人固定为 `Player` |
| `icon_bg` | string | 观察到 `none` |
| `color` | string | CSS 十六进制，大小写混用（`#faC81E`） |
| `color[]` | `[r,g,b]` | RGB 数组；早期样本与合成夹具中可能缺失 |
| `blink` | number | 0 不闪、1 普通闪烁、2 强闪烁 |
| `x`、`y` | number | 归一化坐标 0..1，左上为原点，y 向下 |
| `dx`、`dy` | number，可缺失 | 方向单位向量；本人、飞机（敌我均有〔LVM〕）、可旋转重生点具备 |
| `sx`、`sy`、`ex`、`ey` | number | 仅 `airfield`：跑道两端点，替代 `x/y` |

**`type` 已观察值：**`ground_model`、`aircraft`、`airfield`、`capture_zone`、`defending_point`、`bombing_point`、`respawn_base_tank`、`respawn_base_fighter`、`respawn_base_bomber`、`point_of_interest`〔Neo〕〔WTM〕〔LVM〕〔WP8F〕。

**`icon` 已观察值：**`Player`、`Fighter`、`Bomber`、`Assault`、`LightTank`、`MediumTank`、`HeavyTank`、`Tracked`、`Wheeled`、`SPAA`、`SAM`、`Airdefence`、`Structure`、`waypoint`、`capture_zone`、`bombing_point`、`defending_point`、`respawn_base_tank`、`respawn_base_fighter`、`respawn_base_bomber`、`none`；合成夹具中占领点 icon 为 `A` / `B`〔WTM〕。必须提供未知图标的兜底渲染。

**颜色与阵营（敌我判定的唯一依据是颜色）：**

| 颜色 | 观察到的对象 | 判定 | 来源 |
|---|---|---|---|
| `#faC81E`（250,200,30）黄 | 本人 `Player` | 本人 | Neo、WTM 实测 |
| `#ffffff` 白 | 本人 `Player` | 本人 | WTM 合成夹具 **【冲突】**：实测为黄色；本人一律以 `icon=="Player"` 判定 |
| `#fa0C00`（250,12,0） | 敌方坦克、SAM、SPAA、轰炸点 | 敌 | Neo、WTM、L18、TM |
| `#f00C00` | 敌方飞机（JH-7 试飞） | 敌 | WTM |
| `#fa3200`（250,50,0） | 敌方飞机、敌方机场 | 敌 | LVM、TM |
| `#f01e00`、`#fa0a00`、`#ff0000` | 敌方 | 敌 | TM、L18 |
| `#174DFF`（23,77,255） | 友方机场、友方地面单位 | 友 | WTM（机场实测）、L18 |
| `#185AFF`（24,90,255）、`#145cff`、`#043fff` | 友方飞机、机场 | 友 | LVM、TM、L18、WTM |
| `#17ffff`、`#14cfff` 青 | 友方 | 友 | TM |
| `#39d921`、`#24d921`、`#67d756`、`#54ff54`、`#00ff00` 绿 | 小队 / 连队成员 | 友 | WTM、TM |
| `#f4b13d` 橙 | 占领点 | 中立 / 目标 | WTM 合成夹具 |

- **【冲突】绿色：**Neo（`inferAffiliation`）把绿色（g>150 且 r<150）判为 **neutral**；WTM、TM、L18 把绿色判为 **友军（小队成员）**。本项目采用后者。
- **【冲突】非玩家飞机：**WTM 后端把所有非 `Player` 的 `aircraft` 计入 HostileAir（与其 Air SB 场景一致：Sim 中队友飞机不出现在 map_obj）；LVM 空战样本中存在大量蓝色友方飞机。按颜色判定，不按类型推断阵营。
- 玩家可在游戏设置中自定义标记颜色，L18 因此改为“明确的红色系判敌，其余判友”（提交 771698e）。推荐规则：`r ≥ 200 且 g < 130 且 b < 130` → 敌；`icon=="Player"` → 本人；`type=="airfield"` 仍按颜色判敌我；其余非红色 → 友；无颜色 → 未知。Agent 原样上传颜色，判定在服务器完成，并允许按客户端校准（用本人颜色与已知友方重生点 / 机场颜色学习调色板）。
- 本人在发出“注意地图（Attention to the map）”后约 5 秒会变为**蓝色闪烁**状态〔WTM〕，颜色变化不能当成状态变化。

> **【实测 2026-10-10】调色板（默认设置，试车）：**
> - 本人 `#faC81E` blink 0；**空战出生后约 2 s 本人为 `#043FFF` blink 2**（20 帧）
> - 敌方地面单位 / 轰炸点为 `#fa0C00` blink 0；同一批靶车出生后约 11 s 内会切换为 `#f00C00`（240,12,0）blink 2，之后恢复，即**同一对象会换色**
> - 敌机始终为 `#f00C00` blink 2
> - 友方机场为 `#174DFF`
> - 未见友军单位和小队绿色
>
> 规则 r≥200 && g<130 && b<130 能覆盖两种红色。link18 `isFriendlyColor` 会把 `#f00C00` 误判为友军，不可照搬。
>
> 新图标 **`TankDestroyer`**。海军对象的 type 是 **`ground_model`**（icon 为 BattleShip/Boat/Destroyer/HeavyCruiser），判定海军必须看 icon。非玩家地面单位 0% 带 dx/dy，敌机 100% 带 dx/dy。所有对象都带 `color[]`，`icon_bg` 恒为 `none`（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


**可见性：**据报道游戏只输出本机战术地图当前显示的对象（隐藏对象被省略，而不是带隐藏标志）【待测】〔WTM〕〔Neo〕。WTM 在 Air SB 实测中确认**远端队友飞机无法从 map_obj 获得**，只能从聊天中的网格 + 高度文本获得；普通“注意地图”标记**不产生** `point_of_interest` 对象，只有本人对象闪烁〔WTM〕。陆战中友军在地图上可见，敌军仅在被发现 / 标记时出现（Neo 陆战样本含 7 辆红色 MediumTank）。

### 4.6 `/map.img` 与 `/map.img?gen=<n>`

- 返回 `image/png` 或 `image/jpeg`，扩展名不代表编码，代理必须透传 `Content-Type`〔Neo〕〔WTM〕。
- 在同一 `map_generation` 内静态不变；`/map.img` 与 `/map.img?gen=1` 字节相同；无缓存头〔WTM〕。应在新 generation 下载成功后原子替换，临时失败期间保留旧图。
- **尺寸不固定：**Air Test Flight（JH-7）返回的是 **128×128、1,519 字节的灰色问号占位图**（91.94% `#404040`），不是地形图〔WTM〕；WTP 的哈希表把这张图标为 `no_map`。真实空战 / 陆战底图的尺寸需实测【待测】；Neo 云端接口文档示例为 323,235 字节 JPEG。
- **陆战中切换为飞机：**WTP 的哈希表区分 `*_tankmap`（陆战视图）与 `avg_*_map`（“陆战中的空战视图”，52 张），说明陆战玩家驾驶飞机时 8111 底图会整体换成更大范围的空战视图，**所有对象的归一化坐标也随之换算到新视图**。WTP 在切换的相邻两帧中按 icon 匹配至少 3 个地面对象，拟合 `x_air = a·x_ground + b`、`y_air = c·y_ground + d`（缩放上限 0.5，残差上限 0.01）〔WTP〕。核弹事件会把陆战图切到同名纯空战图。
- 地图识别：WTP 用 16×15 灰度差分哈希（dHash，225 bit，57 位十六进制），以汉明距离 ≤42 模糊匹配 229 条已知地图哈希（空战 18、陆战、陆战中空战视图 52、海战等）〔WTP〕。

> **【实测 2026-10-10】map.img：**
> - 陆战试车：JPEG 2048×2048，506,117 B，sha256 `3a7d46ab…c33e`，dHash 匹配 WTP `firing_range_tankmap`（汉明距离 20）。
> - **空战试飞：JPEG 2048×2048，202,995 B，sha256 `7e6edd02…c45f`，是真实地形图**，dHash 匹配 `air_denmark_map`（距离 23），**不是**占位图。WTM 的 JH-7 试飞拿到占位图，说明这取决于试飞所用地图。
> - 占位图（PNG 128×128，1,519 B，sha256 `cfb99c04…07d0`，dHash 与 WTP `no_map` 精确相同）在机库和加载期返回。
> - `/map.img?gen=N` 与 `/map.img` 字节相同（2/2）。
> - 本地 WTP 哈希表实际为 227 条（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


### 4.7 `/mission.json`

```code
{ "status": "running", "objectives": [ { "primary": true, "status": "in_progress", "text": "Take off" } ] }
{ "status": "running", "objectives": null }
```

- `objectives` 可为 `null`；`status` 观察到 `running`，失败值 **【冲突】**`fail`〔LVM〕与 `failed`（Neo 预期）并存，未知字符串必须原样保留。目标 `status`：`in_progress`、`completed`、`failed`。

> **【实测 2026-10-10】**机库与两段试车中都是 `{"objectives": null, "status": "running"}`，没有变化（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


### 4.8 `/gamechat?lastId=<id>`

```code
[ { "id": 123, "time": 456.7, "mode": "Team", "sender": "PlayerName", "msg": "...", "enemy": false } ]
```

- 返回 `id > lastId` 的记录；`time` 可缺失。**ID 只在游戏启动时归零，下一局延续上一局编号**〔LVM〕〔WTM〕。轮询器必须识别 ID 回退（游戏重启）。首次连接用 `lastId=0` 会拿到整段历史。
- Air SB 中“Guide on me / Follow me / Cover me”等位置呼叫在远端队员的聊天文本中带 `[C5, alt. 600 m]` 网格与高度；“Attention to the map”只带目标网格 `[B4]`；发送者本人看不到坐标后缀〔WTM〕。
- 消息与玩家名是不可信文本，只能按文本渲染。

> **【实测 2026-10-10】**机库和试车中 `/gamechat?lastId=0` 一直返回 `[\n\n]`（无聊天）（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


### 4.9 `/hudmsg?lastEvt=<id>&lastDmg=<id>`

```code
{ "events": [], "damage": [ { "id": 161, "msg": "A (Spitfire) set afire B (Spitfire)",
  "sender": "", "enemy": false, "mode": "" } ] }
```

- `events` 在当前版本中通常为空，`damage` 含击毁 / 起火 / 坠毁文本；ID 语义同 gamechat〔LVM〕〔WTM〕。客户端以各自数组最后一条的 id 作为下次游标〔Neo〕。

> **【实测 2026-10-10】**
> - `events` 恒为空。
> - `lastDmg=0` 返回此前真实对局的 158 条历史（id 1217–1374，约 20.8 KB）。
> - 试车中击毁靶车会产生新的 damage 消息（id 1375）。
> - damage 条目带 **`time` 字段**（任务内秒数，每局重置，不单调）。
> - 中文文本中的字符之间插有零宽空格 U+200B（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


### 4.10 `/loc/map/*_objectives?fmt=js`

- 返回可执行的 JavaScript 赋值语句（非 JSON），英文值为 `"PRIMARY GOALS"` / `"SECONDARY GOALS"`〔WTM〕。Agent 原样作为文本上传。

> **【实测 2026-10-10】**中文客户端返回 `loc_tbl['map/primary_objectives'] = "主​要​目​标"`（含 U+200B），Content-Type 为 `application/javascript`，说明返回值随客户端语言变化（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


## 5. 坐标、距离与朝向

### 5.1 换算公式

```code
pixel_x = image_width  * x            pixel_y = image_height * y
world_x = map_min_x + x * (map_max_x - map_min_x)
world_y = map_min_y + y * (map_max_y - map_min_y)
x = (world_x - map_min_x) / (map_max_x - map_min_x)    // 逆变换
```

- y 轴向下（图像坐标）；正北为图像上方。没有经纬度映射，范围是游戏世界单位（通常按米理解）〔WTM〕〔Neo〕。
- 底图必须保持正方形几何，拉伸会扭曲距离和方位〔WTM〕。

### 5.2 距离与方位

```code
dx_world = (tx - sx) * (map_max_x - map_min_x)
dy_world = (ty - sy) * (map_max_y - map_min_y)
distance = sqrt(dx_world² + dy_world²)
bearing  = (atan2(dx_world, -dy_world) * 180/π + 360) mod 360     // 0 = 北，顺时针
```

- Neo 的 `bearingBetween()` 直接用归一化差值计算，只有在范围为正方形时才等价；本项目统一用世界坐标差值〔Neo 代码〕。

### 5.3 朝向

```code
heading = (atan2(obj.dx, -obj.dy) * 180/π + 360) mod 360
```

- **已实测验证：**100 个 JH-7 样本中与 `/indicators.compass` 的平均差 0.000408°、最大 0.001041°〔WTM〕。Neo、WP8F、L18 均采用同一公式；Neo 云端协议把它量化为 `heading × 100` 的整数。

> **【实测 2026-10-10】**
> - 空战（IA-58A）185 对比对，平均 \|误差\| 0.0037°，p95 0.00005°，最大 0.36°（转弯中两次请求不在同一帧），与 WTM 一致。
> - **陆战 `/indicators` 没有 compass**，无法直接比对；dx/dy 方向与行驶轨迹方位的中位差 0.04°、最大 1.6°（n=25），很可能是车体朝向，是否随炮塔变化待测（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


### 5.4 量化精度（供协议设计）

坐标量化为 u16（0..65535）时：陆战 4096 单位地图分辨率约 0.06 单位，空战 65,536 单位地图约 1 单位，均远小于图标尺寸；朝向 `deg × 100` 精度 0.01°〔Neo 协议〕。

## 6. 本人识别与本人状态信号

- 本人 = `icon == "Player"` 的对象（所有来源一致）；颜色不可靠（黄 / 白 / 蓝闪烁）。
- 本人对象的 `type` 区分 `ground_model` 与 `aircraft`，可直接判定陆战玩家是否正在驾驶飞机；`/indicators.army` 作为交叉验证。
- 阵亡、重生 / 选车、观战、回机库期间的 map_obj / map_info / mission 变化没有文档化状态机〔WTM〕【待测】。WTP 以 `map_info.valid` 为对局有效信号，失效后 20 秒宽限期再判定对局结束〔WTP〕。

> **【实测 2026-10-10】进出试车时序：**
> - 加载开始：HTTP 约 3 s 全部无响应（read_timeout）。
> - 进入时：map_obj 的 Player 出现在 map_info valid=true 之前约 0.2 s（陆战）；空战则是 valid=true 之后 1.8 s 才出现 Player；map.img 在加载结束时切到真实地图。
> - 退出时：Player 消失，同时 indicators 短暂 valid:false；约 0.6 s 后 map_info 变为 valid=false；map.img 回到占位图。
> - 空战试飞中途暂停约 1.4 s 时，所有数据冻结但 HTTP 正常返回。
> - 对局判定应只用 `map_info.valid`（加上 Player 是否存在）（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


## 6A. 小队成员与地图标点（标点）

> **【实测 2026-10-10，实战陆战 RB】**完整报告见 `/workspace/wt-analysis/SQUAD_PINGS_20261010.md`
> - **小队成员**是绿色：陆地 `#67D756`、空中 `#39D921`，blink 0，每帧最多 1 个（自动组队）。普通队友是 `#174DFF`，刚出生的友军短时为 `#1839A7`（陆）/ `#134AFF`（空）。已确认（20 条小队轨迹，与 HUD“团队协作”吻合）。
> - **发信号**：任何友方单位（含本人 Player）在发出无线电或标点时变为 `#043FFF` + blink 2，**盖住原色**。时长：标点约 4.9 s，位置呼叫约 5.9 s，“谢谢”约 1.9 s。判定小队/队友身份必须看闪烁前后的轨迹颜色。
> - **观战**：本人阵亡后 `Player` 变为 `#174DFF` 并跟随被观战的队友。本人只能是 `Player` 且颜色为 `#faC81E`，或者 `#043FFF` 且 blink 2。§6 中“本人 = icon==Player”的规则据此修正。
> - **地图标点不产生 map_obj 对象**：全部会话中 `point_of_interest` 为 0，也没有新的 type 或 icon。标点只表现为两点：(1) gamechat 文本 `注意战术地图！ [c4]` / `注意该坐标位置！ [b6]` / `注意当前标识区域！ [f8]`，网格就是标点目标（18/18 不等于发送者所在格）；(2) 发送者闪烁。
> - **位置呼叫** `请求掩护！ [e6]`、`请求支援！`、`请求指示空袭目标 [a14, 高度 1000 米]` 中的网格是发送者所在格（10/10）。
> - **本人发出的消息不带网格后缀**，所以本人标点的目标位置无法从 API 取得，只能看到本人闪烁。
> - 标点在游戏地图上的显示时长 API 不提供；网页暂用 30 s TTL（推断值）。
> - 网格换算：`x0 = grid_zero.x + (列-1)·grid_steps.x`，`y0 = grid_zero.y + (行字母序-1)·grid_steps.y`，再用 map_min/max 归一化（已用 10/10 位置呼叫验证；切换飞机视图时可能越界）。
> - 首次 `lastId=0` 的历史聊天必须丢弃（df9b 有 13 条）。
> - **项目决定（0.2.2）**：标点统一显示，不按发送者（本人/队友/小队）区分；发送者只作为可选 meta。
> - 未确认：小队成员自己打标点的表现（推断与队友相同）、手动组队与连队战的绿色取值、色盲模式。

## 7. 陆战与空战差异汇总

| 项目 | 陆战 | 空战 | 来源 |
|---|---|---|---|
| `/state` | 通常 `{"valid":false}` | `valid:true`，完整飞行数据 | Neo、WTM |
| `/indicators.army` | `tank` | `air` | Neo、WTM |
| `hud_type` | 1 | 0 | Neo、WTM（各 1 份实测）【待测】 |
| 地图范围 | 例：0..4096 | ±32768 | Neo、WTM、LVM |
| `grid_steps` | 例：225 | 6400（实测）/ 3250（早期） | Neo、WTM、LVM |
| `map.img` | 地形图（JPEG/PNG） | 试飞为 128×128 占位图；实战空战图尺寸待测 | WTM、WTP |
| 对象 | 友军全部可见，敌军发现后可见，占领点、重生点 | 飞机带 dx/dy；地面目标、机场线段；Sim 中队友不可见 | Neo、LVM、WTM |
| 陆战中驾驶飞机 | 底图换成“陆战中的空战视图”，对象坐标重新归一化 | — | WTP |

> **【实测 2026-10-10】**试车/试飞实测：
> - hud_type 陆战 1、空战 0 ✔
> - 地图范围：陆战 0..4096 ✔；空战 **±65536**（丹麦）
> - grid_steps：陆战 225；空战 5500
> - map.img：陆战与空战试飞都是 2048² JPEG（不是占位图）
> - /state：陆战 false ✔，空战 true ✔
> - 陆战 indicators 无 compass（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


## 8. 更新行为与轮询建议

| 实现 | 轮询方式 | 来源 |
|---|---|---|
| 游戏内置网页 | 全部端点 500ms（2Hz） | WTM |
| WTM 实测 | 顺序请求 100 次 / 10.775s（约 9Hz）：`/state` 均值 3.21ms、`/indicators` 2.66ms、`/map_obj.json` 2.82ms，p95 ≤4.3ms，0 错误，**飞行中每次响应都有变化（99/99）** | WTM |
| WTM 生产轮询 | 快组 100ms（state、indicators、map_obj）、慢组 1s（map_info、mission、chat、hud） | WTM 代码 |
| Neo | 全端点 250ms；地图 500ms；云端 relay 上传 500ms（2Hz） | Neo 代码 |
| WP8F | state / indicators 30Hz；map_obj 每 8 帧（约 3.75Hz） | WP8F |
| L18 | 100ms（10Hz），超时 0.3s | L18 |
| WTP | 200ms（5Hz） | WTP |

**本项目建议：**map_obj / indicators / state 10Hz（陆战 `state` 无效时降到 1Hz）；map_info、mission、gamechat、hudmsg 1Hz；map.img 仅在 generation 变化时；同一端点请求不得重叠，慢响应顺延；每个端点单独记录状态、最后成功时间和错误。9Hz 实测仅针对一台机器的空战试飞，陆战与 8 人实战下的新样本率仍需测量（DATA-01/02）。

> **【实测 2026-10-10】10 Hz 实测：**
> - 空战试飞：暂停外 map_obj、indicators、state 都是 169/169 有变化，所以游戏刷新率 ≥ 10 Hz，上限需 20 Hz 采样确认。
> - 陆战试车：行驶中 map_obj 和 indicators 都是 100% 有变化；静止时 map_obj 55%、indicators 22%、state 0%。
> - 延迟：p50 7.5–8.3 ms，p95 20–24 ms，p99 43–65 ms（7 端点并发，比 WTM 慢约 2.5 倍）；机库 p99 < 30 ms。
> - Windows 计时器 15.6 ms 量化使请求间隔在 94/109 ms 之间交替。
> - 每次进出地图 HTTP 约卡 3 s。
> - 约每 2 分钟出现一次 os error 10053（需重试一次）。
> - 带宽（gzip 后）：机库约 5.9 MB/h，陆战试车约 4.5 MB/h，空战试飞约 24 MB/h；map.img 应只在 generation 变化时拉取（详见 `/workspace/wt-analysis/REAL_DATA_REPORT_20261010.md`）


## 9. 类型与解析怪癖

1. `map_info` 数值可能是数字字符串（`"3250.0"`、`"1"`）。
2. 缺失字段 ≠ 0；`-1` 是“不可用”哨兵值；`valid` 优先。
3. 响应体可能为空字符串（Neo `fetchJson` 将空体视为 null）。
4. `color` 大小写不一致；`color[]` 可能缺失，需从十六进制解析。
5. `icon` / `type` 不是封闭枚举；`airfield` 没有 `x/y`。
6. `map.img` 编码不固定；`icons.ttf` 以 `text/plain` 返回且无 CORS。
7. 聊天 / HUD ID 跨对局延续，仅游戏重启归零。
8. 指示器单位因载具而异（m/s 与 km/h、英制座舱）。

## 10. 未决问题（需真实对局验证）

1. 陆战 RB / AB / SB 与空战 AB / RB / SB 下 map_obj 的敌我可见范围是否与游戏内地图完全一致；Sim 模式是否省略友军。
   > **【实测 2026-10-10】部分：**试车中靶车和敌机全部可见、没有友军；实战各模式仍待测。
2. 陆战玩家驾驶飞机时 `map_info` 的 `map_min/max`、`hud_type`、`map_generation` 是否变化，以及切回坦克时的行为。
   > **【实测 2026-10-10】仍待测：**本次是独立的陆战试车和空战试飞，不是陆战中切换飞机。独立任务之间 map_generation（26→27）、范围（0..4096 → ±65536）、hud_type（1→0）都会变化。
3. 实战空战图和“陆战中空战视图”的 `map.img` 尺寸、编码与是否随 generation 变化。
   > **【实测 2026-10-10】部分：**陆战试车和空战试飞（丹麦）都是 2048² JPEG，在同一 generation 内不变；128² 占位图只出现在机库和加载期。实战与陆战中空战视图仍待测。
4. `hud_type` 的完整取值与含义；`grid_zero` 到官方网格标签的换算。
   > **【实测 2026-10-10】部分：**陆战 1、空战 0 已确认（与 Player type 一致）；其他取值和 grid_zero 换算仍待测。另外发现陆战进图后约 1.3 s 内网格为临时值。
5. 阵亡、重生、观战、回机库时的本人对象与 `valid` 变化顺序（DATA-04）。
   > **【实测 2026-10-10】部分：**进出试车的时序已记录（见 §6 注释）；阵亡、观战、重生仍待测。
6. 陆战下 map_obj 新样本率（静止 / 移动）与 8 人实战中的响应时间。
   > **【实测 2026-10-10】已回答（单人试车）：**行驶中 100%（约 9.6 Hz），静止时 55%；延迟 p50 7.5 ms、p95 21 ms、p99 65 ms。8 人实战仍待测。
7. 玩家自定义颜色 / 色盲模式下的实际调色板；小队成员绿色在连队战中的取值。
   > **【实测 2026-10-10】部分：**默认调色板见 §4.5 注释（包括 #f00C00/blink2 高亮态和出生时本人 #043FFF 蓝闪）；色盲模式仍待测。**小队绿色已在实战中确认**：陆 `#67D756`、空 `#39D921`，见 §6A。
8. `point_of_interest` 在哪些模式、哪些操作下出现。
   > **【实测 2026-10-10】仍待测：**试车和 8+ 局实战中都未出现。地图标点不产生 point_of_interest，见 §6A。
9. `mission.status` 的完整集合（`fail` / `failed`、结束、回放）。
   > **【实测 2026-10-10】仍待测：**只见过 `running`（objectives 为 null）。
10. HUD `events` 是否会有内容；POST 是否有任何作用。
   > **【实测 2026-10-10】部分：**events 在机库和试车中恒为空；damage 带 `time` 字段，试车击毁也会产生消息；POST 未测。
11. 关闭“允许远程访问”时 8111 是否仍监听 0.0.0.0。
   > **【实测 2026-10-10】部分：**本机游戏在 **9222** 端口监听所有网卡（4 个局域网/虚拟网卡 IP 都返回 200）；开关状态未知，关闭后的绑定行为仍待测。

## 11. 参考链接

- https://github.com/CreeperUX/WT-8111-Neo （含 PR #1/#2/#3 与 `codex/*` 分支）
- https://github.com/NolanMullins/wt-modern-8111 （`docs/localhost-8111-api.md`、`docs/sim-live-capture.md`）
- https://github.com/lucasvmx/WarThunder-localhost-documentation （`openapi.yaml`）
- https://github.com/matrixsukhoi/wp8f
- https://github.com/LucasZ3214/link18
- https://github.com/unfairbullet/WarThunder-Tactical-Map
- https://github.com/ElBartt/WarThunder-Plotter
- https://github.com/HYLiangSheng/WTDashboard
- https://forum.warthunder.com/t/tools-using-data-provided-on-port-8111/106664
