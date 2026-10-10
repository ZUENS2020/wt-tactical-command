<div class="cover" markdown="1">
<h1 class="title">War Thunder Tactical Command</h1>
<p class="subtitle">V10 项目实施计划｜双地图战术标绘系统</p>

<div class="meta" markdown="1">
**版本：** 1.2（基于 UI V10 冻结需求；在 1.1 基础上修订）<br>
**日期：** 2026-10-10<br>
**状态：** 可进入实现；以 WT-8111-Neo 为架构基线复用，8111 实际采样能力、陆空坐标变换与敌我颜色尚需实测<br>
**部署目标：** 一台香港 VPS（绑定域名）+ 全量上传的 Windows Agent + 手机 / PC 响应式网页<br>
**适用场景：** 《战争雷霆》8v8 连队战、空地混合战术协同（内部使用）
</div>
</div>

<div class="note" markdown="1">
**一句话定义：** 所有人都能打开同一张实时战术地图，在陆战、空战、双地图间切换；每台 Agent 把本机 8111 的全部数据上传到服务器，由服务器融合、存储并按策略决定显示哪些内容；队员通过单地图模式以 TacOps 式的军标与战术图形（APP-6 符号、进攻箭头、阶段线、区域、测距尺）直接标绘。它是地图工具，不是战况管理后台。
</div>

## 版本 1.2 修订说明 { .changelog }

本版本在 1.1 的基础上做以下已确认的修订；未列出的 1.1 决策保持不变：

| # | 修订项 | 要点 | 涉及章节 |
|---|---|---|---|
| 1 | 以 WT-8111-Neo 为架构基线 | 直接 fork 复用：Rust 本地 8111 代理（Agent 底座）、TS 8111 类型与坐标函数、PR #3 cloud-tactical-server（房间、relay / viewer WebSocket、Protobuf 信封、地图图片 PUT/GET、配置与 Docker）、u16 坐标量化、heading×100、NTP 式时钟同步、keyframe / delta 设计；同时列出 Neo 原型中必须修正的缺陷 | 0、4、4.1、4.2、6、8、11、12 |
| 2 | 以 tacops.gg 为标绘交互与军标模型 | APP-6 军标（milsymbol）、进攻箭头、阶段线、区域、圆 / 射程圈、文字、测距尺（多航点）、现状 / 计划状态、时间标签、图层（锁定 / 隐藏）、简报 / 演示跟随模式；改造为无账号、无角色差异的房间模式 | 2、3.3、3.5、3.6、6.1、8 |
| 3 | Agent 全量上传 | Agent 上传全部 8111 端点与全部地图对象（含敌军）、全部地图图片；服务器决定融合、存储与显示内容。重写范围约束、“本期不做”、Agent 过滤规则、协议、带宽、存储保留期与安全 | 0、1、2.4、5、6、7、9、10 |
| 4 | 服务器端敌情显示策略 | 新增 `enemyPolicy`（off / team_visible / fused）与按模式上限；风险表加入 Gaijin 论坛版主关于“无标记模式显示敌方标记可能被视为 ESP”的事实说明；全量上传设计保持不变 | 0、5.7、9.2、10 |
| 5 | 复用优先 | 删除所有基于许可证的限制；开源代码、文档、夹具、地图哈希表直接复用 | 3.3、11、12 |
| 6 | 协议改为 Protobuf | Agent↔服务器、服务器↔网页 WebSocket 均使用 Protobuf 二进制帧（prost / protobuf-es）；REST 管理接口保持 JSON；保留 JSON 调试镜像 | 4、6.2 |
| 7 | 新增 8111 接口规范 | 整合 8 个开源项目的实测与代码，形成附录 A（独立文件 `8111_API_SPEC.md`）：全部端点、字段类型、陆空差异、坐标与朝向公式、敌我颜色表、轮询建议、怪癖与未决问题 | 5、附录 A |
| 8 | 研究结论修正早期假设 | 空战试飞的 map.img 是 128×128 占位图；陆战中驾驶飞机会整体切换底图与对象坐标系；绿色是小队友军而非中立；Sim 模式 map_obj 不含队友飞机；8111 约 9Hz 可用且飞行中每次有变化 | 5.1–5.5、附录 A |
| 9 | 排期缩短 | 复用 Neo 后由 4–6 周缩短为约 3.5–5 周；第一周竖切缩短为 3–4 天；重排 Issue 列表 | 8、8.1、8.2 |

### 版本 1.1 修订说明（历史摘要）

| # | 修订项 | 要点 |
|---|---|---|
| 1 | HTTPS 改用域名 | 域名解析到香港 VPS，Caddy 自动签发 / 续期 Let's Encrypt 证书 |
| 2 | “本人”状态缺失 | 阵亡、重生、观战、陆战玩家驾驶飞机时显式上报状态，网页灰显最后已知位置 |
| 3 | 减轻 P1 | 钢笔曲线移至首次实战测试后；MVP 采用服务器定序 + 后写覆盖 + 撤销 |
| 4 | 战局识别独立成项 | BATTLE-01：mission.json 状态转换 + 地图哈希 / 范围跨提供者匹配 |
| 5 | Windows Agent 分发 | AGENT-02：图文安装说明，评估代码签名 |
| 6 | 排期调整 | 第一周竖切作为首个里程碑 |



## 0. 本版本确认的设计决策

