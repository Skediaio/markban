"""Per-format operations as minimal text edits.

An op is a dict {"op": place|add|rename|desc|remove, ...}:
  place:  id, col (column index), above (id or None = end of column)   (move/reorder)
  add:    id, title, desc, col                                         (appended at end of column)
  rename: col, cur, new
  desc:   id, line                                                     (append a line to the description)
  remove: id

edits(fmt, view, op) returns a list of (path, old, new) computed from `view` (path -> text|None):
  old None  -> create file `path` with `new`
  new None  -> delete file `path` (old = content seen)
  otherwise -> replace the exact, unique snippet `old` by `new` in the file
"""
import re

from common import COLUMNS, slug

B_COLS = [slug(c) for c in COLUMNS]


def uniq_tail(text, whole):
    t = text.rstrip("\n")
    lines = t.split("\n")
    for k in range(1, len(lines) + 1):
        s = "\n".join(lines[-k:])
        if whole.count(s) == 1:
            return s
    return t


def card_line(cid, title):
    return f"- [{title}](cards/{cid}.md)"


# ------------------------------------------------------------------- needs

def needs(fmt, op, fs):
    k, cp = op["op"], f"cards/{op.get('id')}.md"
    if fmt == "A":
        return {"place": ["board.md"], "rename": ["board.md"], "add": ["board.md", cp],
                "remove": ["board.md", cp], "desc": [cp]}[k]
    if fmt == "B":
        if k in ("place", "add"):
            ps = sorted(p for p in fs if p.startswith("cards/") and p.endswith(".md"))
            return ps if cp in fs else ps + [cp]
        return ["board.md"] if k == "rename" else [cp]
    return ["board.md"]


# ----------------------------------------------------------------- format A

def a_find(b, cid):
    m = re.search(rf"^- \[.*\]\(cards/{re.escape(cid)}\.md\)$", b, re.M)
    return m.group(0) if m else None


def a_insert(b, col, x):
    lines = b.split("\n")
    heads = [i for i, l in enumerate(lines) if l.startswith("## ")]
    if col >= len(heads):
        return None
    s = heads[col]
    e = heads[col + 1] if col + 1 < len(heads) else len(lines)
    last = [l for l in lines[s + 1:e] if l.strip()]
    if last:
        return (last[-1] + "\n", last[-1] + "\n" + x + "\n")
    h = f"\n{lines[s]}\n"
    return (h, h + "\n" + x + "\n")


def a_place(view, op):
    b = view.get("board.md")
    x = a_find(b, op["id"]) if b else None
    if x is None:
        return []
    ed = [("board.md", x + "\n", "")]
    b2 = b.replace(x + "\n", "", 1)
    z = a_find(b2, op["above"]) if op["above"] else None
    if z:
        ed.append(("board.md", z + "\n", x + "\n" + z + "\n"))
    else:
        i = a_insert(b2, op["col"], x)
        if i is None:
            return []
        ed.append(("board.md",) + i)
    return ed


def a_add(view, op):
    cp, ed = f"cards/{op['id']}.md", []
    if view.get(cp) is None:
        ed.append((cp, None, op["desc"] + "\n"))
    b = view.get("board.md")
    if b is not None and a_find(b, op["id"]) is None:
        i = a_insert(b, op["col"], card_line(op["id"], op["title"]))
        if i:
            ed.append(("board.md",) + i)
    return ed


def a_remove(view, op):
    cp, ed = f"cards/{op['id']}.md", []
    b = view.get("board.md")
    x = a_find(b, op["id"]) if b else None
    if x:
        ed.append(("board.md", x + "\n", ""))
    if view.get(cp) is not None:
        ed.append((cp, view[cp], None))
    return ed


def tail_desc(view, op):
    cp = f"cards/{op['id']}.md"
    t = view.get(cp)
    if t is None:
        return []
    tail = uniq_tail(t, t)
    return [(cp, tail, tail + "\n" + op["line"])]


def rename_heading(view, op):
    if view.get("board.md") is None:
        return []
    return [("board.md", f"\n## {op['cur']}\n", f"\n## {op['new']}\n")]


# ----------------------------------------------------------------- format B

def b_cards(view):
    out = {}
    for p, t in view.items():
        if p.startswith("cards/") and t is not None:
            m = re.search(r"^column: (\S+)\norder: (-?\d+)\n", t, re.M)
            if m:
                out[p[6:-3]] = (m.group(1), int(m.group(2)), t)
    return out


def b_mk(cs, i, col, o):
    return (f"cards/{i}.md", f"column: {cs[i][0]}\norder: {cs[i][1]}\n", f"column: {col}\norder: {o}\n")


def b_slot(cs, x, col_id, above):
    """Return (sibling ids without x, insertion position, new order or None if no gap)."""
    sibs = sorted((o, i) for i, (c, o, _) in cs.items() if c == col_id and i != x)
    ids = [i for _, i in sibs]
    pos = ids.index(above) if above in ids else len(ids)
    prev = sibs[pos - 1][0] if pos > 0 else None
    nxt = sibs[pos][0] if pos < len(sibs) else None
    if prev is None and nxt is None:
        new = 1000
    elif prev is None:
        new = nxt - 1000
    elif nxt is None:
        new = prev + 1000
    else:
        new = (prev + nxt) // 2 if nxt - prev > 1 else None
    return ids, pos, new


def b_place(view, op):
    cs, x, col = b_cards(view), op["id"], B_COLS[op["col"]]
    if x not in cs:
        return []
    ids, pos, new = b_slot(cs, x, col, op["above"])
    if new is not None:
        return [] if (cs[x][0], cs[x][1]) == (col, new) else [b_mk(cs, x, col, new)]
    seq = ids[:pos] + [x] + ids[pos:]
    return [b_mk(cs, i, col, (n + 1) * 1000) for n, i in enumerate(seq)
            if (cs[i][0], cs[i][1]) != (col, (n + 1) * 1000)]


