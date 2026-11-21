"""Shared model, renderers and tolerant parsers for the markban format experiments.

Model: {"title", "columns": [{"id","name"}], "cards": [{"id","title","column","desc"}], "errors": [...]}
"column" on a card is the column id (for formats A and C the id is the column name).
Cards are listed grouped by column (in column order), in board order within a column.
"""
import os
import random
import re
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
BOARD_TITLE = "Acme sprint board"
COLUMNS = ["Backlog", "Ready", "Doing", "Review", "Done"]
FORMATS = ("A", "B", "C")
GIT_ID = ["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
          "-c", "commit.gpgsign=false"]

TITLES = {
    "Backlog": ["Old prototype", "Dark mode", "Rate limiter", "Search autocomplete", "Email digest",
                "Audit log", "Bulk user import", "Mobile app shell", "Feature flags UI",
                "Webhook retry dashboard", "Localization pipeline", "Admin impersonation",
                "Saved filters", "Onboarding checklist"],
    "Ready": ["Payment webhooks", "Password reset emails", "Session cleanup job",
              "Pagination for invoices", "Health check endpoint", "Upgrade Postgres client",
              "Fix timezone drift", "Cache product catalog"],
    "Doing": ["Fix login timeout", "Two-factor authentication", "Refactor billing module",
              "Add request tracing", "Migrate CI to new runners", "Improve error pages"],
    "Review": ["Rotate API keys", "Fix N+1 in orders list", "Add CSV import validation",
               "Update privacy policy page", "Harden CORS config"],
    "Done": ["Set up staging environment", "Add Sentry integration", "Fix broken avatar upload",
             "Upgrade to Node 22", "Document REST API", "Add database backups",
             "Remove unused dependencies"],
}

POOL = [
    "Users have reported problems with {t} in the current release.",
    "Scope: keep the change small and behind a feature flag if possible.",
    "See the design notes in the wiki before starting.",
    "- Add unit tests for the happy path",
    "- Update the runbook",
    "- Confirm behaviour with the support team",
    "Acceptance: works in staging with production-like data.",
    "Related to the {t} discussion from last planning.",
    "Estimated at 2 days; owner to confirm.",
    "Do not break existing API clients.",
    "Log enough context to debug {t} in production.",
    "Check the metrics dashboard before and after the rollout.",
]

LEGACY = ("Legacy importer", 3,
          "Imports legacy CSV exports from the old CRM.\n"
          "Runs as a nightly batch job on the shared worker.\n"
          "- Keep the column mapping table in sync\n"
          "Needs a rewrite before it can be trusted again.")


def slug(s):
    return re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")


def build_model(legacy=False):
    rng = random.Random(20240601)
    cols = [{"id": slug(n), "name": n} for n in COLUMNS]
    cards = []
    for col in cols:
        titles = list(TITLES[col["name"]])
        if legacy and col["name"] == "Backlog":
            titles.insert(LEGACY[1], LEGACY[0])
        for t in titles:
            if legacy and t == LEGACY[0]:
                desc = LEGACY[2]
            else:
                lines = rng.sample(POOL, rng.randint(2, 6))
                desc = "\n".join(l.format(t=t.lower()) for l in lines)
            cards.append({"id": slug(t), "title": t, "column": col["id"], "desc": desc})
    return {"title": BOARD_TITLE, "columns": cols, "cards": cards, "errors": []}


def cards_in(model, col_id):
    return [c for c in model["cards"] if c["column"] == col_id]


# ---------------------------------------------------------------- renderers

def render(fmt, m):
    return {"A": render_a, "B": render_b, "C": render_c}[fmt](m)


def render_a(m):
    out = ["---", "markban: 0.1", "---", "", "# " + m["title"], ""]
    for col in m["columns"]:
        out += ["## " + col["name"], ""]
        out += [f"- [{c['title']}](cards/{c['id']}.md)" for c in cards_in(m, col["id"])]
        out.append("")
    fs = {"board.md": "\n".join(out).rstrip("\n") + "\n"}
    for c in m["cards"]:
        fs[f"cards/{c['id']}.md"] = c["desc"] + "\n"
    return fs


