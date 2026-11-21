#!/usr/bin/env python3
"""Collaboration simulation (no LLM) -> results/collab.md.
Usage: collab_sim.py [--s1-trials N=20] [--s2-trials N=200] [--only s1|s2]

Writers own disjoint cards (card i belongs to writer i % K) and writer w alone renames column w, so every
op has a well-defined expected effect and any missing effect is a lost update (an op may still be
"superseded" by a later op of the same writer, those are excluded). Ops of a writer are planned up front
from its own local plan, then executed against shared state.
"""
import os
import random
import shutil
import sys
import tempfile

import ops
from common import COLUMNS, HERE, build_model, git, init_repo, parse, read_dir, render, slug, sync_dir, write_dir


# ------------------------------------------------------------------ planning

def gen_plan(model, wid, K, n, rng):
    st = {c["id"]: {"col": [x["id"] for x in model["columns"]].index(c["column"])}
          for i, c in enumerate(model["cards"]) if i % K == wid}
    names, plan, addn, renn = list(COLUMNS), [], 0, 0
    for k in range(n):
        alive, r = sorted(st), rng.random()
        if not alive or r < 0.15:
            addn += 1
            title = f"Task w{wid} n{addn}"
            op = dict(kind="add", op="add", id=slug(title), title=title, col=rng.randrange(5),
                      desc=f"Added by writer {wid} (add {addn}).")
            st[op["id"]] = {"col": op["col"]}
        elif r < 0.55:
            x = rng.choice(alive)
            move = r < 0.40
            col = rng.randrange(5) if move else st[x]["col"]
            cands = [i for i in alive if i != x and st[i]["col"] == col]
            above = rng.choice(cands) if cands and rng.random() < 0.7 else None
            st[x]["col"] = col
            op = dict(kind="move" if move else "reorder", op="place", id=x, col=col, above=above)
        elif r < 0.80 or r >= 0.90 and (r >= 0.95 or wid >= 5):
            op = dict(kind="desc", op="desc", id=rng.choice(alive), line=f"Note w{wid}-{k}: updated.")
        elif r < 0.90:
            x = rng.choice(alive)
            del st[x]
            op = dict(kind="remove", op="remove", id=x)
        else:
            renn += 1
            op = dict(kind="rename", op="rename", col=wid, cur=names[wid], new=f"Lane w{wid} v{renn}")
            names[wid] = op["new"]
        op["uid"] = (wid, k)
        plan.append(op)
    mark_superseded(plan)
    return plan


def mark_superseded(plan):
    removed, pos, ren = set(), set(), set()
    for op in reversed(plan):
        k, s = op["kind"], False
        if k in ("move", "reorder"):
            s = op["id"] in removed or op["id"] in pos or bool(op["above"] and (op["above"] in removed or op["above"] in pos))
            pos.add(op["id"])
        elif k == "remove":
            removed.add(op["id"])
        elif k in ("add", "desc"):
            s = op["id"] in removed
        else:
            s = op["col"] in ren
            ren.add(op["col"])
        op["sup"] = s


def evaluate(fmt, fs, plans):
    """Return (parsed model, {uid: ok} for non-superseded ops)."""
    M = parse(fmt, fs)
    cm = {}
    for c in M["cards"]:
        cm.setdefault(c["id"], c)
    cidx = {c["id"]: i for i, c in enumerate(M["columns"])}
    order = {}
    for c in M["cards"]:
        order.setdefault(cidx.get(c["column"]), []).append(c["id"])
    pos = lambda i: cidx.get(cm[i]["column"]) if i in cm else None
    res = {}
    for plan in plans:
        for op in plan:
            if op["sup"]:
                continue
            k = op["kind"]
            if k == "add":
                ok = op["id"] in cm and op["desc"] in cm[op["id"]]["desc"]
            elif k == "desc":
                ok = op["id"] in cm and op["line"] in cm[op["id"]]["desc"].split("\n")
            elif k == "remove":
                ok = op["id"] not in cm
            elif k == "rename":
                ok = op["col"] < len(M["columns"]) and M["columns"][op["col"]]["name"] == op["new"]
            else:
                ok = pos(op["id"]) == op["col"]
                if ok and op["above"]:
                    o = order[op["col"]]
                    ok = pos(op["above"]) == op["col"] and o.index(op["id"]) < o.index(op["above"])
            res[op["uid"]] = ok
    return M, res


def err_counts(M):
    c = {"dup": 0, "twocol": 0, "broken": 0}
    for t, _ in M["errors"]:
        c[t] += 1
    return c


# ---------------------------------------------------------------- S1 engine