def b_add(view, op):
    cp = f"cards/{op['id']}.md"
    if view.get(cp) is not None:
        return []
    cs, col = b_cards(view), B_COLS[op["col"]]
    top = max([o for c, o, _ in cs.values() if c == col], default=0)
    return [(cp, None, f"---\ntitle: {op['title']}\ncolumn: {col}\norder: {top + 1000}\n---\n{op['desc']}\n")]


def b_remove(view, op):
    cp = f"cards/{op['id']}.md"
    return [(cp, view[cp], None)] if view.get(cp) is not None else []


def b_rename(view, op):
    if view.get("board.md") is None:
        return []
    return [("board.md", f"name: {op['cur']}\n", f"name: {op['new']}\n")]


# ----------------------------------------------------------------- format C

def c_block(t, cid):
    m = re.search(rf"^### (.*)\n<!-- id: {re.escape(cid)} -->$", t, re.M)
    if not m:
        return None
    n = re.compile(r"^#{2,3} ", re.M).search(t, m.end())
    return m.group(1), t[m.start():(n.start() if n else len(t))]


def c_insert(t, col, block):
    names = re.findall(r"^## (.*)$", t, re.M)
    if col >= len(names):
        return None
    bx = block.rstrip("\n") + "\n\n"
    if col + 1 < len(names):
        h = f"\n## {names[col + 1]}\n"
        return (h, "\n" + bx + h[1:])
    tail = uniq_tail(t, t)
    return (tail, tail + "\n\n" + bx.rstrip("\n"))


def c_place(view, op):
    t = view.get("board.md")
    blk = c_block(t, op["id"]) if t else None
    if blk is None:
        return []
    raw = blk[1]
    ed = [("board.md", raw, "")]
    t2 = t.replace(raw, "", 1)
    bx = raw.rstrip("\n") + "\n\n"
    z = c_block(t2, op["above"]) if op["above"] else None
    if z:
        old = f"### {z[0]}\n<!-- id: {op['above']} -->"
        ed.append(("board.md", old, bx + old))
    else:
        i = c_insert(t2, op["col"], raw)
        if i is None:
            return []
        ed.append(("board.md",) + i)
    return ed


def c_add(view, op):
    t = view.get("board.md")
    if t is None or c_block(t, op["id"]):
        return []
    i = c_insert(t, op["col"], f"### {op['title']}\n<!-- id: {op['id']} -->\n\n{op['desc']}\n\n")
    return [("board.md",) + i] if i else []


def c_remove(view, op):
    t = view.get("board.md")
    blk = c_block(t, op["id"]) if t else None
    return [("board.md", blk[1], "")] if blk else []


def c_desc(view, op):
    t = view.get("board.md")
    blk = c_block(t, op["id"]) if t else None
    if blk is None:
        return []
    tail = uniq_tail(blk[1], t)
    return [("board.md", tail, tail + "\n" + op["line"])]


EDITS = {
    "A": {"place": a_place, "add": a_add, "remove": a_remove, "desc": tail_desc, "rename": rename_heading},
    "B": {"place": b_place, "add": b_add, "remove": b_remove, "desc": tail_desc, "rename": b_rename},
    "C": {"place": c_place, "add": c_add, "remove": c_remove, "desc": c_desc, "rename": rename_heading},
}


def edits(fmt, view, op):
    return EDITS[fmt][op["op"]](view, op)


# ---------------------------------------------------------------- applying

def snapshot_results(view, es):
    """Apply edits to the snapshot; return [(path, new_text|None)] (naive whole-file writes)."""
    res, order = {}, []
    for p, old, new in es:
        if p not in res:
            res[p] = view.get(p)
            order.append(p)
        cur = res[p]
        if old is None:
            res[p] = new
        elif new is None:
            res[p] = None
        elif cur is not None and old in cur:
            res[p] = cur.replace(old, new, 1)
    return [(p, res[p]) for p in order]


def apply_live(fs, e):
    """Apply one edit to the live fs like an Edit tool: False if the target is gone/ambiguous."""
    p, old, new = e
    if old is None:
        fs[p] = new
        return True
    if p not in fs:
        return False
    if new is None:
        del fs[p]
        return True
    if fs[p].count(old) != 1:
        return False
    fs[p] = fs[p].replace(old, new, 1)
    return True


def run_op(fmt, fs, op):
    """Serially apply an op to an in-memory fs (mutates fs); returns the edits."""
    view = {p: fs.get(p) for p in needs(fmt, op, fs)}
    es = edits(fmt, view, op)
    for p, t in snapshot_results(view, es):
        if t is None:
            fs.pop(p, None)
        else:
            fs[p] = t
    return es


def task_ops(model):
    done_first = [c for c in model["cards"] if c["column"] == "done"][0]["id"]
    return {
        2: {"op": "place", "id": "fix-login-timeout", "col": 4, "above": done_first},
        3: {"op": "add", "id": "export-to-csv", "title": "Export to CSV",
            "desc": "Let users download the current table as CSV.", "col": 0},
        4: {"op": "rename", "col": 3, "cur": "Review", "new": "QA"},
        5: {"op": "place", "id": "audit-log", "col": 0, "above": "rate-limiter"},
        6: {"op": "remove", "id": "old-prototype"},
        7: {"op": "desc", "id": "payment-webhooks", "line": "Blocked on provider sandbox access."},
    }