| 主题 | 已决定的方案 |
|---|---|
| 页面尺寸 | **整页锁定浏览器可用视口；无水平、垂直滚动；**适配手机竖屏、横屏、桌面 |
| 地图外观 | **地图视口始终 1:1 正方形，**优先最大化占据顶栏以下的高度；图片等比绘制，严禁拉伸 |
| 顶栏 | 仅保留应用标识、陆战 / 空战 / 双图切换、指挥 / 成员 UI 切换及必要连接指示（含敌情策略徽标） |
| 单地图 | **桌面和横屏：左侧尽可能大的正方形地图 + 右侧纯标绘工具；**窄屏竖屏：地图在上、紧凑工具在下 |
| 双地图 | 同时显示陆战图和空战图，**操作栏完全消失；**宽屏左右并排、窄屏上下排列；两张图分别独立缩放 |
| 功能主线 | TacOps 式标绘：APP-6 点军标、进攻箭头（首轮为折线骨架）、阶段线、区域、测距尺、人工攻击来源；地图显示实时友军位置及朝向，自动敌情按策略显示 |
| 两种 UI | 指挥 / 成员只是**默认布局和控件密度不同；无角色级操作权限差异，**任何成员均可切换 |
| 数据上传 | **每台 Windows Agent 上传全部 8111 端点、全部地图对象（含敌军）和全部地图图片；**服务器决定融合、存储与显示；不要求指挥员运行游戏 |
| 双地图来源 | 至少 1 名空战成员保持空战地图，陆战成员提供陆战地图；服务器分别缓存两种图片与坐标参数，并利用双方同时看到的对象自动拟合陆空变换 |
| 架构基线 | **fork WT-8111-Neo：**Agent 底座 = Neo `src-tauri` Rust 代理；服务器底座 = Neo PR #3 `cloud-tactical-server`；前端复用 Neo 8111 类型与坐标函数 |
| 标绘模型 | **以 tacops.gg 为交互与军标模型，**milsymbol 渲染 APP-6；现状 / 计划状态、时间标签、图层、简报模式 |
| 后端 | 香港 VPS；单实例 Rust + Axum + Tokio + prost；SQLite；原始数据分段文件；无需 Redis / Kafka |
| 协议 | WebSocket 二进制 Protobuf（Agent 与网页两条通道）；REST 管理接口 JSON |
| 敌情显示 | **服务器端 `enemyPolicy` 开关：**off / team_visible / fused，房间级设置 + 按对局模式设上限；默认 off，变更全房间可见并记录 |
| HTTPS 入口 | **购买域名解析到香港 VPS（无需 ICP 备案）；**Caddy 自动签发 / 续期 Let's Encrypt 证书 |
| 编辑并发 | MVP 采用**服务器定序确认 + 后写覆盖（last-write-wins）+ 撤销；**保留 operationId 去重与服务器版本号，不做冲突提示 UI |
| 频率 | **10Hz 是端到端态势同步目标；**Agent 以 10Hz 轮询快组端点，实际新样本率须首先测量 |
| 本期不做 | 游戏内 Overlay、鼠标监听、屏幕 / 雷达画面识别、战损报告、死亡自动标记、载具受损部位管理、编队状态栏、回放 UI（原始数据已保留，回放界面留待下一轮） |

**范围约束：**全量上传意味着服务器拥有每台 Agent 看到的全部对象（含敌军）；“显示什么”由服务器策略决定，而不是由 Agent 过滤。人工敌方位置、型号简写和攻击来源箭头依旧属于基础标绘；自动敌情是一个可开关的独立图层，不做独立情报管理后台。

## 1. 使用场景与关键流程

### 1.1 标准连队协同

1. 队员在 PC 上启动《战争雷霆》和 Agent；Agent 自动读取本地 127.0.0.1:8111 的全部端点并上传。
2. 至少一名飞行员使用空战地图类型、一名陆战成员使用陆战地图类型，为房间提供两套底图和坐标元数据；所有 Agent 的地图图片都会上传，服务器按哈希去重。
3. 队员和指挥员在手机 / PC 浏览器中进入同一房间。浏览器可选陆战、空战或双图模式。
4. 地图显示全队友军：运行 Agent 的队员带昵称和朝向；未运行 Agent 但出现在任一 Agent 地图上的友军以匿名友军符号显示。自动敌情图层按房间 `enemyPolicy` 显示或隐藏。
5. 队员在单图模式用 APP-6 军标、进攻箭头、阶段线、区域标注集合点、目标与进攻路径；服务器立即同步到所有网页，同一份世界坐标正确投影到两张图。
6. 若受到不明方向攻击，队员用“来源”工具画出受袭点与疑似来袭方向，明确标识为人工报告 / 未确认。
7. 战前或战中需要统一讲解时，任一成员可进入简报模式并“开始演示”，其他人的视口跟随演示者（可随时退出跟随）。

### 1.2 手机操作原则

- 单图是编辑工作台；双图是态势总览，切换不改变用户权限。
- 触控点击地图不会误触触发工具；只有显式选择工具后才创建标绘。
- 地图双指缩放、单指平移与绘制手势要区分。绘制中允许取消、撤销上一节点、完成（与 TacOps 的触控操作条一致）。
- 所有必要操作都能通过点击按钮完成，不能只能依赖右键、悬停、长按或键盘。
- 需要选择目标点时，优先缩放地图，而不是让弹出表单占据大半屏幕。

## 2. MVP 功能清单

### 2.1 P0 必须具备

| 功能 | 最小实现 | 验收重点 |
|---|---|---|
| 房间接入 | 房间码 + 房间口令换取会话令牌；Agent 使用设备令牌；任意成员通过网页查看和编辑 | 连接后得到同一套标绘数据；WebSocket 层强制鉴权 |
| 陆战 / 空战 / 双图 | 顶栏三态切换；两图画面独立 | 任何设备随时切换，不丢状态 |
| 地图尺寸与平移 | 视口正方形；缩放、平移、重置视图 | 不拉伸、不产生页面滚动 |
| 全量采集上传 | Agent 轮询全部端点，上传量化观测帧（10Hz）、原始批次（1Hz）、地图图片（按 generation） | 服务器可按时间回放任一 Agent 的原始响应 |
| 友军态势 | 每名 Agent 的本人位置 / 朝向与显式本人状态；其它 Agent 看到的友军对象融合为匿名友军 | 8 人同时显示，朝向准确；不重复显示同一友军；本人缺失时灰显 |
| 敌情策略开关 | 房间级 `enemyPolicy`（默认 off）+ 模式上限；顶栏徽标显示当前策略 | 切换后 1 个快照周期内生效；off 时网页收不到任何自动敌情 |
| 选择工具 | 点中现有标绘，重命名、移动或删除 | 无常驻“所有标记”列表 |
| 基础点标 | APP-6 点军标（集合、进攻、防守、观察、敌军、危险、空袭、侦察等映射为军标） | 标记出现在两图正确的地理位置 |
| 攻击来源 | 先点受袭位置，再点疑似来源，生成方向线 / 箭头 | 表达“方向估计”而非自动定位 |
| 同步 | 标绘事件立即广播；态势快照目标 10Hz | 断线重连不会丢已提交标绘 |
| 地图来源管理 | 两套资源按战局缓存；dHash 地图识别 + 战局自动识别 + 不一致时人工确认（BATTLE-01） | 不能误把其他战局或“陆战中空战视图”当成本局底图 |

