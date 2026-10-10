#!/usr/bin/env bash
# Usage: scripts/receiverctl.sh start|stop|restart|status|install-binary
cd /workspace/wt-collect
PIDF=logs/supervisor.pid
mkdir -p logs bin
running() { [ -f $PIDF ] && kill -0 "$(cat $PIDF)" 2>/dev/null; }
case "$1" in
  install-binary) # copy freshly built binary into bin/ (so cargo rebuilds don't disturb the running one)
    cp -f target/release/wt-receiver bin/wt-receiver.new && mv -f bin/wt-receiver.new bin/wt-receiver && echo installed ;;
  start)
    if running; then echo "already running (supervisor pid $(cat $PIDF))"; exit 0; fi
    [ -x bin/wt-receiver ] || cp target/release/wt-receiver bin/wt-receiver
    rm -f logs/stop
    nohup setsid scripts/receiver-supervisor.sh > /dev/null 2>&1 < /dev/null &
    echo $! > $PIDF; sleep 1; echo "started supervisor pid $(cat $PIDF)"; pgrep -af bin/wt-receiver ;;
  stop)
    touch logs/stop; pkill -f '^./bin/wt-receiver' ; sleep 1
    running && kill "$(cat $PIDF)" 2>/dev/null; rm -f $PIDF logs/stop; echo stopped ;;
  restart) # restarts only the receiver process; supervisor brings it back
    pkill -f '^./bin/wt-receiver'; sleep 3; pgrep -af bin/wt-receiver ;;
  status)
    running && echo "supervisor pid $(cat $PIDF)" || echo "supervisor NOT running"
    pgrep -af bin/wt-receiver; curl -s -m 3 http://100.94.249.78:18111/healthz && echo " (healthz ok)" ;;
  *) echo "usage: $0 start|stop|restart|status|install-binary"; exit 1 ;;
esac
