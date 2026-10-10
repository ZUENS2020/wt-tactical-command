#!/usr/bin/env python3
"""TCP proxy adding fixed one-way delay per direction (RTT = 2*delay) and an optional bandwidth cap.
Usage: delayproxy.py --listen 18299 --target 127.0.0.1:18199 --delay-ms 250 [--mbit 10]"""
import argparse, asyncio, time
ap = argparse.ArgumentParser()
ap.add_argument("--listen", type=int, required=True)
ap.add_argument("--target", required=True)
ap.add_argument("--delay-ms", type=float, default=250)
ap.add_argument("--mbit", type=float, default=0)
ap.add_argument("--delay-file", default="", help="if set, one-way delay (ms) is re-read from this file (live change)")
a = ap.parse_args()
TH, TP = a.target.rsplit(":", 1)
D = a.delay_ms / 1000
import os
_df = {"t": 0, "v": D}
def cur_delay():
    if not a.delay_file:
        return D
    now = time.monotonic()
    if now - _df["t"] > 0.2:
        _df["t"] = now
        try:
            _df["v"] = float(open(a.delay_file).read().strip()) / 1000
        except Exception:
            pass
    return _df["v"]

async def pipe(r, w):
    q = asyncio.Queue()
    async def rd():
        while True:
            b = await r.read(65536)
            await q.put((time.monotonic() + cur_delay(), b))
            if not b:
                return
    async def wr():
        free_at = 0.0
        while True:
            t, b = await q.get()
            if a.mbit:  # serialize at link rate (shared per direction per connection)
                now = time.monotonic()
                start = max(now, free_at)
                free_at = start + len(b) * 8 / (a.mbit * 1e6)
                t = max(t, free_at)
            dt = t - time.monotonic()
            if dt > 0:
                await asyncio.sleep(dt)
            if not b:
                try: w.write_eof()
                except Exception: pass
                return
            w.write(b)
            await w.drain()
    await asyncio.gather(rd(), wr(), return_exceptions=True)
    w.close()

async def handle(cr, cw):
    await asyncio.sleep(cur_delay())  # SYN
    try:
        sr, sw = await asyncio.open_connection(TH, int(TP))
    except Exception:
        cw.close(); return
    await asyncio.sleep(cur_delay())  # SYN-ACK
    await asyncio.gather(pipe(cr, sw), pipe(sr, cw))

async def main():
    s = await asyncio.start_server(handle, "127.0.0.1", a.listen)
    print(f"delayproxy :{a.listen} -> {a.target} one-way {a.delay_ms} ms, cap {a.mbit or 'none'} Mbit/s", flush=True)
    async with s:
        await s.serve_forever()
asyncio.run(main())