### 2.2 P1 建议在首轮联机测试前完成

| 功能 | 最小实现 | 说明 |
|---|---|---|
| 进攻箭头 | 多节点折线骨架 + 箭体宽度 + 箭头（TacOps “Attack Arrow / MainAxisOfAdvance”），最后一次点击放置箭头 | 首轮只用折线骨架；平滑曲线随钢笔工具放到 P5 |
| 阶段线 / 边界 | 带标签折线（“PL 红”），虚线 / 实线 | 对应 TacOps PhaseLine / Boundary |
| 区域工具 | 多边形与矩形（AO、OBJ、空袭区、侦察区、警戒区）+ 圆 / 射程圈（半径以米输入） | 与军标共用类别字典 |
| 测距尺 | 多航点折线，显示分段 / 总距离、方位、按速度估算 ETA；可选择共享为标绘 | 本地工具默认不广播 |
| 现状 / 计划 | 每个标绘带 `status`：present（实线）/ planned（虚线），APP-6 惯例 | 区分“已发生”与“计划” |
| 时间标签 | 可选时间标签：对局时间 T+mm:ss 或北京时间 HH:mm；全局开关显示 / 隐藏 | 对应 TacOps DTG 标签 |
| 标绘属性 | 简短名称 / 注释 / 军标 SIDC / 颜色 / 可见范围（空、陆、双）/ 图层 | 仅在选中或绘制后显示紧凑编辑区 |
| 编辑并发 | 服务器定序确认 + 后写覆盖 + 撤销 / 撤销删除；保留 operationId 与版本号 | MVP 不做逐对象版本冲突提示 UI |
| 简单诊断 | 顶栏显示连接 / 数据是否过期 / 敌情策略；不做复杂状态面板 | 诊断详情只在设置或开发页面显示 |

### 2.3 P2–P3 与首次实战测试之后

| 阶段 | 功能 | 说明 |
|---|---|---|
| P2–P3 | 图层：新增 / 重命名 / 隐藏 / 锁定 / 排序，每局默认“本局”“通用”两层 | 对应 TacOps Outliner；锁定只防误触，任何人可解锁 |
| P2–P3 | 简报 / 演示：隐藏工具栏的只读视图；“开始演示”广播视口，其他人跟随、可“恢复跟随” | 演示权任何人可接管，符合无角色差异 |
| P2–P3 | 文字标注、导出 PNG | 文字作为独立标绘 |
| P5 / 下一轮 | 钢笔曲线（三次 Bézier）与平滑进攻箭头；完整军标编辑器（梯队、修饰符）；逐对象冲突提示；回放 UI；可选 Yjs 协同 | 数据格式已预留 |

### 2.4 本期明确排除

- 友军死亡自动记录、死亡证据状态机、战损报告、载具损伤部件与历史时间轴（原始 hudmsg 已存储，不做解析产品化）。
- 屏幕画面 / 雷达 / RWR 识别（Neo 原生采集管线、Link18 OCR 均不引入），以及任何内存读取、注入、hook。
- 指挥 / 成员权限分级、编队审批、复杂任务派发流程。
- 游戏内透明 Overlay、鼠标 Raw Input 双读。
- 单独的移动 App、实时语音 / VoIP、回放与复盘界面、3D 地图。

## 3. UI / UX 设计规范（以 V10 为基准）

### 3.1 结构

**桌面 / 横屏单图：**

```diagram
┌──────────────────────────────┐
│ WT Tactical [陆战] [空战] [双地图]  [指挥/成员]            │ ← 唯一顶栏
├─────────────────┬────────────┤
│                                  │ 选择 标点 路线         │
│                                  │ 区域 来源              │
│          正方形战术地图          │                        │
│      占满可用高度，保持 1:1      │ 当前工具的少量控件     │
│                                  │ 选中后才有名称/删除    │
│                                  │                        │
└─────────────────┴────────────┘
```

**桌面 / 横屏双图：**

```diagram
┌─────────────────────────────┐
│ WT Tactical [陆战] [空战] [双地图]  [指挥/成员]          │
├──────────────┬──────────────┤
│                            │                            │
│       正方形陆战地图       │       正方形空战地图       │
│                            │                            │
└──────────────┴──────────────┘
```

**手机竖屏：** 单图 = 上方大正方形地图 + 下方按可用空间压缩的工具；双图 = 两张正方形地图上下排，整屏同时可见，禁止滚动。极窄 / 低矮屏幕允许缩小地图和工具文字，但不得扭曲地图。

### 3.2 强制布局算法

令 W、H 为浏览器实际可用的工作区宽高（已扣除顶栏 T、安全区和容器间距）。令地图边长为 S：

- **单图桌面：**`S = min(H - T - padding, W - sideWidth - gap - padding)`。
- **双图横排：**`S = min(H - T - padding, (W - gap - padding) / 2)`。
- **双图竖排：**`S = min(W - padding, (H - T - gap - padding) / 2)`。
- **单图竖排：**先给紧凑工具区分配必要高度 toolsH，`S = min(W - padding, H - T - toolsH - gap - padding)`。

实现建议：`height: 100dvh; overflow: hidden` 与 ResizeObserver / visualViewport 自适应；地图容器始终 `aspect-ratio: 1 / 1`。地图背景图片以等比变换绘制，不能通过 CSS 分别拉伸宽、高。

<div class="note" markdown="1">
极端小屏存在天然的空间取舍：当两张正方形地图和顶栏无法同时达到舒适点击尺寸时，优先满足无滚动 + 正方形，缩小标记密度、隐藏非必要文字，并提供单图切换放大。
</div>

### 3.3 视觉层级与军标

