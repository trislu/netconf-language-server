#!/usr/bin/env python3
"""Aggregate docs/perf/giant-scale-2026-09/results/raw.tsv (10k-interval grid,
5 reps) into summary.csv + report.html (self-contained inline-SVG charts).
Pure stdlib."""
import os, statistics, datetime
HERE=os.path.dirname(os.path.abspath(__file__))
RAW=os.path.join(HERE,"results","raw.tsv")
def load():
    rows=[]
    with open(RAW) as f:
        head=f.readline().strip().split("\t")
        for ln in f:
            v=ln.strip().split("\t")
            if len(v)<len(head): continue
            rows.append(dict(zip(head,v)))
    for r in rows:
        for k in ("files","rep","catalog_rss_kb","catalog_peak_kb","modules","submodules","diags"):
            r[k]=int(r[k])
        for k in ("catalog_s","closure_s"):
            r[k]=float(r[k])
        r["named"]=int(r["named"])
    return rows
rows=load()
if not rows:
    raise SystemExit("raw.tsv empty — run the grid driver first")
files=sorted({r["files"] for r in rows})
def agg(K):
    sub=[r for r in rows if r["files"]==K]
    def f(k,fn):
        xs=[r[k] for r in sub]
        return fn(xs)
    cat_s=[r["catalog_s"] for r in sub]
    clo_s=[r["closure_s"] for r in sub]
    mods=[r["modules"] for r in sub]
    rss=[r["catalog_rss_kb"] for r in sub]
    peak=[r["catalog_peak_kb"] for r in sub]
    diags={r["diags"] for r in sub}
    return {
        "K":K,"n":len(sub),
        "cat_mean":statistics.fmean(cat_s),"cat_med":statistics.median(cat_s),
        "cat_min":min(cat_s),"cat_max":max(cat_s),
        "thr":K/statistics.fmean(cat_s),
        "rss_mean_mb":statistics.fmean(rss)/1024,"peak_max_mb":max(peak)/1024,
        "clo_mean":statistics.fmean(clo_s),"clo_med":statistics.median(clo_s),
        "clo_max":max(clo_s),
        "mods_med":statistics.median(mods),
        "diags_set":sorted(diags),
        "named":f("named",statistics.median),
    }
stats=[agg(K) for K in files]
with open(os.path.join(HERE,"summary.csv"),"w") as f:
    f.write("files,reps,catalog_s_mean,catalog_s_median,catalog_s_min,catalog_s_max,throughput_files_s,"
            "catalog_rss_mean_mb,catalog_peak_max_mb,closure_s_mean,closure_s_median,closure_s_max,"
            "modules_median,diags,named_median\n")
    for s in stats:
        f.write(f"{s['K']},{s['n']},{s['cat_mean']:.4f},{s['cat_med']:.4f},{s['cat_min']:.4f},{s['cat_max']:.4f},"
                f"{s['thr']:.0f},{s['rss_mean_mb']:.1f},{s['peak_max_mb']:.1f},{s['clo_mean']:.4f},{s['clo_med']:.4f},"
                f"{s['clo_max']:.4f},{s['mods_med']},{'|'.join(map(str,s['diags_set']))},{s['named']}\n")