class Writer:
    def __init__(self, fmt, plan, mode):
        self.fmt, self.plan, self.mode = fmt, plan, mode
        self.i, self.phase, self.done, self.retries, self.status = 0, None, False, 0, {}

    def finish(self, st="ok"):
        self.status[self.op["uid"]] = st
        self.i += 1
        self.phase = None

    def step(self, fs):
        if self.phase is None:
            if self.i >= len(self.plan):
                self.done = True
                return
            self.op = self.plan[self.i]
            self.reads, self.view, self.phase = list(ops.needs(self.fmt, self.op, fs)), {}, "read"
        if self.phase == "read":
            if self.reads:
                p = self.reads.pop(0)
                self.view[p] = fs.get(p)
            if self.reads:
                return
            self.es = ops.edits(self.fmt, self.view, self.op)
            if not self.es:
                return self.finish()
            if self.mode == "strrep":
                self.queue, self.applied = list(self.es), 0
            else:
                self.writes = ops.snapshot_results(self.view, self.es)
            self.phase = "write"
            return
        if self.mode == "strrep":
            if ops.apply_live(fs, self.queue.pop(0)):
                self.applied += 1
                if not self.queue:
                    self.finish()
            else:
                self.finish("partial" if self.applied else "failed")
            return
        p, t = self.writes[0]
        if self.mode == "cas" and fs.get(p) != self.view.get(p):
            self.retries += 1
            self.phase = None
            if self.retries > 100000:
                self.finish("failed")
            return
        self.writes.pop(0)
        if t is None:
            fs.pop(p, None)
        else:
            fs[p] = t
        self.view[p] = t
        if not self.writes:
            self.finish()


def s1_trial(fmt, K, mode, t, model):
    rng = random.Random(f"s1-{K}-{t}")
    plans = [gen_plan(model, w, K, 50, random.Random(f"plan-{K}-{t}-{w}")) for w in range(K)]
    writers = [Writer(fmt, p, mode) for p in plans]
    fs = render(fmt, model)
    sched = random.Random(f"sched-{K}-{t}-{mode}-{fmt}")
    live = list(writers)
    while live:
        w = sched.choice(live)
        w.step(fs)
        if w.done:
            live.remove(w)
    M, res = evaluate(fmt, fs, plans)
    st = {}
    for w in writers:
        st.update(w.status)
    failed = sum(1 for v in st.values() if v != "ok")
    partial = sum(1 for v in st.values() if v == "partial")
    silent = sum(1 for u, ok in res.items() if not ok and st.get(u) == "ok")
    return dict(total=sum(len(p) for p in plans), evaluable=len(res), lost=silent, failed=failed, partial=partial,
                retries=sum(w.retries for w in writers), err=err_counts(M))


def run_s1(trials):
    model = build_model()
    out = []
    for K in (2, 4, 8):
        out += [f"### K = {K} writers x 50 ops (mean over {trials} seeds)", "",
                "| format | writer | ops | evaluable | silently lost | lost % | loud failures | partial failures | CAS retries | dup ids | two columns | broken/dangling |",
                "|---|---|---|---|---|---|---|---|---|---|---|---|"]
        for fmt in "ABC":
            for mode, label in (("naive", "naive read-modify-write"), ("cas", "compare-and-swap"), ("strrep", "string-replace (Edit)")):
                rs = [s1_trial(fmt, K, mode, t, model) for t in range(trials)]
                m = lambda f: sum(f(r) for r in rs) / trials
                ev = m(lambda r: r["evaluable"])
                out.append(f"| {fmt} | {label} | {m(lambda r: r['total']):.0f} | {ev:.1f} | {m(lambda r: r['lost']):.1f} | "
                           f"{100 * m(lambda r: r['lost']) / ev:.1f}% | {m(lambda r: r['failed']):.1f} | {m(lambda r: r['partial']):.1f} | "
                           f"{m(lambda r: r['retries']):.1f} | {m(lambda r: r['err']['dup']):.2f} | {m(lambda r: r['err']['twocol']):.2f} | "
                           f"{m(lambda r: r['err']['broken']):.2f} |")
                print(out[-1], flush=True)
        out.append("")
    return out


# ---------------------------------------------------------------- S2 engine

