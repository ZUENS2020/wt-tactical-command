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
| `dist/wt-collector-win64.zip` | Windows package (exe + toml with token + Chinese README) |
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
