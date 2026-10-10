# Generates ASCII diagrams aligned for Noto Sans Mono CJK SC (ASCII=1 unit, all else=2 units)
def w(s): return sum(1 if ord(c) < 128 else 2 for c in s)
def pad(s, n, align='l'):
    d = n - w(s)
    assert d >= 0, (s, n)
    if align == 'c':
        l = d // 2; return ' ' * l + s + ' ' * (d - l)
    return s + ' ' * d
H = '─'
def hl(n): assert n % 2 == 0, n; return H * (n // 2)

def single():
    L, R = 34, 24
    T = L + 2 + R
    out = ['┌' + hl(T) + '┐',
           '│' + pad(' WT Tactical [陆战] [空战] [双地图]  [指挥/成员]', T) + '│ ← 唯一顶栏',
           '├' + hl(L) + '┬' + hl(R + 0) + '┤']
    # note: ┬ takes 2 units, so L + 2 + R == T
    left = ['', '', '正方形战术地图', '占满可用高度，保持 1:1', '', '']
    right = [' 选择 标点 路线', ' 区域 来源', '', ' 当前工具的少量控件', ' 选中后才有名称/删除', '']
    for a, b in zip(left, right):
        out.append('│' + pad(a, L, 'c') + '│' + pad(b, R) + '│')
    out.append('└' + hl(L) + '┴' + hl(R) + '┘')
    return out

def dual():
    L = R = 28
    T = L + 2 + R
    out = ['┌' + hl(T) + '┐',
           '│' + pad(' WT Tactical [陆战] [空战] [双地图]  [指挥/成员]', T) + '│',
           '├' + hl(L) + '┬' + hl(R) + '┤']
    for a, b in [('', ''), ('正方形陆战地图', '正方形空战地图'), ('', '')]:
        out.append('│' + pad(a, L, 'c') + '│' + pad(b, R, 'c') + '│')
    out.append('└' + hl(L) + '┴' + hl(R) + '┘')
    return out

def arch():
    C = 60
    def c(s): return pad(s, C, 'c').rstrip()
    def box(lines, inner):
        res = [c('┌' + hl(inner) + '┐')]
        for s in lines: res.append(c('│' + pad(s, inner, 'c') + '│'))
        res.append(c('└' + hl(inner) + '┘'))
        return res
    out = [c('参战队员 PC（最多 8 个 Agent）'), c('War Thunder → 127.0.0.1:8111'), c('│'), c('▼'),
           c('Rust Agent（只读）'), c('本人位置 / 朝向 / 本人状态 / 本地双图来源信息'),
           ' ' * 29 + '│ WSS / HTTPS（域名）', c('▼')]
    out += box(['香港 VPS（单实例）', 'Rust + Tokio + Axum', 'RoomState 内存同步', 'Annotation Service',
                'Ground/Air Map Cache', 'SQLite (WAL)'], 32)
    out.append(' ' * 29 + '│')
    out.append(' ' * 17 + '┌' + hl(10) + '┴' + hl(10) + '┐')
    out.append(' ' * 17 + '│' + ' ' * 22 + '│')
    out.append(' ' * 11 + '10Hz 聚合态势' + ' ' * 12 + '即时标绘事件')
    out.append(' ' * 17 + '│' + ' ' * 22 + '│')
    out.append(' ' * 17 + '└' + hl(10) + '┬' + hl(10) + '┘')
    out.append(c('▼'))
    out.append(c('手机 / PC 响应式网页'))
    out += box(['陆战单图 | 空战单图 | 双图', '地图位置、朝向、人工战术标绘'], 36)
    return out

def deploy():
    x = 12
    return [' 客户端： https://<域名>/',
            '          wss://<域名>/ws',
            pad('', x) + '│',
            pad('', x - 2) + 'Caddy（域名证书自动签发 / 续期）',
            pad('', x) + '│',
            pad('', x - 4) + 'Rust / Axum',
            pad('', x - 2) + '├── 静态前端',
            pad('', x - 2) + '├── /api/*',
            pad('', x - 2) + '├── /ws/*',
            pad('', x - 2) + '├── /assets/maps/*',
            pad('', x - 2) + '└── SQLite + 地图缓存']

def tree():
    items = [('wt-tactical-command/', ''), ('├── apps/', ''),
             ('│   ├── web/', '# React + TS；V10 全屏 UI'),
             ('│   ├── agent/', '# Windows Rust 8111 Agent'),
             ('│   └── server/', '# VPS Rust / Axum'),
             ('├── crates/', ''),
             ('│   ├── protocol/', '# 消息 Schema / 版本 / 验证'),
             ('│   └── map-core/', '# 地空变换 / Path / Bézier'),
             ('├── packages/', ''),
             ('│   └── map-icons/', '# 已校对图标映射 / 自绘符号'),
             ('├── fixtures/', ''),
             ('│   ├── 8111-ground/', '# 脱敏采集样本'),
             ('│   └── 8111-air/', '# 脱敏采集样本'),
             ('├── infra/', ''),
             ('│   ├── caddy/', ''),
             ('│   ├── systemd/', ''),
             ('│   └── sqlite-backup/', ''),
             ('├── docs/', ''),
             ('│   ├── adr/', '# 关键架构决策'),
             ('│   ├── api/', '# REST / WS 协议'),
             ('│   ├── install/', '# Agent 图文安装说明（AGENT-02）'),
             ('│   └── qa/', '# 测试记录'),
             ('└── README.md', '')]
    col = max(w(a) for a, _ in items) + 2
    return [(pad(a, col) + b).rstrip() for a, b in items]

D = {'single': single(), 'dual': dual(), 'arch': arch(), 'deploy': deploy(), 'tree': tree()}
if __name__ == '__main__':
    for k, v in D.items():
        print(k, max(w(s) for s in v)); print('\n'.join(v))
