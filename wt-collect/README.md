# wt-collect — real-game 8111 test data pipeline

```
Windows PC (shengyuan-zhang)                         nec-manager-box (100.94.249.78)
wt-collector.exe --127.0.0.1:8111 (game)            wt-receiver 0.0.0.0:18111 (tailnet only)
   raw samples -> gzip batches /250ms --Tailscale-->  data/<date>/<session>/samples-NNNN.jsonl.gz
   map.img by sha256 -------------------------------> data/<date>/<session>/images/<hash>.png|jpg
   local JSONL copy + spool (never lose data)         wt-analyze <session> -> <session>/report/*.md,*.csv
```

| path | what |
|---|---|
| `collector/` | wt-collector (Rust, tokio+reqwest, plain HTTP, no TLS). `cargo build --release -p wt-collector --target x86_64-pc-windows-gnu` |
| `receiver/` | wt-receiver (axum). Env in `receiver.env` (token, bind, data dir, size guard) |
| `analyzer/` | wt-analyze |
| `common/` | shared `Record` / `Batch` format + readers |
| `mock/mock8111.py` | SYNTHETIC 8111 mock (ground / ground→air / dead / respawn / air RB loop) |
| `scripts/receiverctl.sh` | `start|stop|restart|status|install-binary` (supervisor with auto-restart) |
| `scripts/e2e-mock.sh` | end-to-end test: mock → collector → receiver (with outage) → analyzer |
| `dist/wt-collector-win64.zip` | Windows package 0.2.0 (exe + toml with token + Chinese README + cli-run.bat); previous: `dist/wt-collector-win64-0.1.5.zip` |
| `bin/` | installed binaries used by the supervisor / analyzer |
| `test/e2e-sessions/` | mock test sessions (synthetic; moved out of data/) |

## Record format (one JSON line per request)
`session, seq, ep, url, wall_ms, mono_ns, lat_us, status, err?, ctype?, bytes, body? (raw, exact), body_b64?, img_hash?, changed, cfg_hz, epoch`.
Synthetic records: `#meta` (collector config), `#note` (labels). Receiver de-dups by (session, seq) via `seq.idx`.
API notes go to `notes.jsonl`.

## Receiver API
- `GET /` live status page (2 s refresh), `GET /api/status` JSON, `GET /healthz`
- `POST /api/ingest` (Bearer token, gzip JSON `Batch`), `POST /api/image?session=&hash=&ctype=` (raw bytes, sha256 verified)
- `POST /api/note` `{"text": "...", "session"?: "...", "player"?: "..."}` → latest session of that player if no session given
  `curl -H "Authorization: Bearer $WT_TOKEN" -H 'Content-Type: application/json' -d '{"text":"color-blind on"}' http://100.94.249.78:18111/api/note`
- Size guard: 507 when disk free < WT_MIN_FREE_GB (5) or data dir > WT_MAX_GB (30). Collector then spools locally.

## Analyzer
`bin/wt-analyze <session-id | id-prefix | dir | latest> [--out DIR]` → `report.md` + CSVs (endpoints, map_obj_motion,
objects, colors_by_note_segment, map_generations, transitions, frame_switches, player_intervals, ids, mission, segments, bandwidth).

## Naval filter (receiver, since 2026-10-10)
All ship/naval info is removed in the receiver ingest path before writing (receiver/src/naval.rs):
- /map_obj.json: objects with icon (case-insensitive exact) in WT_NAVAL_ICONS (default list in naval.rs) or
  type containing any of WT_NAVAL_TYPE_SUBSTR (ship,boat,naval) are dropped; kept objects are copied byte-for-byte;
  record gets `filtered_naval: N`, `bytes`/`changed` recomputed. Extend with WT_NAVAL_ICONS_EXTRA in receiver.env.
- /indicators, /state: when own vehicle is naval (indicators army ship/boat/naval or type shipModels/…ship…/boat…,
  or the Player map object is naval) content is removed: record kept as placeholder with `dropped: "naval"`.
- Unknown icons/types (not in the known list) are counted in data/unknown_icons.json (flushed every 10 s).
- Retro pass: `WT_DATA=… bin/wt-receiver filter-existing` (receiver stopped). Backup: backup-pre-naval-filter.tar.gz.

## 0.1.5: no local storage + clock guard
- Collector writes no files (no JSONL copy, no spool, no images); bounded in-memory buffer (10 s / 8 MB), drops oldest.
- Clock sync: GET /api/time (no auth) -> {server_ms, max_skew_ms}; bursts of 8 at start, 4 every 30 s, min-RTT sample,
  offset = server - (t0 + rtt/2). Records carry wall_ms + ts_server_ms; batch header `clock` (offset/rtt/local drops).
