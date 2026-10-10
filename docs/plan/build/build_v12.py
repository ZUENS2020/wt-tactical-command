import markdown, re, subprocess, pathlib
from diagrams_v12 import D
base = pathlib.Path('/workspace/wt-plan')
md = (base/'build/template_v12_part1.md').read_text() + '\n' + (base/'build/template_v12_part2.md').read_text()
for k, v in D.items():
    md = md.replace('{{%s}}' % k, '\n'.join(v))
spec = (base/'8111_API_SPEC.md').read_text()
spec = spec[:spec.index('\n## 11. 参考链接')] + '\n\n## 11. 参考链接\n\n见第 12 节（含全部仓库与论坛链接）。\n'
lines = spec.splitlines()
assert lines[0].startswith('# ')
body = []
for ln in lines[1:]:
    m = re.match(r'^(#{2,5}) (.*)', ln)
    if m:
        lvl = len(m.group(1)) + 1
        txt = m.group(2)
        txt = re.sub(r'^(\d+(?:\.\d+)*)\. ', r'A.\1 ', txt) if lvl == 3 else re.sub(r'^(\d+\.\d+) ', r'A.\1 ', txt)
        ln = '#' * lvl + ' ' + txt
    body.append(ln)
md = md.replace('{{SPEC}}', '\n'.join(body).strip())
name = 'WT_Tactical_Command_Plan_v1.2'
(base/f'{name}.md').write_text(md)
html_body = markdown.markdown(md, extensions=['tables', 'fenced_code', 'md_in_html', 'attr_list'])
css = (base/'build/style.css').read_text() + (base/'build/style_v12.css').read_text()
html = f'''<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8">
<title>WT Tactical Command V10 Plan v1.2</title><style>{css}</style></head><body>{html_body}</body></html>'''
(base/f'{name}.html').write_text(html)
subprocess.run(['google-chrome', '--headless=new', '--no-sandbox', '--disable-gpu', '--no-pdf-header-footer',
                f'--print-to-pdf={base}/{name}.pdf', f'file://{base}/{name}.html'], check=True,
               stderr=subprocess.DEVNULL)
print('ok')
