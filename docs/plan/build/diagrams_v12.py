from diagrams import w, pad, hl, single, dual

def arch():
    C = 64
    def c(s): return pad(s, C, 'c').rstrip()
    def box(lines, inner):
        res = [c('┌' + hl(inner) + '┐')]
        for s in lines: res.append(c('│' + pad(s, inner, 'c') + '│'))
        res.append(c('└' + hl(inner) + '┘'))
        return res
    mid = ' ' * 31
    out = [c('参战队员 PC（最多 8 个 Agent）'),
           c('War Thunder → 127.0.0.1:8111（全部端点）'), c('│'), c('▼')]
    out += box(['Rust Agent（由 Neo 本地代理改造）', '全量轮询 10Hz / 1Hz，本人识别',
                '量化观测帧 / 原始批次 / 地图图片'], 36)
    out.append(mid + '│ WSS（Protobuf）+ HTTPS PUT（域名）')
    out.append(c('▼'))
    out += box(['香港 VPS（单实例，基于 Neo cloud-tactical-server）', 'Rust + Tokio + Axum + prost',
                'Room / 设备令牌 / 时钟同步', 'Ingest -> 世界坐标 -> Fusion',
                '敌情显示策略（enemyPolicy）', 'Annotation Service（定序 + 撤销）',
                'Map Asset Store（按哈希）', 'Raw Store（zstd 分段，按期清理）', 'SQLite (WAL)'], 52)
    out.append(mid + '│')
    out.append(' ' * 17 + '┌' + hl(12) + '┴' + hl(12) + '┐')
    out.append(' ' * 17 + '│' + ' ' * 26 + '│')
    out.append(' ' * 6 + '10Hz FusedSnapshot（经策略过滤）' + '  ' + '即时标绘事件')
    out.append(' ' * 17 + '│' + ' ' * 26 + '│')
    out.append(' ' * 17 + '└' + hl(12) + '┬' + hl(12) + '┘')
    out.append(c('▼'))
    out.append(c('手机 / PC 响应式网页（React + milsymbol）'))
    out += box(['陆战单图 | 空战单图 | 双图', '友军 / 自动敌情图层（按策略）/ 战术标绘'], 44)
    return out

def deploy():
    x = 12
    return [' 客户端： https://<域名>/',
            '          wss://<域名>/ws/agent   wss://<域名>/ws/web',
            pad('', x) + '│',
            pad('', x - 2) + 'Caddy（域名证书自动签发 / 续期）',
            pad('', x) + '│',
            pad('', x - 4) + 'Rust / Axum',
            pad('', x - 2) + '├── 静态前端',
            pad('', x - 2) + '├── /api/rooms/*      房间 / 加入 / 策略 / 地图图片',
            pad('', x - 2) + '├── /ws/agent         Agent 上行（Protobuf）',
            pad('', x - 2) + '├── /ws/web           网页下行 + 标绘（Protobuf）',
            pad('', x - 2) + '├── /assets/maps/*    按哈希缓存的底图',
            pad('', x - 2) + '└── SQLite + Raw Store（/var/lib/wttc/raw）']

def tree():
    items = [('wt-tactical-command/', ''), ('├── apps/', ''),
             ('│   ├── web/', '# React + TS；V10 全屏 UI + milsymbol'),
             ('│   ├── agent/', '# Rust；由 Neo src-tauri 代理改造'),
             ('│   └── server/', '# Rust / Axum；fork Neo cloud-tactical-server'),
             ('├── crates/', ''),
             ('│   ├── protocol/', '# protos/*.proto + prost 生成 + 校验'),
             ('│   ├── wt8111/', '# 8111 解析器（数字 / 字符串兼容）'),
             ('│   ├── fusion/', '# 世界坐标融合 / 敌情策略'),
             ('│   └── map-core/', '# 陆空变换 / 地图哈希 / 几何'),
             ('├── packages/', ''),
             ('│   ├── proto-ts/', '# protobuf-es 生成的 TS 类型'),
             ('│   └── symbols/', '# milsymbol 封装 + 战术图形'),
             ('├── upstream/', ''),
             ('│   └── wt-8111-neo/', '# 上游镜像（git subtree）'),
             ('├── fixtures/', ''),
             ('│   ├── 8111-ground/', '# 实测样本（含原始批次回放）'),
             ('│   ├── 8111-air/', '# 含 WTM JH-7 夹具'),
             ('│   └── map-hashes/', '# 引入 WTP 哈希表'),
             ('├── infra/', '# caddy / systemd / backup / raw 清理'),
             ('├── docs/', ''),
             ('│   ├── 8111_API_SPEC.md', '# 本计划附录 A'),
             ('│   ├── adr/', '# 关键架构决策'),
             ('│   ├── install/', '# Agent 图文安装说明'),
             ('│   └── qa/', '# 测试记录'),
             ('└── README.md', '')]
    col = max(w(a) for a, _ in items) + 2
    return [(pad(a, col) + b).rstrip() for a, b in items]

D = {'single': single(), 'dual': dual(), 'arch': arch(), 'deploy': deploy(), 'tree': tree()}
if __name__ == '__main__':
    for k, v in D.items():
        print(k, max(w(s) for s in v)); print('\n'.join(v))
