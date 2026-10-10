#!/usr/bin/env python3
"""Fault injector on 127.0.0.1:8111 for collector tests.
  hang      : accept connections, read request, never answer  -> read_timeout
  blackhole : listen with full backlog, never accept            -> connect_timeout
"""
import socket, sys, threading, time
mode = sys.argv[1]
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", 8111))
if mode == "hang":
    s.listen(128)
    def h(c):
        try:
            c.recv(65536); time.sleep(3600)
        except Exception: pass
    while True:
        c, _ = s.accept(); threading.Thread(target=h, args=(c,), daemon=True).start()
else:
    s.listen(0)
    fill = []
    for _ in range(4):  # fill accept queue so further SYNs are dropped
        c = socket.socket(); c.setblocking(False)
        try: c.connect(("127.0.0.1", 8111))
        except BlockingIOError: pass
        fill.append(c)
    time.sleep(3600)