- 地图是视觉主体：浅色低饱和地形图、清晰网格、克制的灰绿 / 米灰 UI。
- **符号体系采用 APP-6（milsymbol 渲染）：**友军蓝色矩形框、敌军红色菱形框、中立绿色方框、未知黄色四叶框；内部图标区分装甲、防空、飞机（战斗机 / 攻击机 / 轰炸机）、直升机、目标、重生点。8111 的 icon 映射到 SIDC（如 MediumTank → 装甲、SPAA/SAM → 防空、Fighter → 固定翼战斗机）。
- **自动与人工区分：**人工标绘用实线框；服务器融合的自动敌情用 APP-6 “计划 / 推定”虚线框 + 数据年龄（如“6s”），消失后按策略淡出；两者不可混淆。
- 颜色只表示含义：蓝色友军、红色敌情 / 疑似来源、琥珀色命令和进攻路径、青色侦察；阶段线默认黄色虚线。不要把整个界面染成高饱和色。
- 可直接使用 8111 提供的地图图片作为底图；需要时也可加载 8111 内置页面使用的图标字体作为备选符号。
- 禁止常驻状态卡、图例大面板、列表墙、说明段落；工具栏入口固定且简短。
- 地图边缘仅可显示必要的缩放 / 居中按钮，优先采用不遮挡标绘的角落；符号在缩放时保持恒定视觉尺寸（复用 Neo 的独立 overlay 渲染做法）。
- 单图交互区在未选工具时只显示工具按钮；选择对象后出现仅与该对象有关的简短编辑控件。

### 3.4 指挥 / 成员 UI

两个模式共享：同一房间、同一 WebSocket 协议、同一可执行操作集、同一读写权限。

- **指挥界面：**默认展开工具名称、军标库分类（任务 / 机动 / 区域 / 火力）、图形编辑选项，适合连续绘图。
- **成员界面：**默认折叠为图标工具 / 常用军标收藏，适合快速查看和快速点标。
- 用户随时切换，切换时视口位置、编辑历史、已绘标记保持不变。
- 服务端不得根据“UI 模式”拦截编辑权限；只校验是否为该房间有效连接及输入是否合法。敌情策略、图层锁定、演示接管同样对所有成员开放，但变更会广播并记录操作者。

### 3.5 标绘操作细节

| 工具 | 交互 | 数据形状 |
|---|---|---|
| 军标点 | 从收藏 / 军标库选择 → 点击地图 → 可选名称 / 状态 / 时间 → 提交 | Point + SIDC |
| 进攻箭头 | 逐点点击骨架，最后一次点击放置箭头；可调箭体宽度；完成 / 撤销末点 / 取消 | LineString + width + arrowhead |
| 阶段线 / 边界 | 逐点点击；输入标签（如“PL 红”）；实线 / 虚线 | LineString + label |
| 区域 | 多边形逐点点击或矩形拖拽；圆以中心 + 半径（米）创建 | Polygon / Rect / Circle |
| 测距尺 | 逐点点击航点；实时显示分段距离、方位、总距离与 ETA；可“共享为标绘” | LineString（本地或共享） |
| 攻击来源 | 第一次点击受击点，第二次点击来源方向；可改为扇形估计 | 原点 + 方位角 + 可选角宽 |
| 文字 | 点击放置，输入短文本 | Point + text |
| 钢笔曲线（P5） | 点击创建拐点；拖动创建平滑点；节点 / 手柄编辑 | 三次 Bézier 控制点（预留格式） |
| 选择 | 点击标记，显示可编辑属性、图层、状态和删除按钮 | 标记 ID + 服务器版本号 |

绘制工具统一使用撤销、取消和提交动作；桌面增加快捷键（参考 TacOps：L 线、Enter 在箭头处完成、Esc 取消），手机提供可点击操作条。路线建议保留“自由平移”模式，防止手机绘制时意外拉动地图。

### 3.6 tacops.gg 交互模型对照与改造

研究方式：浏览公开站点 tacops.gg 与无需账号的 app.tacops.gg 访客编辑器（Everon 地图），并检查其公开前端资源。TacOps 基于 GeoJSON 要素、milsymbol 军标、Yjs + WebSocket 的实时协作。

| TacOps 能力 | 观察到的行为 | 本项目改造 |
|---|---|---|
| 左侧工具栏 | 选择、删除、图片、线（Lines，快捷键 L，带子菜单）、多边形（带子菜单）、文字 TXT、测距尺、颜色、撤销 / 重做 / 清空 | 精简为：选择 / 军标 / 箭头 / 线 / 区域 / 测距 / 来源；手机为底部紧凑条 |
| 军标库 | 右侧可搜索库，分类 Favs / Land / Air / Sea / Tasks / Maneuver / C2 / Fires / Areas / Obstacles 等，数百个 APP-6 符号与战术图形；符号编辑器设置阵营、梯队、修饰符 | 预置约 30 个与战雷相关的符号 + 战术任务图形；收藏常用；阵营一键切换；梯队 / 修饰符留到 P5 |
| 战术图形 | Attack Arrow、MainAxisOfAdvance、AviationAxisOfAdvance、PhaseLine、Boundary、LineOfDeparture、LimitOfAdvance、Objective、SupportByFire、AttackByFire、ObstacleBelt、Counterattack 等 | 首轮：进攻箭头、阶段线、出发线、目标区、火力支援位置；其余按实战需要补充 |
| 状态 | 符号属性 Present / Planned / Fully Capable 等 | 只保留 present / planned 两态，实线 / 虚线 |
| 时间标签 | 要素可带 DTG（W 修饰符，格式 DDHHMMZMONYY），全局“显示 / 隐藏 DTG” | 改为对局时间 T+mm:ss 或北京时间短格式；全局开关 |
| 测距尺 | 多航点路线，分段 / 总距离、航点命名、高程剖面 | 分段 / 总距离 + 方位 + ETA；无高程 |
| 图层 / Outliner | 按类型或图层组织，锁定 / 隐藏 / 重命名 / 排序 / 分组 | P2 引入两层默认图层 + 锁定 / 隐藏 |
| 简报模式 | 隐藏工具栏的只读视图；演示者“开始 / 结束 / 接管演示”，观看者“恢复跟随” | 简报 = 只读布局；演示视口经服务器广播，任何人可接管 |
| 分享与协作 | 一条链接只读分享；Pro 可编辑链接与实时协作（编辑席位） | 房间链接 + 口令即可编辑，无账号、无席位、无付费分级 |
| 导出 | 导出 PNG | P3 导出当前视图 PNG |

经典 TacOps 兵棋（背景）：先下达命令、再同时执行的回合模型，单位按航路点移动，火力依托预先登记的目标参考点（TRP）。本项目只借鉴“航路点 + 预设参考点”的表达习惯（测距尺航点、目标区标注），不引入回合制。

## 4. 系统总体架构