- Receiver guard (receiver/src/guard.rs): WT_MAX_SKEW_MS (default 3000): |recv - ts_server_ms| > max -> too_old / from_future;
  batches without valid sync -> no_sync (whole batch). WT_ACCEPT_LEGACY=auto|on|off (auto: accept pre-0.1.5 only).
  Counters per session in guard.json, /api/status `clock_guard`, status page, analyzer report. Stored records get recv_ms.
- Test: test/clock-test.sh <label> <faketime skew> <linux|wine> (outage + 3.6 s link delay via mock/delayproxy.py --delay-file).

## 0.2.2: uniform pings

- Pings are not distinguished by owner. Collector default buttons = original 9 + one `ping` (标点), replacing the 4 buttons of 0.2.1.
- Receiver `ev:"ping"` has one uniform shape: `id, t, t_end (= t + ttl_ms), ttl_ms (30 s display policy), grid, area, x, y,
  precision (grid | none), kind, text, source` + optional `meta {sender, own, sender_role, sender_track, sender_xy, sender_cell, ...}`
  that nothing branches on. `/api/status.allies.recent_pings` lists active pings without any owner fields; location calls and
  point_of_interest markers moved to `recent_calls`. Package `dist/wt-collector-win64.zip` (0.2.2), previous `dist/wt-collector-win64-0.2.1.zip`.

## 0.2.1: squadmates + map pings (标点)

