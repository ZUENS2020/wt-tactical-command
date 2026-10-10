# enemy/ — 敌方地图标记解析（Python 参考实现，供移植 Rust）

```
model.py     数据结构（MapInfo / MapObj / Frame / HudEvent / Contact / ThreatSummary）
classify.py  敌我判定 r>=200&&g<130&&b<130；海军 icon 排除；icon→class；Player/机场/轰炸点不进跟踪
geo.py       world = map_min + norm*(map_max-map_min)（y 向下=南）；compass = atan2(east, -south)
tracker.py   EnemyTracker：按 (type, icon) 分组、门限最近邻、全局贪心分配；lost/stale/reacquire/expire
reveal.py    reveal_kind 规则 R0..R9（全部为假设，见 ENEMY_MARKERS_20261010.md）
summary.py   8 扇区（N..NW）攻击方向直方图 + 最近威胁
stream.py    仅分析用：采集器记录 → 事件流；hudmsg 分类；session 类型判定
tests/       unittest（录制帧夹具 + 合成用例），运行：
             cd /workspace/wt-analysis && .venv/bin/python -m unittest discover -s enemy/tests -t .
```

## 用法
```python
tr = EnemyTracker(TrackerConfig(), session_kind="battle")
tr.set_map_info(MapInfo.from_json(map_info_json))       # 每次 map_info 到达都调用（临时网格问题）
tr.add_hud_event(HudEvent(t_ms, id, msg, involves_player, stream.hud_kind(msg)))
contacts = tr.update(Frame(t_ms, tuple(MapObj.from_json(o) for o in map_obj_json)))
summary  = threat_summary(contacts, t_ms)
```

## Rust 移植要点
- 所有时间显式传入（ms），无全局时钟；无随机；分配用“距离升序全局贪心”，结果确定。
- `Contact` → `struct Contact`，`reveal_kind` → `enum RevealKind { Scouted, HitRevealed, VisibleStatic, Unknown }`，
  `reveal_rule: &'static str + 参数`。
- 门限：`gate = gate_base_m + vmax(type) * (t - min(last_seen, static_since))`，不可见轨迹再加 `reacquire_extra_m`。
  用“位置最后一次变化时刻”而不是“最后一次观测时刻”，否则周期性跳变（侦察标记）会断轨。
- 颜色/blink 不参与关联键：同一目标会在 `#fa0C00/0` 与 `#f00C00/2` 之间切换。
- 默认参数：gate_base 25 m，地面 vmax 35 m/s，空中 450 m/s，lost 400 ms，reacquire 5 s，expire 60 s。