# ---------- SVG chart helpers ----------
def chart(title, series, xmax, ymax, xstep, ystep, xlab, ylab, W=880, H=300):
    """series: list of (color, label, points [(x,y)...], style) ; style in {line,bar}"""
    pad_l,pad_b,pad_t,pad_r=64,34,26,14
    def X(x): return pad_l+(W-pad_l-pad_r)*x/xmax
    def Y(y): return pad_t+(H-pad_t-pad_b)*(1-y/ymax)
    o=[f'<svg width="{W}" height="{H}" viewBox="0 0 {W} {H}"><rect width="{W}" height="{H}" fill="#fff"/>']
    for x in range(0,int(xmax)+1,xstep):
        o.append(f'<line x1="{X(x):.1f}" y1="{Y(0):.1f}" x2="{X(x):.1f}" y2="{Y(ymax):.1f}" stroke="#f0f0f0"/>')
        o.append(f'<text x="{X(x):.1f}" y="{Y(0)+15:.1f}" font-size="11" text-anchor="middle" fill="#555">{x/1000:.0f}k</text>')
    for y in range(0,int(ymax)+1,ystep):
        o.append(f'<line x1="{X(0):.1f}" y1="{Y(y):.1f}" x2="{X(xmax):.1f}" y2="{Y(y):.1f}" stroke="#f0f0f0"/>')
        o.append(f'<text x="{X(0)-6:.1f}" y="{Y(y)+4:.1f}" font-size="11" text-anchor="end" fill="#555">{y}</text>')
    o.append(f'<text x="{pad_l+(W-pad_l-pad_r)/2:.1f}" y="{H-2:.1f}" font-size="12" text-anchor="middle" fill="#222">{xlab}</text>')
    o.append(f'<text x="12" y="{pad_t+(H-pad_t-pad_b)/2:.1f}" font-size="12" text-anchor="middle" fill="#222" transform="rotate(-90 12 {pad_t+(H-pad_t-pad_b)/2:.1f})">{ylab}</text>')
    o.append(f'<text x="{pad_l}" y="14" font-size="12" fill="#222" font-weight="600">{title}</text>')
    lx=pad_l+150
    for color,label,pts,style in series:
        if style=="line":
            s=" ".join(f"{X(x):.1f},{Y(y):.1f}" for x,y in pts)
            o.append(f'<polyline points="{s}" fill="none" stroke="{color}" stroke-width="2"/>')
            for x,y in pts:
                o.append(f'<circle cx="{X(x):.1f}" cy="{Y(y):.1f}" r="2.6" fill="{color}"/>')
        else:
            bw=max(6.0, (W-pad_l-pad_r)/len(pts)*0.5)
            for x,y in pts:
                o.append(f'<rect x="{X(x)-bw/2:.1f}" y="{Y(y):.1f}" width="{bw:.1f}" height="{max(0.5,Y(0)-Y(y)):.1f}" fill="{color}" opacity="0.85"/>')
        o.append(f'<rect x="{lx}" y="{12}" width="12" height="12" fill="{color}"/>')
        o.append(f'<text x="{lx+16}" y="22" font-size="11" fill="#222">{label}</text>')
        lx+= max(90, len(label)*6.5)
    o.append("</svg>")
    return "\n".join(o)

def band(xs, ymed, ymin, ymaxv, color):
    return " ".join(f"{x:.1f},{y:.1f}" for x,y in zip(xs,ymed)), \
           [(xs[i],ymin[i]) for i in range(len(xs))] + [(xs[len(xs)-1-i],ymaxv[len(xs)-1-i]) for i in range(len(xs))]

xs=[s["K"] for s in stats]
xmax=160000
xstep=20000
c1=chart("Catalog scan wall time vs tree size (median of 5; error bars = min/max)",
         [("#c8c8c8","min–max range",[(xs[i],min(stats[i]['cat_min'],stats[i]['cat_max'])) for i in range(len(xs))], "band"),
          ("#2b7fba","median catalog wall (s)",[(xs[i],stats[i]['cat_med']) for i in range(len(xs))],"line")],
         xmax, 60, xstep, 10, "files in the catalog (10k grid to full)",
         "wall time (s)") if False else None