- `common/src/mapobj.rs`: shared map_obj classifier. Side: own (`Player` #faC81E, or #043FFF blink 2), squad (#67D756 ground / #39D921 air),
  ally (#174DFF, fresh spawn #1839A7 / #134AFF), enemy (unchanged red rule), neutral. `Player` #174DFF = spectated teammate after death, never "own".
  Kind: unit vs capture_zone / respawn_base / airfield / bombing_point / defending_point / map_marker (point_of_interest; never a unit, never naval-filtered).
- `common/src/radio.rs`: gamechat quick-chat parser (ping_map / ping_grid / ping_marked / need_cover / need_support / air_target_request / thanks ...),
  grid `[c4]` / `[a14, 高度 1000 米]` extraction, grid label <-> normalized xy (`grid_zero + (n-1)*grid_steps`).
- `receiver/src/markers.rs`: friendly tracks + signal blinks (#043FFF/2) fused with chat -> `<session>/markers.jsonl`
  (`ev: squad | signal_start | signal_end | ping | location | radio | signal_only | map_marker`), `GET /api/markers?session=..`,
  `/api/status` -> `allies`. Offline: `wt-receiver markers <session_dir> [--write]`. Backlog of the first `lastId=0` gamechat poll is ignored.
- Collector 0.2.1: 4 new default buttons (i_pinged 我打了标点, team_pinged 队友标点, squad_pinged 小队队友标点, squad_near 小队成员在附近);
  snapshot adds `enemy_units, squad_xy, signal_units, self_signal, spectating`; self_xy no longer follows a spectated teammate.
  Package `dist/wt-collector-win64.zip` (0.2.1), previous `dist/wt-collector-win64-0.2.0.zip`.
- Findings: `analysis/SQUAD_PINGS_20261010.md`.

## 0.2.0: GUI + labels + hangar upload pause

- **GUI** (`collector/src/gui.rs`, egui/eframe 0.36 + glow/OpenGL, cross-compiled `x86_64-pc-windows-gnu`, only system DLLs).
  No arguments on Windows (double-click) = window; any argument / `run` / `diag` / `note` = console mode exactly as 0.1.5
  (`cli-run.bat` in the zip = `wt-collector.exe run`). `wt-collector gui` forces the window. Double-clicked: the console is
  freed (std handles nulled). Window position/size/always-on-top persisted in `%APPDATA%\wt-collector\gui.json`.
  Close button: `[gui] on_close = "ask"|"minimize"|"exit"` (ask = modal: minimize keeps collecting / exit). No tray icon.
  Chinese font loaded at runtime (msyh.ttc → Deng → simhei → simsun; `[gui] font` or env `WT_GUI_FONT="path#ttc_index"`).
  No global hotkeys (by request): labels only from button clicks (`source: "button"`).
- **Labels** (`collector/src/labels.rs`): data-driven `[[labels]] id/text/color/note` in wt-collector.toml (defaults = 9 buttons).
  Event JSON: `{type:"label", uid:"<session>#<seq>", label_id, text, note?, t_local, t_server_synced (null if no sync),
  clock_synced, clock_offset_ms, clock_rtt_ms, source, session, seq, mono_ns, epoch, battle_id?, snapshot{in_battle, map_gen,
  map_img_hash, hud_type, self_xy, self_dxdy, self_type, army, vehicle, objects, enemy_objects, map_obj_seq, map_obj_wall_ms}, version}`.
  Undo (10 s): `label_id "_undo", action "undo", ref_uid`. Tag: `"_tag", action "tag", ref_uid, tags[]`.
  Sent by a dedicated task to `POST /api/label` (JSON `{session, player, version, clock_synced_now, clock_offset_now_ms,
  sent_local_ms, labels[]}`), queued + retried (backoff ≤10 s) until confirmed; independent of clock guard and hangar pause.
  CLI test: `wt-collector label <id> [--note ..] [--session ..] [--tag test]`; `wt-collector labels` lists the buttons.
- **Receiver** (`receiver/src/labels.rs`): `/api/label` stores each label as a Record `ep:"#label"` (body = label JSON +
  `recv_ms, t_server, t_server_source (synced|offset_at_send|recv_estimate), delay_ms, late, player`) in the session's
  samples-*.jsonl.gz (dedup by seq, never dropped for skew; `late=true` instead) **and** one line in `<session>/labels.jsonl`.
  `GET /api/labels?session=<id|substring|latest>&date=YYYY-MM-DD[&test=1][&undone=1][&meta=1][&format=csv]` (Bearer token):
  resolves undo/tags; labels tagged `test` and undone labels are excluded by default.
  `curl -H "Authorization: Bearer $WT_TOKEN" 'https://wt.zuens2020.top/api/labels?date=2026-10-10&format=csv'`
  Status JSON per session: `labels`, `upload_state {paused, reason, age_s}`; status page shows `PAUSED (hangar) · collector alive`.
- **Label ↔ frames**: `scripts/labels_near.py <session_dir|data_root> [--session S] [--window 3] [--csv out.csv] [--frames]` prints
  each label with the enemy map_obj markers (r≥200,g<130,b<130) in ±3 s (simple NN tracks: type/icon, first/last dt,
  color/blink timeline, appears/gone in window, distance m) and hudmsg lines in the window. On the server:
  `sudo python3 labels_near.py /var/lib/wt-receiver/data --session <id>`.
- **Hangar upload pause** (`collector/src/gate.rs`, applied in writer_loop): pause when map_info valid=false AND map_obj `[]`
  (or map_info unknown + placeholder map.img sha256 `cfb99c04…`) for ≥2 s, or game API unreachable ≥10 s. While paused only
  synthetic records go out: one `#state {state:"paused", event:"entered_hangar"|"entered_game_down", vehicle, army, ...}`,
  `#heartbeat` every 30 s (suppressed counters, last error), `#state {event:"vehicle_changed"}`. Resume immediately when
  map_obj is non-empty / map_info valid / game back: `#state {state:"active", event:"left_hangar", paused_ms}` + latest record
  per endpoint (<2 s old). The placeholder image bytes are never uploaded (record keeps img_hash). Note: seq numbers have gaps
  during pauses (suppressed records); `#state`/`#heartbeat` explain them.
- Tests: `cargo test -p wt-collector` (label serialization/config parsing, undo window, gate), `cargo test -p wt-receiver`
  (label storage/late/unsynced, undo/tag resolve), `test/gate-label-test.sh` (mock with long hangar → local receiver),
  wine GUI smoke test + screenshots `shots/gui.png`, `shots/gui-hangar.png`.

### 中文使用说明（0.2.0）
- 双击 `wt-collector.exe` → 打开“WT 数据采集 · 标注”窗口，采集自动在后台开始。状态区显示：游戏接口（端口）、
  游戏状态（对局中 / 机库 · 已暂停上传）、上传（条/秒、队列、延迟）、时钟偏差、本次标签数 / 待发送。
- 按钮（一次点击 = 一个标签，点击后出现绿色确认，10 秒内可点“撤销”）：
  我侦察到敌人(i_scouted) · 队友侦察(team_scouted) · 我击中敌人(i_hit) · 我被击中(got_hit) · 我被侦察(i_spotted) ·
  敌机/敌人出现在地图上(enemy_icon) · 击毁(kill) · 阵亡(died) · 自定义备注(note，在输入框输入后回车)。
- 没有全局快捷键，只能鼠标点击；建议游戏用“无边框窗口”，本窗口默认置顶。
- 配置：wt-collector.toml 中 `[[labels]]`（id / text / color / note）可增删改按钮，改完重启即可；`[gui]` 有
  always_on_top、on_close（ask/minimize/exit）、columns、scale、font。`wt-collector.exe labels` 查看当前按钮。
- 机库里自动暂停上传（只发状态和每 30 秒心跳），进入对局立即恢复；标签任何时候都照常发送。
- 命令行模式：`cli-run.bat` 或 `wt-collector.exe run`（带任何参数启动也都是命令行模式），与 0.1.5 完全相同。