```diagram
                 参战队员 PC（最多 8 个 Agent）
           War Thunder → 127.0.0.1:8111（全部端点）
                               │
                               ▼
            ┌──────────────────┐
            │ Rust Agent（由 Neo 本地代理改造）  │
            │   全量轮询 10Hz / 1Hz，本人识别    │
            │  量化观测帧 / 原始批次 / 地图图片  │
            └──────────────────┘
                               │ WSS（Protobuf）+ HTTPS PUT（域名）
                               ▼
    ┌──────────────────────────┐
    │ 香港 VPS（单实例，基于 Neo cloud-tactical-server） │
    │            Rust + Tokio + Axum + prost             │
    │             Room / 设备令牌 / 时钟同步             │
    │            Ingest -> 世界坐标 -> Fusion            │
    │            敌情显示策略（enemyPolicy）             │
    │         Annotation Service（定序 + 撤销）          │
    │             Map Asset Store（按哈希）              │
    │          Raw Store（zstd 分段，按期清理）          │
    │                    SQLite (WAL)                    │
    └──────────────────────────┘
                               │
                 ┌──────┴──────┐
                 │                          │
      10Hz FusedSnapshot（经策略过滤）  即时标绘事件
                 │                          │
                 └──────┬──────┘
                               ▼
           手机 / PC 响应式网页（React + milsymbol）
        ┌──────────────────────┐
        │         陆战单图 | 空战单图 | 双图         │
        │  友军 / 自动敌情图层（按策略）/ 战术标绘   │
        └──────────────────────┘
```

**技术选型：**

| 层 | 推荐方案 | 选择原因 |
|---|---|---|
| Agent | Rust + Tokio + Reqwest + tokio-tungstenite + prost；由 Neo `src-tauri/src/server.rs` 改造 | Neo 已实现 127.0.0.1:8111 代理与静态页托管；改为无界面常驻 + 上传器 |
| Server | Rust + Axum 0.8 + Tokio + prost；fork Neo `cloud-tactical-server` | 房间、relay / viewer WS、地图图片接口、配置、Docker 已具备 |
| Database | SQLite + SQLx，WAL | 房间、战局、标绘、事件、地图索引 |
| 原始数据 | 按 Agent / 战局分段的 zstd 压缩 Protobuf 文件 | 顺序追加写，按期清理，不进 SQLite |
| Web | TypeScript + React + Vite（与 Neo 同栈）+ Canvas/SVG + milsymbol | 高频单位用 Canvas，标绘与手柄用 SVG；APP-6 由 milsymbol 生成 |
| 协议 | Protobuf（WS 二进制）；prost 生成 Rust、protobuf-es 生成 TS；REST JSON | 与 Neo 一致；量化后体积小；schema 即文档；JSON 调试镜像便于排查 |
| 图片资源 | VPS 本地磁盘按 SHA-256 存储 + HTTP 缓存 | 双图与“陆战中空战视图”都保留 |
| 静态部署 | 同一 VPS 上的 Caddy + 域名 | 静态站和反向代理共用一个公网入口 |
| 管理 | systemd（或 Neo 自带 Dockerfile）+ 日志轮转 + 每日备份 + Raw 清理定时任务 | 无需 Kubernetes / Redis / 消息队列 |

**Protobuf 与 JSON 的取舍：**Neo 已证明 Protobuf + u16 量化在本场景可行，且 Agent 与服务器两端都用 Rust、网页用生成的 TS 类型，类型一致性比 JSON 更好；JSON 只用于 REST 管理接口、调试镜像（`/api/rooms/{id}/debug/snapshot.json`）和导出。Neo 前端手写的 Protobuf 编解码（`cloudTactical.ts`）不复用，改用 protobuf-es 代码生成，避免 schema 漂移。

### 4.1 WT-8111-Neo 复用决策

研究范围：main 分支、`codex/cloud-tactical-server`、`codex/cloud-gui-adaptation`（PR #3）、`codex/gui-split-demo-clean`（PR #2，陆战 / 空战工作台切换与 WinUI 启动修复）、`codex/quick-fire-mission-map-controls`（PR #1，火力任务与对象过滤）。

| Neo 组件 | 位置 | 决定 | 说明 |
|---|---|---|---|
| Rust 本地代理 | `src-tauri/src/server.rs`（Axum，0.0.0.0:17711，`/api/wt/*` → 127.0.0.1:8111，透传 Content-Type，`no-store`，rust-embed 静态页） | **复用为 Agent 底座** | 改为默认只绑定 127.0.0.1；保留本机状态页；新增轮询器与上传器（把 Neo 浏览器内 relay 下沉到 Rust，这也是 Neo 文档列出的下一步） |
| 8111 TS 类型与函数 | `src/lib/wt8111.ts`（WTMapInfo、WTMapObject、findPlayer、objectToWorld、distanceBetween、bearingBetween） | **直接复用** | 方位改用世界坐标差值；类型扩展为数字 / 字符串兼容 |
| 8111 文档 | `WT_8111_API_REFERENCE(.zh-CN).md` | **并入附录 A** | 与 WTM、LVM 合并并标注冲突 |
| 云端服务器 | `cloud-tactical-server/`（Axum + prost，rooms.rs、relay.rs、viewer.rs、fusion.rs、server.rs、config.rs，Dockerfile、compose、server.example.yaml） | **fork 为服务器底座** | 保留房间 REST、WS 路由结构、配置、Docker；重写鉴权、融合、快照，新增标绘、存储、策略 |
| Protobuf 信封 | `protos/envelope.proto`、`observation.proto`、`snapshot.proto` | **复用设计并扩展** | 保留 WsEnvelope oneof、Join / Ping / Pong / Ack / RequestKeyframe；新增原始批次、标绘、策略、演示等消息（6.2） |
| 坐标量化 | x/y → u16（0..65535），heading → deg×100 | **复用** | 精度见附录 A §5.4 |
| keyframe / delta | `removed_local_ids`、`RequestKeyframe`、`Ack` | **复用设计、补全实现** | Neo 服务器尚未实现 delta 与 keyframe 请求；MVP 先每帧全量，P3 视带宽启用 delta |
| 时钟同步 | NTP 式 Ping/Pong，5 样本中值，±30s / 60s 时间戳钳制 | **直接复用** | 用于观测时间对齐与数据年龄 |
| 房间与地图图片 | `POST /api/rooms`、`/join`、`PUT/GET /api/rooms/{id}/map-image`（`X-WT8111-Map-Generation` 头） | **复用接口形态** | 改为按图片哈希存多张、按提供者与战局索引，而非“每房间一张” |
| 融合引擎 | `fusion.rs`（指纹匹配、3s 航迹、15s 空中目标椭圆 ROI） | **参考重写** | 见 4.2 缺陷；ROI 仅在 `enemyPolicy=fused` 时启用 |
| Web 地图工作台 | `src/App.tsx`（缩放 / 平移、恒定尺寸符号 overlay、对象过滤、NATO 风格符号、测距） | **部分复用** | 交互代码可借鉴；UI 以 V10 为准；符号换成 milsymbol |
| WinUI 3 启动器 | `src-winui/`（C# / Windows App SDK，托管 wt-8111-neo.exe、房间配置、连接检查） | **MVP 不复用** | 增加 .NET 打包负担；Agent 先用托盘 / 控制台 + 本机状态页，必要时下一轮再评估 |
| 云端 WebGUI | `webgui/index.html`、`map.html` | **仅参考** | 房间管理页思路可借鉴，界面以 V10 为准 |
| 原生采集 / 雷达识别 | `NATIVE_CAPTURE_PIPELINE.zh-CN.md` | **不引入** | Neo 自身已暂停该方向 |