# build minmax band explicitly (svg polygon) + median line
def chart_wall():
    pad_l,pad_b,pad_t,pad_r,W,H=64,34,26,14,880,300
    ymax=60
    def X(x): return pad_l+(W-pad_l-pad_r)*x/xmax
    def Y(y): return pad_t+(H-pad_t-pad_b)*(1-y/ymax)
    o=[f'<svg width="{W}" height="{H}" viewBox="0 0 {W} {H}"><rect width="{W}" height="{H}" fill="#fff"/>']
    for x in range(0,xmax+1,xstep):
        o.append(f'<line x1="{X(x):.1f}" y1="{Y(0):.1f}" x2="{X(x):.1f}" y2="{Y(ymax):.1f}" stroke="#f0f0f0"/>')
        o.append(f'<text x="{X(x):.1f}" y="{Y(0)+15:.1f}" font-size="11" text-anchor="middle" fill="#555">{x//1000}k</text>')
    for y in range(0,ymax+1,10):
        o.append(f'<line x1="{X(0):.1f}" y1="{Y(y):.1f}" x2="{X(xmax):.1f}" y2="{Y(y):.1f}" stroke="#f0f0f0"/>')
        o.append(f'<text x="{X(0)-6:.1f}" y="{Y(y)+4:.1f}" font-size="11" text-anchor="end" fill="#555">{y}</text>')
    poly_up=[(X(xs[i]),Y(max(stats[i]['cat_min'],stats[i]['cat_max']))) for i in range(len(xs))]
    poly_dn=[(X(xs[i]),Y(min(stats[i]['cat_min'],stats[i]['cat_max']))) for i in range(len(xs))][::-1]
    bandpts=" ".join(f"{a:.1f},{b:.1f}" for a,b in poly_up+poly_dn)
    o.append(f'<polygon points="{bandpts}" fill="#cfe4f2"/>')
    med=[(X(xs[i]),Y(stats[i]['cat_med'])) for i in range(len(xs))]
    o.append('<polyline points="'+ " ".join(f"{a:.1f},{b:.1f}" for a,b in med)+'" fill="none" stroke="#2b7fba" stroke-width="2.2"/>')
    for a,b in med: o.append(f'<circle cx="{a:.1f}" cy="{b:.1f}" r="3" fill="#2b7fba"/>')
    o.append('<rect x="700" y="12" width="12" height="12" fill="#2b7fba"/><text x="716" y="22" font-size="11" fill="#222">median (5 reps)</text>')
    o.append('<rect x="836" y="12" width="12" height="12" fill="#cfe4f2"/><text x="700" y="12" fill="none"></text>')
    o.append(f'<text x="720" y="34" font-size="11" fill="#666">band = min–max</text>')
    o.append(f'<text x="{pad_l+(W-pad_l-pad_r)/2:.1f}" y="{H-2:.1f}" font-size="12" text-anchor="middle" fill="#222">files in the catalog (10k grid to full 165,521)</text>')
    o.append(f'<text x="12" y="{pad_t+(H-pad_t-pad_b)/2:.1f}" font-size="12" text-anchor="middle" fill="#222" transform="rotate(-90 12 {pad_t+(H-pad_t-pad_b)/2:.1f})">catalog wall (s)</text>')
    o.append(f'<text x="{pad_l}" y="14" font-size="12" fill="#222" font-weight="600">1 · Catalog scan wall time — median of 5 with min–max band</text>')
    o.append("</svg>")
    return "\n".join(o)

def chart_thr():
    pts=[(s["K"],s["thr"]) for s in stats]
    return chart("2 · Catalog throughput (files/s, median of 5)", [("#3a9e5f","files/s",pts,"line")],
                 160000, 20000, 20000, 5000, "files in the catalog", "throughput (files/s)")
def chart_rss():
    pts=[(s["K"],s["peak_max_mb"]) for s in stats]
    return chart("3 · Parallel-scan reported RSS/peak (max of 5) — transient parse high-water",
                 [("#c0504d","peak RSS (MB)",pts,"bar")],160000,3000,20000,500,
                 "files in the catalog","reported peak (MB)")
def chart_clo():
    pts=[(s["K"],s["clo_med"]) for s in stats]
    return chart("4 · Open-closure compile time vs tree size (median of 5)",
                 [("#7a4fb0","closure compile (s)",pts,"line")],160000,5,20000,1,
                 "files in the catalog","closure compile (s)")
def chart_mods():
    pts=[(s["K"],s["mods_med"]) for s in stats]
    return chart("5 · Closure size (modules compiled for the 20 picked roots, median)",
                 [("#e09a2c","modules",pts,"line")],160000,80,20000,20,
                 "files in the catalog","modules in closure")

