"""PNG: enemy tracks on the map (blink-state colouring) + per-track blink timeline."""
import pickle, datetime, matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from PIL import Image
plt.rcParams['font.sans-serif'] = ['Noto Sans CJK SC', 'WenQuanYi Zen Hei', 'DejaVu Sans']
plt.rcParams['axes.unicode_minus'] = False
R = pickle.load(open('/tmp/enemy_results_full.pkl', 'rb'))
MAPS = ['map_ground_3a7d46abe5a8.png', 'map_air_7e6edd02c5ec.png']
COL = {0: '#ff8c00', 2: '#ff00ff'}
for si, (s, mp) in enumerate(zip(R, MAPS)):
    img = Image.open(mp).convert('RGB'); W = img.size[0]
    fig, (ax, ax2) = plt.subplots(1, 2, figsize=(20, 9.5), gridspec_kw={'width_ratios': [1, 1.15]})
    ax.imshow(img)
    pl = s['player_track']
    ax.plot([p[1] * W for p in pl], [p[2] * W for p in pl], '-', c='yellow', lw=2.5, label='本人轨迹')
    if pl:
        ax.plot(pl[0][1] * W, pl[0][2] * W, '*', c='yellow', ms=16, mec='k')
    cs = [c for c in s['contacts'] if c['type'] == 'ground_model']
    xs, ys = [], []
    for c in cs:
        h = c['_hist']
        for a, b in zip(h, h[1:]):
            ax.plot([a[1] * W, b[1] * W], [a[2] * W, b[2] * W], '-', c=COL.get(a[3], 'w'), lw=3)
        last = h[-1]
        ax.scatter([p[1] * W for p in h], [p[2] * W for p in h], c=[COL.get(p[3], 'w') for p in h], s=70, edgecolors='k', linewidths=0.5, zorder=3)
        ax.annotate('#%d %s' % (c['contact_id'], c['icon']), (last[1] * W, last[2] * W), xytext=(6, -6), textcoords='offset points', color='w', fontsize=8,
                    bbox=dict(fc='k', alpha=0.5, lw=0))
        xs += [p[1] * W for p in h]; ys += [p[2] * W for p in h]
    xs += [p[1] * W for p in pl]; ys += [p[2] * W for p in pl]
    pad = 120
    ax.set_xlim(min(xs) - pad, max(xs) + pad); ax.set_ylim(max(ys) + pad, min(ys) - pad)
    ax.plot([], [], c=COL[0], lw=4, label='#fa0C00 blink0'); ax.plot([], [], c=COL[2], lw=4, label='#f00C00 blink2')
    ax.legend(loc='lower right', fontsize=9)
    ax.set_title('%s %s–%s 地面敌对象轨迹（颜色=该观测的 blink 状态）' % (s['session'][-4:], s['start'], s['end']))
    # timeline
    t0 = min(c['blink_timeline'][0]['t_ms'] for c in s['contacts'])
    rows = s['contacts']
    for i, c in enumerate(rows):
        h = c['_hist']
        for a, b in zip(h, h[1:]):
            ax2.plot([(a[0] - t0) / 1000, (b[0] - t0) / 1000], [i, i], c=COL.get(a[3], 'gray'), lw=6, solid_capstyle='butt')
        mv = [((a[0] - t0) / 1000) for a, b in zip(h, h[1:]) if (a[1], a[2]) != (b[1], b[2])]
        ax2.plot(mv, [i + 0.32] * len(mv), '|', c='k', ms=4)
    for e in s['hud_live']:
        t = datetime.datetime.strptime('2026-10-10 ' + e['t'], '%Y-%m-%d %H:%M:%S.%f').timestamp() * 1000
        ax2.axvline((t - t0) / 1000, c='red', ls='--'); ax2.text((t - t0) / 1000, len(rows) - 0.3, 'hudmsg kill: ' + e['msg'].split('击毁了')[-1].strip(), color='red', fontsize=8, rotation=90, va='top')
    for b in s['speed_minus1_blips']:
        t = datetime.datetime.strptime('2026-10-10 ' + b, '%Y-%m-%d %H:%M:%S.%f').timestamp() * 1000
        ax2.axvline((t - t0) / 1000, c='blue', ls=':'); ax2.text((t - t0) / 1000, -0.9, 'speed=-1', color='blue', fontsize=8)
    ax2.set_yticks(range(len(rows))); ax2.set_yticklabels(['#%d %s %s' % (c['contact_id'], 'air' if c['type'] == 'aircraft' else 'gnd', c['icon']) for c in rows], fontsize=7)
    ax2.set_xlabel('秒（相对 %s）；黑色短竖线=该帧位置有变化' % datetime.datetime.fromtimestamp(t0 / 1000).strftime('%H:%M:%S.%f')[:-3])
    ax2.set_ylim(-1.2, len(rows)); ax2.invert_yaxis(); ax2.grid(axis='x', alpha=0.3)
    ax2.set_title('每个追踪对象的 blink 时间线（橙=fa0C00/0，品红=f00C00/2）')
    fig.tight_layout()
    out = 'enemy_tracks_%s.png' % ('ground_testdrive' if si == 0 else 'air_testflight')
    fig.savefig(out, dpi=110); print(out)