def run_s2(trials, tmp):
    model = build_model()
    out, cell = [], {}
    for fmt in "ABC":
        tpl = os.path.join(tmp, f"tpl-{fmt}")
        os.makedirs(tpl)
        write_dir(tpl, render(fmt, model))
        init_repo(tpl)
        attrs = "board.md merge=union\n" + ("cards/*.md merge=union\n" if fmt == "B" else "")
        for nb in (2, 4):
            for t in range(trials):
                plans = [gen_plan(model, b, nb, 5, random.Random(f"s2-{nb}-{t}-{b}")) for b in range(nb)]
                d = os.path.join(tmp, "trial")
                shutil.rmtree(d, ignore_errors=True)
                shutil.copytree(tpl, d)
                base = git(d, "rev-parse", "HEAD").stdout.strip()
                for b, plan in enumerate(plans):
                    git(d, "checkout", "-q", "-B", f"b{b}", base)
                    before = read_dir(d)
                    fs = dict(before)
                    for op in plan:
                        ops.run_op(fmt, fs, op)
                    sync_dir(d, before, fs)
                    git(d, "add", "-A")
                    git(d, "commit", "-q", "--allow-empty", "-m", f"branch {b}")
                for strat in ("default", "union"):
                    git(d, "checkout", "-q", "-B", "work", base)
                    if strat == "union":
                        write_dir(d, {".gitattributes": attrs})
                        git(d, "add", "-A")
                        git(d, "commit", "-q", "-m", "attrs")
                    conflicts, ok_plans, aborted = 0, [], 0
                    for b, plan in enumerate(plans):
                        r = git(d, "merge", "-q", "--no-edit", f"b{b}", check=False)
                        if r.returncode:
                            conflicts += 1
                            git(d, "merge", "--abort", check=False)
                            aborted += sum(1 for o in plan if not o["sup"])
                        else:
                            ok_plans.append(plan)
                    M, res = evaluate(fmt, read_dir(d), ok_plans)
                    e = err_counts(M)
                    lost = sum(1 for v in res.values() if not v)
                    invalid = any(e.values())
                    c = cell.setdefault((fmt, nb, strat), dict(n=0, conf=0, confT=0, inv=0, dup=0, two=0, brk=0, lost=0, lostT=0, ab=0, clean=0))
                    c["n"] += 1
                    c["conf"] += conflicts
                    c["confT"] += conflicts > 0
                    c["inv"] += invalid
                    c["dup"] += e["dup"] > 0
                    c["two"] += e["twocol"] > 0
                    c["brk"] += e["broken"] > 0
                    c["lost"] += lost
                    c["lostT"] += lost > 0
                    c["ab"] += aborted
                    c["clean"] += conflicts == 0 and not invalid and lost == 0
            print(f"S2 {fmt} nb={nb} done", flush=True)
    for nb in (2, 4):
        out += [f"### {nb} branches x 5 ops, merged sequentially into main ({trials} trials per cell)", "",
                "| format | merge | trials with conflict | conflicts/trial | ops aborted with conflicted branch/trial | silently lost ops/trial | trials with silent loss | trials invalid | dup ids | two columns | broken/dangling | fully clean |",
                "|---|---|---|---|---|---|---|---|---|---|---|---|"]
        for fmt in "ABC":
            for strat in ("default", "union"):
                c = cell[fmt, nb, strat]
                n, pc = c["n"], lambda x: f"{100 * x / n:.1f}%"
                label = strat if strat == "default" else "union (" + ("board.md + cards/*.md" if fmt == "B" else "board.md") + ")"
                out.append(f"| {fmt} | {label} | {pc(c['confT'])} | {c['conf'] / n:.2f} | {c['ab'] / n:.2f} | {c['lost'] / n:.2f} | "
                           f"{pc(c['lostT'])} | {pc(c['inv'])} | {pc(c['dup'])} | {pc(c['two'])} | {pc(c['brk'])} | {pc(c['clean'])} |")
        out.append("")
    return out


def main():
    a = sys.argv[1:]
    arg = lambda k, d: int(a[a.index(k) + 1]) if k in a else d
    s1, s2 = arg("--s1-trials", 20), arg("--s2-trials", 200)
    only = a[a.index("--only") + 1] if "--only" in a else None
    out = ["# Collaboration simulation", "",
           "Generated by `collab_sim.py` (deterministic seeds). Writers own disjoint cards, so every missing effect is a lost update",
           "(an op later overridden by the same writer is excluded). A lost op can cascade: a later op that anchors on it may also fail.",
           "In format B, concurrent inserts into the same gap can pick equal `order` values; ties sort by id, so \"X above Z\" can silently flip (counted as lost).", ""]
    if only != "s2":
        out += ["## S1: shared directory", "",
                "Ops are interleaved at read-file / write-file granularity. naive = overwrite whole file from the stale read; "
                "CAS = re-read the file about to be written and retry the op if it changed; string-replace = apply the exact "
                "old->new snippet to the CURRENT file, abort the op (no retry) if the snippet is gone or ambiguous. "
                "`silently lost` = no error reported but the effect is missing at the end.", ""]
        out += run_s1(s1)
    if only != "s1":
        out += ["## S2: git branch per agent", "",
                "Each branch applies 5 ops serially and commits once; main merges b0..bN sequentially (`git merge`). A conflicting merge is "
                "aborted (that branch's ops are dropped and counted as aborted, not as silent loss). `union` = `.gitattributes` merge=union "
                "committed on main first. Validity is checked with the format parser.", ""]
        tmp = tempfile.mkdtemp(prefix="markban-s2-")
        try:
            out += run_s2(s2, tmp)
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
    path = os.path.join(HERE, "results", "collab.md" if not only else f"collab-{only}.md")
    with open(path, "w") as fh:
        fh.write("\n".join(out) + "\n")
    print("wrote", path)


if __name__ == "__main__":
    main()