rows_html=[]
for s in stats:
    rows_html.append(
        f"<tr><td>{s['K']:,}</td><td>{s['n']}</td><td>{s['cat_mean']:.2f}</td><td>{s['cat_med']:.2f}</td>"
        f"<td>{s['cat_min']:.2f}–{s['cat_max']:.2f}</td><td>{s['thr']:.0f}</td>"
        f"<td>{s['rss_mean_mb']:.0f}</td><td>{s['peak_max_mb']:.0f}</td>"
        f"<td>{s['clo_mean']:.3f}</td><td>{s['clo_max']:.3f}</td><td>{s['mods_med']}</td>"
        f"<td>{'|'.join(map(str,s['diags_set']))}</td></tr>")
stamp=datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
html=f"""<!doctype html><html><head><meta charset="utf-8"><title>YANG giant-scale serveperf report</title>
<style>
body{{font-family: system-ui,sans-serif; margin:2rem auto; max-width:1020px; color:#222;}}
h1{{font-size:1.3rem}} h2{{font-size:1.05rem; margin-top:2rem}}
svg{{border:1px solid #ddd; border-radius:6px; background:#fff; margin:0.4rem 0}}
table{{border-collapse:collapse; font-size:.8rem; margin:1rem 0; width:100%}}
td,th{{border:1px solid #ccc; padding:3px 6px; text-align:right}}
th:first-child,td:first-child{{text-align:left}}
.note{{font-size:.85rem; color:#555; max-width:95ch}}
code{{background:#f4f4f4; padding:0 3px}}
</style></head><body>
<h1>YANG catalog + open-closure serving — giant-scale grid report</h1>
<p class="note">Generated {stamp} from <code>results/raw.tsv</code> ({len(rows)} runs: every 10k step of the
tree, 10,000 → 165,521 files, 5 repetitions each). Tool: <code>examples/serveperf</code> (LTO release
profile, 16-way parallel catalog scan via <code>CatalogIndex::scan_many_files</code>, then materialize +
compile the open closure of the first 20 distinct module names, text-light ON). Generator:
<code>make_report.py</code> (re-run to regenerate).</p>
{chart_wall()}
{chart_thr()}
{chart_rss()}
{chart_clo()}
{chart_mods()}
<p class="note"><b>Chart 3 caveat:</b> the parallel scan's reported RSS/peak is dominated by the 16-way
transient parse high-water (allocator keeps freed CST arenas) — it is <i>not</i> the retained catalog
footprint (~230 MB sequential, chart 1 of the earlier <a href="../../charts/serving-scale.html">charts</a>).
The resident language server scans sequentially and stays at ~255 MB on the full tree
(sequential retained catalog ~230 MB; see yrepo docs/memory-findings.md).</p>
<h2>Summary (per interval, over 5 reps)</h2>
<table>
<tr><th>files</th><th>reps</th><th>cat mean (s)</th><th>cat median (s)</th><th>cat min–max (s)</th><th>files/s</th>
<th>RSS mean (MB)*</th><th>peak max (MB)*</th><th>closure mean (s)</th><th>closure max (s)</th><th>modules (med)</th><th>diags seen</th></tr>
{''.join(rows_html)}
</table>
<p class="note">* parallel-scan reported RSS — transient high-water artifact, not retention.</p>
<p class="note">Data columns (raw.tsv): files, rep, named, catalog_s, catalog_rss_kb, catalog_peak_kb,
closure_s, modules, submodules, diags. Environment: 16 cores / 15 GB RAM; runs on the restored YangModels-scale
population (the stable "YangModels vendor-tree scale" sample).</p>
<p class="note"><b>Closure-root note:</b> the closure roots are the first 20 distinct module names of each
interval's catalog, so the *content* of the closure changes with the interval — at 10k files the pick lands on
a heavier closure (~3 s, 24 modules, 3 diags) while at 20k+ it is the same light set (~0.05 s, 31 modules,
107 diags). The catalog numbers are the tree-scale signal; the closure column measures a fixed protocol, not a
fixed module set.</p>
</body></html>"""
open(os.path.join(HERE,"report.html"),"w").write(html)
print("wrote summary.csv and report.html with", len(stats), "intervals")
