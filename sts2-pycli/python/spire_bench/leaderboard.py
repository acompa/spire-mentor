"""The static leaderboard of a season, built only from verified transcripts.

    just bench-leaderboard season/reveal.json site/ transcripts/*.jsonl   # from sts2-pycli/

It replays every transcript (``spire_bench.verify``), then scores each agent
pooled over every episode it started. An agent is scored only when all of its
transcripts verify and they are exactly episodes 0..n-1, where n is the count
the reveal gives for it (from the gateway's index). A missing transcript could
be a lost run left out, so the count is the reveal's claim, not the highest
number present; each agent checks that claim against its own receipts. Agents
that fail are listed with the reason instead of a score.

Writes ``leaderboard.json``, ``index.html``, the commitment and reveal, and a
copy of every transcript, so a reader can check a receipt or replay a row.
"""

from __future__ import annotations

import argparse
import asyncio
import html
import json
import shutil
from collections import defaultdict
from pathlib import Path
from typing import Any

from spire_bench.scoring import score
from spire_bench.verify import season_args, verify_all


def judge(
    results: list[dict[str, Any]], started: dict[str, int]
) -> tuple[list[dict[str, Any]], dict[str, list[str]]]:
    """``(score rows, {agent: reasons it is not scored})`` from verify results
    and the reveal's episode count per agent."""
    by_agent: dict[str, list[dict[str, Any]]] = defaultdict(list)
    unowned = []
    for r in results:
        if r.get("agent_id") is None:
            unowned.append(r)
        else:
            by_agent[r["agent_id"]].append(r)
    flagged: dict[str, list[str]] = {}
    records = []
    for agent in sorted(set(by_agent) | set(started)):
        rs = by_agent.get(agent, [])
        why = [f"episode {r['episode_no']}: {r['why']}" for r in rs if not r["ok"]]
        nos = {r["episode_no"] for r in rs}
        if agent not in started:
            why.append("the reveal does not count this agent's episodes")
        missing = sorted(set(range(started.get(agent, 0))) - nos)
        if missing:
            shown = ", ".join(map(str, missing[:10])) + (" ..." if len(missing) > 10 else "")
            why.append(f"{len(missing)} episode(s) missing: {shown}")
        extra = sorted(nos - set(range(started.get(agent, 0))))
        if extra and agent in started:
            why.append(f"{len(extra)} episode(s) beyond the reveal's count {started[agent]}")
        if why:
            flagged[agent] = why
            continue
        records += [{**r, "agent_id": agent} for r in rs]
    if unowned:
        flagged["(unreadable)"] = [f"{r.get('path')}: {r.get('why')}" for r in unowned]
    return score(records), flagged


def _pct(x: float | None) -> str:
    return "–" if x is None else f"{100 * x:.1f}%"