### 4.2 Neo 原型中必须修正的问题（代码审查结论）

| # | 问题 | 位置 | 影响 | 修正 |
|---|---|---|---|---|
| 1 | WebSocket 不校验房间口令：`/join` 只是 REST 检查，`ws_relay` / `ws_viewer` 直接升级，`JoinRequest.password` 未被验证 | server.rs、relay.rs | 知道房间名即可上传 / 观看 | WS 升级前校验短期会话令牌；Agent 用设备令牌 |
| 2 | 口令用 `DefaultHasher` + 固定盐 | rooms.rs | 不安全 | argon2id |
| 3 | 本人位置（`ObservationFrame.player`）被解析后丢弃，快照中没有任何“哪个队员在哪里” | fusion.rs | 友军无身份 | 本人轨迹作为带身份的友军单独下发 |
| 4 | `label_id` 每帧重建的局部字典，跨客户端无意义，却用于跨客户端匹配 | cloudTactical.ts、fusion.rs | 融合错误 | 会话级字典或直接传 icon 枚举 |
| 5 | `local_id` 由含坐标的签名生成，目标一动就换 ID | cloudTactical.ts | 无法稳定跟踪 | Agent 端按最近邻关联分配稳定 local_id |
| 6 | 跨客户端直接比较 u16 归一化坐标，不区分陆战图、空战图、“陆战中空战视图” | fusion.rs | 不同视图的对象被错误合并或拆分 | 先按各自 map_min/max 换算世界坐标再融合 |
| 7 | 速度用服务器接收时刻 `Instant` 计算，未用已同步的 `observed_at_ms` | fusion.rs | 网络抖动放大速度误差 | 统一用观测时间 |
| 8 | `source_count` 恒为 1；`map_generation` 跨客户端直接比较 | fusion.rs、rooms.rs | 指标失真；底图被错误替换 | 计数来源；按提供者 + 哈希管理底图 |
| 9 | ROI 椭圆 heading 以“度”写入，客户端按“度×100”解析 | fusion.rs、cloudTactical.ts | ROI 旋转错误 | 统一 deg×100 |
| 10 | schema 漂移：客户端写 field 13 `measurement_age_ms`、读 `total_age_ms` / `contributing_clients`，proto 文件与服务器均无 | cloudTactical.ts、protos | 静默丢字段 | 只用代码生成；CI 校验 proto 兼容 |
| 11 | relay 运行在浏览器页面中（500ms 上传），关闭页面即停止 | useCloudTacticalRelay.ts | 不适合比赛 | 下沉到 Rust Agent |
| 12 | 阵营判定把绿色判为中立；Player 之外的红色判定依赖字符串包含 | App.tsx、cloudTactical.ts | 小队友军被判中立 | 采用附录 A §4.5 规则，服务器端判定 |
| 13 | 聊天 / HUD 游标从 0 开始，首次轮询拿到整段历史 | useWT8111.ts | 重复数据 | Agent 记录并持久化游标，检测回退 |


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
 客户端： https://<域名>/
          wss://<域名>/ws/agent   wss://<域名>/ws/web
            │
          Caddy（域名证书自动签发 / 续期）
            │
        Rust / Axum
          ├── 静态前端
          ├── /api/rooms/*      房间 / 加入 / 策略 / 地图图片
          ├── /ws/agent         Agent 上行（Protobuf）
          ├── /ws/web           网页下行 + 标绘（Protobuf）
          ├── /assets/maps/*    按哈希缓存的底图
          └── SQLite + Raw Store（/var/lib/wttc/raw）
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
wt-tactical-command/
├── apps/
│   ├── web/              # React + TS；V10 全屏 UI + milsymbol
│   ├── agent/            # Rust；由 Neo src-tauri 代理改造
│   └── server/           # Rust / Axum；fork Neo cloud-tactical-server
├── crates/
│   ├── protocol/         # protos/*.proto + prost 生成 + 校验
│   ├── wt8111/           # 8111 解析器（数字 / 字符串兼容）
│   ├── fusion/           # 世界坐标融合 / 敌情策略
│   └── map-core/         # 陆空变换 / 地图哈希 / 几何
├── packages/
│   ├── proto-ts/         # protobuf-es 生成的 TS 类型
│   └── symbols/          # milsymbol 封装 + 战术图形
├── upstream/
│   └── wt-8111-neo/      # 上游镜像（git subtree）
├── fixtures/
│   ├── 8111-ground/      # 实测样本（含原始批次回放）
│   ├── 8111-air/         # 含 WTM JH-7 夹具
│   └── map-hashes/       # 引入 WTP 哈希表
├── infra/                 # caddy / systemd / backup / raw 清理
├── docs/
│   ├── 8111_API_SPEC.md  # 本计划附录 A
│   ├── adr/              # 关键架构决策
│   ├── install/          # Agent 图文安装说明
│   └── qa/               # 测试记录
└── README.md
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

**版本：** 1.0（随项目计划 v1.2 发布）　**日期：** 2026-10-10　**适用：** War Thunder Tactical Command 的 Windows Agent、服务器解析器与测试夹具

本规范把多个开源项目的实测记录、代码实现和文档合并为一份可执行的接口说明。8111 没有官方版本化契约，下列内容全部按“开放 schema”处理：未知字段要保留，已知字段可能缺失。

### A.1 来源与可信度标记

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

文中 **【冲突】** 表示不同来源相互矛盾，**【待测】** 表示必须在真实对局中验证。

### A.2 服务概述

- 游戏运行时在 `8111` 端口提供 HTTP 服务；没有健康检查端点，只能通过请求是否成功、`valid` 字段和 `mission.status` 推断状态〔WTM〕。
- 所有已知端点均为 `GET`。响应头包含 `Access-Control-Allow-Origin: *`、`Access-Control-Allow-Methods: GET, POST, OPTIONS`；`OPTIONS /state` 返回 200 空体；未发现任何有意义的 POST 操作〔Neo〕〔WTM〕。
- **远程访问（0.0.0.0）：**游戏设置“战斗设置 → 允许远程访问（Allow remote access）/ 端口 8111”〔WTD〕〔WTM〕。WTM 在 2026-09-01 实测中观察到服务监听 `0.0.0.0:8111`，从局域网用游戏 PC 的以太网地址访问成功；关闭该选项时是否只绑定回环尚未验证【待测】。因此不能假设 8111 只对本机可见。
- 本项目 Agent 始终访问 `http://127.0.0.1:8111`，不依赖远程访问开关；同时在安装说明中提醒：开启远程访问时同一局域网的任何设备都能读取原始数据，严禁路由器端口映射。

### A.3 端点总表

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

### A.4 各端点详细说明

#### A.4.1 `/`（内置网页）

- 创建 640×640 正方形 canvas，把 `/map.img` 拉伸铺满；网格线和字母数字标签由 `map_min`、`map_max`、`grid_steps` 生成，**不使用 `grid_zero`**〔WTM〕。
- 每 500ms 请求 mission、map_obj、map_info、chat、hudmsg、indicators、state，每 25ms 重绘一次缓存地图〔WTM〕。即内置页面本身只有 **2Hz 数据刷新**。
- 本人为带深色描边的白色方向箭头（按 `atan2(dx,-dy)` 旋转）；其它对象颜色直接取自 `map_obj.json`；符号来自自定义字体 `icons.ttf`（588,472 字节，返回 `text/plain` 且无 CORS 头）〔WTM〕〔Neo〕。

#### A.4.2 `/indicators`

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

#### A.4.3 `/state`

- 字段名内嵌单位：`"H, m"`、`"TAS, km/h"`、`"IAS, km/h"`、`"M"`、`"AoA, deg"`、`"AoS, deg"`、`"Ny"`、`"Vy, m/s"`、`"Wx, deg/s"`、`"Mfuel, kg"`、`"Mfuel0, kg"`、`"throttle 1, %"`、`"RPM 1"`、`"manifold pressure 1, atm"`、`"oil temp 1, C"` 等，发动机编号从 1 开始、按实际存在发现〔WTM〕〔Neo〕。
- **陆战通常只返回 `{"valid": false}`**（T-80BVM、ZTZ-96A 均如此），陆战状态应使用 `/indicators` 与地图〔Neo〕〔WTM〕。`valid:false` 不等于“在机库”。
- 空战 JH-7 样本为 613 字节；同一份数据中 `power N, hp` 为 0 但推力非零——数值字段存在不代表适用〔WTM〕。

#### A.4.4 `/map_info.json`

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

#### A.4.5 `/map_obj.json`

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

**可见性：**据报道游戏只输出本机战术地图当前显示的对象（隐藏对象被省略，而不是带隐藏标志）【待测】〔WTM〕〔Neo〕。WTM 在 Air SB 实测中确认**远端队友飞机无法从 map_obj 获得**，只能从聊天中的网格 + 高度文本获得；普通“注意地图”标记**不产生** `point_of_interest` 对象，只有本人对象闪烁〔WTM〕。陆战中友军在地图上可见，敌军仅在被发现 / 标记时出现（Neo 陆战样本含 7 辆红色 MediumTank）。

#### A.4.6 `/map.img` 与 `/map.img?gen=<n>`

- 返回 `image/png` 或 `image/jpeg`，扩展名不代表编码，代理必须透传 `Content-Type`〔Neo〕〔WTM〕。
- 在同一 `map_generation` 内静态不变；`/map.img` 与 `/map.img?gen=1` 字节相同；无缓存头〔WTM〕。应在新 generation 下载成功后原子替换，临时失败期间保留旧图。
- **尺寸不固定：**Air Test Flight（JH-7）返回的是 **128×128、1,519 字节的灰色问号占位图**（91.94% `#404040`），不是地形图〔WTM〕；WTP 的哈希表把这张图标为 `no_map`。真实空战 / 陆战底图的尺寸需实测【待测】；Neo 云端接口文档示例为 323,235 字节 JPEG。
- **陆战中切换为飞机：**WTP 的哈希表区分 `*_tankmap`（陆战视图）与 `avg_*_map`（“陆战中的空战视图”，52 张），说明陆战玩家驾驶飞机时 8111 底图会整体换成更大范围的空战视图，**所有对象的归一化坐标也随之换算到新视图**。WTP 在切换的相邻两帧中按 icon 匹配至少 3 个地面对象，拟合 `x_air = a·x_ground + b`、`y_air = c·y_ground + d`（缩放上限 0.5，残差上限 0.01）〔WTP〕。核弹事件会把陆战图切到同名纯空战图。
- 地图识别：WTP 用 16×15 灰度差分哈希（dHash，225 bit，57 位十六进制），以汉明距离 ≤42 模糊匹配 229 条已知地图哈希（空战 18、陆战、陆战中空战视图 52、海战等）〔WTP〕。

#### A.4.7 `/mission.json`

```code
{ "status": "running", "objectives": [ { "primary": true, "status": "in_progress", "text": "Take off" } ] }
{ "status": "running", "objectives": null }
```

- `objectives` 可为 `null`；`status` 观察到 `running`，失败值 **【冲突】**`fail`〔LVM〕与 `failed`（Neo 预期）并存，未知字符串必须原样保留。目标 `status`：`in_progress`、`completed`、`failed`。

#### A.4.8 `/gamechat?lastId=<id>`

```code
[ { "id": 123, "time": 456.7, "mode": "Team", "sender": "PlayerName", "msg": "...", "enemy": false } ]
```

- 返回 `id > lastId` 的记录；`time` 可缺失。**ID 只在游戏启动时归零，下一局延续上一局编号**〔LVM〕〔WTM〕。轮询器必须识别 ID 回退（游戏重启）。首次连接用 `lastId=0` 会拿到整段历史。
- Air SB 中“Guide on me / Follow me / Cover me”等位置呼叫在远端队员的聊天文本中带 `[C5, alt. 600 m]` 网格与高度；“Attention to the map”只带目标网格 `[B4]`；发送者本人看不到坐标后缀〔WTM〕。
- 消息与玩家名是不可信文本，只能按文本渲染。

#### A.4.9 `/hudmsg?lastEvt=<id>&lastDmg=<id>`

```code
{ "events": [], "damage": [ { "id": 161, "msg": "A (Spitfire) set afire B (Spitfire)",
  "sender": "", "enemy": false, "mode": "" } ] }
```

- `events` 在当前版本中通常为空，`damage` 含击毁 / 起火 / 坠毁文本；ID 语义同 gamechat〔LVM〕〔WTM〕。客户端以各自数组最后一条的 id 作为下次游标〔Neo〕。

#### A.4.10 `/loc/map/*_objectives?fmt=js`

- 返回可执行的 JavaScript 赋值语句（非 JSON），英文值为 `"PRIMARY GOALS"` / `"SECONDARY GOALS"`〔WTM〕。Agent 原样作为文本上传。

### A.5 坐标、距离与朝向

#### A.5.1 换算公式

```code
pixel_x = image_width  * x            pixel_y = image_height * y
world_x = map_min_x + x * (map_max_x - map_min_x)
world_y = map_min_y + y * (map_max_y - map_min_y)
x = (world_x - map_min_x) / (map_max_x - map_min_x)    // 逆变换
```

- y 轴向下（图像坐标）；正北为图像上方。没有经纬度映射，范围是游戏世界单位（通常按米理解）〔WTM〕〔Neo〕。
- 底图必须保持正方形几何，拉伸会扭曲距离和方位〔WTM〕。

#### A.5.2 距离与方位

```code
dx_world = (tx - sx) * (map_max_x - map_min_x)
dy_world = (ty - sy) * (map_max_y - map_min_y)
distance = sqrt(dx_world² + dy_world²)
bearing  = (atan2(dx_world, -dy_world) * 180/π + 360) mod 360     // 0 = 北，顺时针
```

- Neo 的 `bearingBetween()` 直接用归一化差值计算，只有在范围为正方形时才等价；本项目统一用世界坐标差值〔Neo 代码〕。

#### A.5.3 朝向

```code
heading = (atan2(obj.dx, -obj.dy) * 180/π + 360) mod 360
```

- **已实测验证：**100 个 JH-7 样本中与 `/indicators.compass` 的平均差 0.000408°、最大 0.001041°〔WTM〕。Neo、WP8F、L18 均采用同一公式；Neo 云端协议把它量化为 `heading × 100` 的整数。

#### A.5.4 量化精度（供协议设计）

坐标量化为 u16（0..65535）时：陆战 4096 单位地图分辨率约 0.06 单位，空战 65,536 单位地图约 1 单位，均远小于图标尺寸；朝向 `deg × 100` 精度 0.01°〔Neo 协议〕。

### A.6 本人识别与本人状态信号

- 本人 = `icon == "Player"` 的对象（所有来源一致）；颜色不可靠（黄 / 白 / 蓝闪烁）。
- 本人对象的 `type` 区分 `ground_model` 与 `aircraft`，可直接判定陆战玩家是否正在驾驶飞机；`/indicators.army` 作为交叉验证。
- 阵亡、重生 / 选车、观战、回机库期间的 map_obj / map_info / mission 变化没有文档化状态机〔WTM〕【待测】。WTP 以 `map_info.valid` 为对局有效信号，失效后 20 秒宽限期再判定对局结束〔WTP〕。

### A.7 陆战与空战差异汇总

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

### A.8 更新行为与轮询建议

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

### A.9 类型与解析怪癖

1. `map_info` 数值可能是数字字符串（`"3250.0"`、`"1"`）。
2. 缺失字段 ≠ 0；`-1` 是“不可用”哨兵值；`valid` 优先。
3. 响应体可能为空字符串（Neo `fetchJson` 将空体视为 null）。
4. `color` 大小写不一致；`color[]` 可能缺失，需从十六进制解析。
5. `icon` / `type` 不是封闭枚举；`airfield` 没有 `x/y`。
6. `map.img` 编码不固定；`icons.ttf` 以 `text/plain` 返回且无 CORS。
7. 聊天 / HUD ID 跨对局延续，仅游戏重启归零。
8. 指示器单位因载具而异（m/s 与 km/h、英制座舱）。

### A.10 未决问题（需真实对局验证）

1. 陆战 RB / AB / SB 与空战 AB / RB / SB 下 map_obj 的敌我可见范围是否与游戏内地图完全一致；Sim 模式是否省略友军。
2. 陆战玩家驾驶飞机时 `map_info` 的 `map_min/max`、`hud_type`、`map_generation` 是否变化，以及切回坦克时的行为。
3. 实战空战图和“陆战中空战视图”的 `map.img` 尺寸、编码与是否随 generation 变化。
4. `hud_type` 的完整取值与含义；`grid_zero` 到官方网格标签的换算。
5. 阵亡、重生、观战、回机库时的本人对象与 `valid` 变化顺序（DATA-04）。
6. 陆战下 map_obj 新样本率（静止 / 移动）与 8 人实战中的响应时间。
7. 玩家自定义颜色 / 色盲模式下的实际调色板；小队成员绿色在连队战中的取值。
8. `point_of_interest` 在哪些模式、哪些操作下出现。
9. `mission.status` 的完整集合（`fail` / `failed`、结束、回放）。
10. HUD `events` 是否会有内容；POST 是否有任何作用。
11. 关闭“允许远程访问”时 8111 是否仍监听 0.0.0.0。


### A.11 参考链接

见第 12 节（含全部仓库与论坛链接）。

</div>