def render_b(m):
    out = ["---", "markban: 0.1", "title: " + m["title"], "columns:"]
    for col in m["columns"]:
        out += [f"  - id: {col['id']}", f"    name: {col['name']}"]
    out.append("---")
    fs = {"board.md": "\n".join(out) + "\n"}
    for col in m["columns"]:
        for n, c in enumerate(cards_in(m, col["id"])):
            fs[f"cards/{c['id']}.md"] = (
                f"---\ntitle: {c['title']}\ncolumn: {col['id']}\norder: {(n + 1) * 1000}\n---\n{c['desc']}\n")
    return fs


def render_c(m):
    out = ["---", "markban: 0.1", "---", "", "# " + m["title"], ""]
    for col in m["columns"]:
        out += ["## " + col["name"], ""]
        for c in cards_in(m, col["id"]):
            out += [f"### {c['title']}", f"<!-- id: {c['id']} -->", "", c["desc"], ""]
    return {"board.md": "\n".join(out).rstrip("\n") + "\n"}


# ------------------------------------------------------------------ parsers

def split_fm(text):
    """Return (frontmatter_lines, body) or (None, text)."""
    lines = text.split("\n")
    if lines[0].rstrip() != "---":
        return None, text
    for i in range(1, len(lines)):
        if lines[i].rstrip() == "---":
            return lines[1:i], "\n".join(lines[i + 1:])
    return None, text


def parse_fm(lines, errors):
    d, cur, key = {}, None, None
    for ln in lines:
        if not ln.strip():
            continue
        m = re.match(r"^([A-Za-z_]\w*):\s*(.*?)\s*$", ln)
        if m:
            key, val = m.groups()
            if key in d:
                errors.append(("broken", f"duplicate key {key}"))
            d[key] = val if val != "" else []
            continue
        m = re.match(r"^\s+-\s+(\w+):\s*(.*?)\s*$", ln)
        if m and isinstance(d.get(key), list):
            cur = {m.group(1): m.group(2)}
            d[key].append(cur)
            continue
        m = re.match(r"^\s+(\w+):\s*(.*?)\s*$", ln)
        if m and cur is not None and isinstance(d.get(key), list):
            cur[m.group(1)] = m.group(2)
            continue
        errors.append(("broken", f"bad frontmatter line {ln!r}"))
    return d


def _head(fs, err):
    b = fs.get("board.md")
    if b is None:
        err.append(("broken", "no board.md"))
        return None, None
    fm, body = split_fm(b)
    if fm is None:
        err.append(("broken", "no frontmatter in board.md"))
        return {}, b
    d = parse_fm(fm, err)
    if d.get("markban") != "0.1":
        err.append(("broken", "markban version missing or not 0.1"))
    return d, body


def _dups(seen, err):
    for cid, cols in seen.items():
        if len(cols) > 1:
            err.append(("twocol" if len(set(cols)) > 1 else "dup", cid))


CARD_RE = re.compile(r"^- \[(.+)\]\(cards/([^)/]+)\.md\)$")
IDC_RE = re.compile(r"^<!--\s*id:\s*([a-z0-9-]+)\s*-->$")


def parse_a(fs):
    err = []
    m = {"title": None, "columns": [], "cards": [], "errors": err}
    _, body = _head(fs, err)
    if body is None:
        return m
    col, seen, names = None, {}, set()
    for ln in body.split("\n"):
        s = ln.rstrip()
        if not s:
            continue
        if s.startswith("## "):
            name = s[3:].strip()
            if name in names:
                err.append(("broken", f"duplicate column {name}"))
            names.add(name)
            col = {"id": name, "name": name}
            m["columns"].append(col)
        elif s.startswith("# "):
            m["title"] = s[2:].strip()
        else:
            mm = CARD_RE.match(s)
            if mm and col:
                title, cid = mm.groups()
                seen.setdefault(cid, []).append(col["id"])
                if len(seen[cid]) == 1:
                    path = f"cards/{cid}.md"
                    if path not in fs:
                        err.append(("broken", f"missing card file {cid}"))
                    m["cards"].append({"id": cid, "title": title, "column": col["id"],
                                       "desc": fs.get(path, "").strip()})
            else:
                err.append(("broken", f"unexpected line {s!r}"))
    _dups(seen, err)
    for p in fs:
        if p.startswith("cards/") and p.endswith(".md") and p[6:-3] not in seen:
            err.append(("broken", f"orphan card file {p}"))
    return m


