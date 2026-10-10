import markdown, re, subprocess, pathlib
from diagrams import D
base = pathlib.Path('/workspace/wt-plan')
md = (base/'build/template.md').read_text()
for k, v in D.items():
    md = md.replace('{{%s}}' % k, '\n'.join(v))
name = 'WT_Tactical_Command_Plan_v1.1'
(base/f'{name}.md').write_text(md)
body = markdown.markdown(md, extensions=['tables', 'fenced_code', 'md_in_html', 'attr_list'])
css = (base/'build/style.css').read_text()
html = f'''<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8">
<title>WT Tactical Command V10 Plan v1.1</title><style>{css}</style></head><body>{body}</body></html>'''
(base/f'{name}.html').write_text(html)
subprocess.run(['google-chrome', '--headless=new', '--no-sandbox', '--disable-gpu', '--no-pdf-header-footer',
                f'--print-to-pdf={base}/{name}.pdf', f'file://{base}/{name}.html'], check=True,
               stderr=subprocess.DEVNULL)
print('ok')
