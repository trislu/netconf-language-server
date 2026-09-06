import subprocess, re, sys, time, os
SP="/home/dev/workspace/netconf-lsp/yrepo/target/release/examples/serveperf"
LIST="/tmp/yang_files.txt"
OUT="/home/dev/workspace/netconf-lsp/netconf-language-server/docs/perf/giant-scale-2026-09/results/raw.tsv"
TOTAL=165521
LOG="/tmp/serveperf_grid.log"
intervals=[k for k in range(10000,160001,10000)]+[TOTAL]
REPS=5
os.makedirs(os.path.dirname(OUT),exist_ok=True)
log=open(LOG,"a")
def say(m):
    print(m,flush=True); log.write(m+"\n"); log.flush()
say("driver start intervals="+str(len(intervals))+" reps="+str(REPS))
header="files\trep\tnamed\tcatalog_s\tcatalog_rss_kb\tcatalog_peak_kb\tclosure_s\tmodules\tsubmodules\tdiags"
with open(OUT,"w") as out:
    out.write(header+"\n")
cat_re=re.compile(r"\[serveperf\] catalog scanned=(\d+) named=(\d+) wall_s=([0-9.]+) rss_kb=(\d+) peak_kb=(\d+)")
clo_re=re.compile(r"\[serveperf\] closure roots=\d+ modules=(\d+) submodules=(\d+) diags=(\d+) wall_s=([0-9.]+) rss_kb=(\d+) peak_kb=(\d+)")
def run(K,rep):
    t0=time.time()
    p=subprocess.run([SP,"/home/dev/workspace/yang-modules/yang","--roots","20","--limit",str(K),"--file-list",LIST],
                     capture_output=True,text=True)
    wall=time.time()-t0
    if p.returncode!=0:
        say(f"ERR K={K} rep={rep} rc={p.returncode}: {p.stderr[:300]}"); return None
    cat=clo=None
    for line in p.stdout.splitlines():
        m=cat_re.search(line)
        if m: cat=m.groups()
        m=clo_re.search(line)
        if m: clo=m.groups()
    if not cat or not clo:
        say(f"ERR parse K={K} rep={rep}"); return None
    named=cat[1]; c_s=float(cat[2]); c_rss=int(cat[3]); c_peak=int(cat[4])
    mods,subs,diags,cl_s=int(clo[0]),int(clo[1]),int(clo[2]),float(clo[3])
    return (K,rep,named,c_s,c_rss,c_peak,cl_s,mods,subs,diags,wall)
count=0
for rep in range(1,REPS+1):
    for K in intervals:
        r=run(K,rep)
        if r is None: continue
        K,rep,named,c_s,c_rss,c_peak,cl_s,mods,subs,diags,wall=r
        with open(OUT,"a") as out:
            out.write(f"{K}\t{rep}\t{named}\t{c_s:.4f}\t{c_rss}\t{c_peak}\t{cl_s:.4f}\t{mods}\t{subs}\t{diags}\n")
        count+=1
        say(f"ok files={K} rep={rep} cat_s={c_s:.2f} rss_mb={c_rss/1024:.0f} closure_s={cl_s:.3f} mods={mods} diags={diags} run_wall={wall:.1f}s")
say(f"driver done runs={count}")
