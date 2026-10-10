import gzip, json, glob, base64
D='/workspace/wt-collect/data/2026-10-10/'
def load(sess):
    recs=[]
    for f in sorted(glob.glob(D+sess+'/samples-*.jsonl.gz')):
        with gzip.open(f,'rt') as fh:
            for line in fh:
                line=line.strip()
                if line: recs.append(json.loads(line))
    recs.sort(key=lambda r:(r['wall_ms'],r['seq']))
    return recs
def body(r):
    if r.get('body') is not None: return r['body']
    if r.get('body_b64'): return base64.b64decode(r['body_b64'])
    return None
def js(r):
    b=body(r)
    if b is None or isinstance(b,bytes): return None
    try: return json.loads(b)
    except Exception: return None