def parse_b(fs):
    err = []
    m = {"title": None, "columns": [], "cards": [], "errors": err}
    d, _ = _head(fs, err)
    if d is None:
        return m
    m["title"] = d.get("title")
    cols = d.get("columns")
    if not isinstance(cols, list) or not cols:
        err.append(("broken", "no columns"))
        cols = []
    for c in cols:
        if "id" in c and "name" in c:
            m["columns"].append({"id": c["id"], "name": c["name"]})
        else:
            err.append(("broken", "column without id/name"))
    ids = [c["id"] for c in m["columns"]]
    if len(set(ids)) != len(ids):
        err.append(("broken", "duplicate column id"))
    bycol = {i: [] for i in ids}
    for p in sorted(fs):
        if not (p.startswith("cards/") and p.endswith(".md")):
            continue
        cid = p[6:-3]
        fm, body = split_fm(fs[p])
        if fm is None:
            err.append(("broken", f"no frontmatter in {p}"))
            continue
        e0 = len(err)
        cd = parse_fm(fm, err)
        try:
            order = int(cd["order"])
            title, col = cd["title"], cd["column"]
            if not isinstance(title, str) or not isinstance(col, str) or not title:
                raise KeyError
        except (KeyError, ValueError):
            if len(err) == e0:
                err.append(("broken", f"missing/invalid title, column or order in {p}"))
            continue
        if col not in bycol:
            err.append(("broken", f"unknown column {col} in {p}"))
            continue
        bycol[col].append((order, cid, {"id": cid, "title": title, "column": col, "desc": body.strip()}))
    for i in ids:
        m["cards"] += [c for _, _, c in sorted(bycol[i], key=lambda t: (t[0], t[1]))]
    return m


def parse_c(fs):
    err = []
    m = {"title": None, "columns": [], "cards": [], "errors": err}
    _, body = _head(fs, err)
    if body is None:
        return m
    col, card, seen, names = None, None, {}, set()

    def close():
        if card is not None and card.get("id") is not None:
            card["desc"] = "\n".join(card.pop("lines")).strip()
            seen.setdefault(card["id"], []).append(card["column"])
            if len(seen[card["id"]]) == 1:
                m["cards"].append(card)

    for ln in body.split("\n"):
        s = ln.rstrip()
        if s.startswith("### "):
            close()
            card = None
            if col is None:
                err.append(("broken", "card outside column"))
                continue
            card = {"id": None, "title": s[4:].strip(), "column": col["id"], "lines": []}
        elif s.startswith("## "):
            close()
            card = None
            name = s[3:].strip()
            if name in names:
                err.append(("broken", f"duplicate column {name}"))
            names.add(name)
            col = {"id": name, "name": name}
            m["columns"].append(col)
        elif s.startswith("# ") and col is None:
            m["title"] = s[2:].strip()
        elif card is not None:
            if card["id"] is None:
                if not s:
                    continue
                mm = IDC_RE.match(s)
                if mm:
                    card["id"] = mm.group(1)
                else:
                    err.append(("broken", f"card {card['title']!r} lacks id comment"))
                    card = None
            else:
                card["lines"].append(ln)
        elif s:
            err.append(("broken", f"unexpected line {s!r}"))
    close()
    _dups(seen, err)
    return m


def parse(fmt, fs):
    return {"A": parse_a, "B": parse_b, "C": parse_c}[fmt](fs)


# ----------------------------------------------------------------- file I/O

def read_dir(path):
    fs = {}
    for root, dirs, files in os.walk(path):
        dirs[:] = [d for d in dirs if d != ".git"]
        for f in files:
            full = os.path.join(root, f)
            with open(full, encoding="utf-8") as fh:
                fs[os.path.relpath(full, path)] = fh.read()
    return fs


def write_dir(path, fs):
    for p, t in fs.items():
        full = os.path.join(path, p)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w", encoding="utf-8") as fh:
            fh.write(t)


def sync_dir(path, before, after):
    """Apply the difference between two fs dicts to a directory."""
    for p in before:
        if p not in after:
            os.remove(os.path.join(path, p))
    write_dir(path, {p: t for p, t in after.items() if before.get(p) != t})


def git(d, *args, check=True):
    return subprocess.run(["git", "-C", d, *GIT_ID, *args], check=check,
                          capture_output=True, text=True)


def init_repo(d, msg="Initial board"):
    git(d, "init", "-q", "-b", "main")
    git(d, "add", "-A")
    git(d, "commit", "-q", "-m", msg)