def render(board: dict[str, Any]) -> str:
    e = html.escape
    rows = "".join(
        "<tr>"
        f"<td>{e(r['agent_id'])}</td><td>{e(r['character'])}</td><td>A{r['ascension']}</td>"
        f"<td class=n>{r['won']}/{r['finished']}</td><td class=n><b>{_pct(r['win_rate'])}</b></td>"
        f"<td class=n>{_pct(r['wilson_95'][0])} – {_pct(r['wilson_95'][1])}</td>"
        f"<td class=n>{r['void']}</td></tr>"
        for r in board["scores"]
    )
    flagged = "".join(
        f"<li><b>{e(a)}</b><ul>{''.join(f'<li>{e(w)}</li>' for w in ws)}</ul></li>"
        for a, ws in board["not_scored"].items()
    )
    c = board["commitment"]
    return f"""<title>Spire Bench {e(c["season"])}</title>
<style>
:root {{ --bg:#fbfaf7; --fg:#1d1d1b; --mut:#6b6a66; --line:#e3e1db; --acc:#9a3412; }}
@media (prefers-color-scheme: dark) {{ :root:not([data-theme="light"]) {{
  --bg:#171614; --fg:#ecebe7; --mut:#a3a19b; --line:#34322e; --acc:#fb923c; color-scheme:dark; }} }}
:root[data-theme="dark"] {{ --bg:#171614; --fg:#ecebe7; --mut:#a3a19b; --line:#34322e;
  --acc:#fb923c; color-scheme:dark; }}
body {{ background:var(--bg); color:var(--fg); font:15px/1.5 system-ui,sans-serif;
  margin:0; padding:2rem 16px; }}
main {{ max-width:60rem; margin:0 auto; }}
h1 {{ margin:0 0 .25rem; }} .mut {{ color:var(--mut); }}
.wrap {{ overflow-x:auto; }} table {{ border-collapse:collapse; width:100%; margin:1.5rem 0; }}
th,td {{ padding:.45rem .6rem; border-bottom:1px solid var(--line); text-align:left; }}
th {{ font-weight:600; color:var(--mut); }} .n {{ text-align:right; font-variant-numeric:tabular-nums; }}
code {{ font-size:.85em; word-break:break-all; }} a {{ color:var(--acc); }}
</style>
<main>
<h1>Spire Bench — season {e(c["season"])}</h1>
<p class=mut>{e(c["game_version"])} · every episode an agent started counts ·
win rate with a 95% Wilson interval · void = the referee failed, not scored</p>
<div class=wrap><table>
<tr><th>Agent</th><th>Character</th><th>Asc.</th><th class=n>Won</th><th class=n>Win rate</th>
<th class=n>95% interval</th><th class=n>Void</th></tr>
{rows or "<tr><td colspan=7 class=mut>No agent is scored yet.</td></tr>"}
</table></div>
{f"<h2>Not scored</h2><ul>{flagged}</ul>" if flagged else ""}
<h2>Check it yourself</h2>
<p>Season id <code>{e(board["season_id"])}</code>. The <a href="commitment.json">commitment</a>
was published before drand round {c["drand"]["round"]}; the <a href="reveal.json">reveal</a>
gives its K and that round's randomness. Each transcript is under
<a href="transcripts/">transcripts/</a>; replay them with
<code>just bench-verify-season reveal.json transcripts/*.jsonl</code>, and compare the
fingerprint of your own episodes with the last receipt the gateway sent you, and
the number of episodes the reveal counts for you with the number you started.</p>
</main>
"""


def build(reveal_path: Path, out: Path, transcripts: list[Path]) -> dict[str, Any]:
    key, kw = season_args(reveal_path)
    results = asyncio.run(verify_all(list(transcripts), key, **kw))
    reveal = json.loads(reveal_path.read_text(encoding="utf-8"))
    if not isinstance(reveal.get("episodes"), dict):
        raise SystemExit("the reveal does not say how many episodes each agent started")
    rows, flagged = judge(results, {a: int(n) for a, n in reveal["episodes"].items()})
    board = {
        "season_id": kw["season_id"],
        "commitment": reveal["commitment"],
        "scores": rows,
        "not_scored": flagged,
        "episodes_started": reveal["episodes"],
        "episodes": [
            {k: r.get(k) for k in ("episode_id", "ok", "status", "reason", "fingerprint", "why")}
            for r in results
        ],
    }
    (out / "transcripts").mkdir(parents=True, exist_ok=True)
    for t in transcripts:
        shutil.copyfile(t, out / "transcripts" / t.name)
    shutil.copyfile(reveal_path, out / "reveal.json")
    (out / "commitment.json").write_text(
        json.dumps(reveal["commitment"], indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    (out / "leaderboard.json").write_text(json.dumps(board, indent=2) + "\n", encoding="utf-8")
    (out / "index.html").write_text(render(board), encoding="utf-8")
    return board


def main() -> int:
    ap = argparse.ArgumentParser(description="Build the Spire Bench leaderboard.")
    ap.add_argument("reveal")
    ap.add_argument("out_dir")
    ap.add_argument("transcripts", nargs="+")
    args = ap.parse_args()
    board = build(Path(args.reveal), Path(args.out_dir), [Path(t) for t in args.transcripts])
    print(f"{len(board['scores'])} rows, {len(board['not_scored'])} agent(s) not scored")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
