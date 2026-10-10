import sys, glob, gzip, os, hashlib
root = sys.argv[1]
for d in sorted(glob.glob(f"{root}/*/*/")):
    n = 0; seqs = set(); h = hashlib.sha256()
    for f in sorted(glob.glob(d + "samples-*")):
        op = gzip.open if f.endswith(".gz") else open
        with op(f, "rb") as fh:
            for l in fh:
                n += 1; h.update(l)
    imgs = len(glob.glob(d + "images/*"))
    print(f"{os.path.basename(d.rstrip('/'))}: records {n} images {imgs} sha {h.hexdigest()[:12]}")
