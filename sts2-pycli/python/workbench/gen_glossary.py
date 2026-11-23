"""Generate the workbench's hover-tip glossary, one table per game build.

The real game explains a term wherever it is named: an event option that says
"Enchant a card with **Adroit 3**" shows Adroit's tip on hover, and an
enchanted card in the deck carries its enchantment's tab and text. The
workbench names these terms but had nothing to say about them (GH #1599,
#1600). This module turns the build's own localization into the table the
page reads for both:

``enchantments``
    ``{UPPER_SNAKE: {title, description, extra, show_amount, image}}`` —
    ``description`` is the hover tip, ``extra`` the line an enchanted card
    adds to its own text (``extraCardText``), both still templates
    (``{Amount}``, ``{Block:diff()}``…) that the page fills with the
    enchantment's amount. ``image`` is the icon under the art root.
``keywords``
    The card keywords (Exhaust, Retain, Innate…): ``{UPPER_SNAKE: {title,
    description}}``.
``tips``
    The game's static hover tips (Block, Channel, Card Reward…) whose text
    needs no run context.
``upgrades``
    ``{CARD_ID: {description, cost}}``: each upgradable card's face once
    upgraded, as the workbench draws an unupgraded deck card (the plain or
    template branch of ``enrich_card``, keyword lines included, markup kept),
    for the page's "View upgrades" toggle (GH #1598).
``relics`` / ``potions``
    ``{UPPER_SNAKE: {title, description, image}}``, the description resolved
    as the workbench describes an owned one (``plain_owned_description``).

The default build's table is ``web/public/glossary.json``; every other
supported build's is ``web/public/<ver>/glossary.json``
(``versionedPublicPath`` on the page), like ``card_text.json``.

``--lang zhs`` writes the same tables in the game's Simplified Chinese
(``localization_zhs/``, one set for every build) to
``web/public/zh/[<ver>/]glossary.json``, which the page reads when it runs
in Chinese (``localizedPublicPath``). Each text falls back to the English one
where the Chinese table lacks the key or the Chinese template cannot be
filled, so the zh table has exactly the English one's entries.

Usage::

    PYTHONPATH=python uv run python -m workbench.gen_glossary [--version VER] [--lang zhs]
"""

from __future__ import annotations

import argparse
import json
import re
from collections.abc import Callable
from pathlib import Path
from typing import Any

from sts2_client import LEGACY_GAME_VERSION, SUPPORTED_GAME_VERSIONS

from workbench import render
from workbench.localization import (
    build_workbench_loc,
    card_var_value,
    keyword_card_lines,
    load_owned_localization,
    loc_dir_for,
    plain_description,
    plain_owned_description,
)

_PYCLI = Path(__file__).resolve().parent.parent.parent
_PUBLIC = _PYCLI / "web" / "public"
_WIKI_DATA = _PYCLI.parent / "sts2-wiki" / "data"
# The game's Simplified Chinese tables (``scripts/vendor_localization.py``,
# from the newest build): one set for every build.
ZH_LOC_DIR = _PYCLI / "localization_zhs"
# ``eng``: the build's English (``loc_dir_for``); ``zhs``: Simplified Chinese.
LANGS = ("eng", "zhs")

# A static tip whose text reads run context (``{BossName}``, ``{Arrows}``) has
# nothing to say on its own; the page cannot fill it.
_CONTEXT_VAR = re.compile(r"\{[A-Za-z]+(?::[^{}]*)?\}")


def public_dir_for(lang: str = "eng") -> Path:
    """``web/public/`` for English, ``web/public/zh/`` for Chinese (the
    page's ``localizedPublicPath``)."""
    if lang not in LANGS:
        raise ValueError(f"unsupported lang {lang!r}; supported: {', '.join(LANGS)}")
    return _PUBLIC if lang == "eng" else _PUBLIC / "zh"


def out_path_for(version: str | None = None, lang: str = "eng") -> Path:
    """``web/public/[zh/]glossary.json`` for the default build, else
    ``web/public/[zh/]<ver>/glossary.json``."""
    key = version or LEGACY_GAME_VERSION
    base = public_dir_for(lang)
    if key == LEGACY_GAME_VERSION:
        return base / "glossary.json"
    return base / key / "glossary.json"


def _load(loc_dir: Path, name: str) -> dict[str, str]:
    path = loc_dir / f"{name}.json"
    if not path.is_file():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def zh_table(name: str, version: str | None = None) -> dict[str, str]:
    """The Chinese ``name`` table for build ``version``.

    The shared set (``localization_zhs/``) comes from one build, named in
    its ``BUILD.txt``. A key whose ENGLISH text differs between ``version``
    and that build describes a different effect there (a card reworked
    between builds): its Chinese is dropped, so the English stays (wrong
    Chinese is worse). The build's own set (``localization_zhs_<ver>/``,
    vendored from its pck) then wins key by key. The Rust twin is
    ``sts2-workbench`` ``data::zh_tables_for``.
    """
    shared = _load(ZH_LOC_DIR, name)
    version = version or LEGACY_GAME_VERSION  # a file naming no build
    build_txt = ZH_LOC_DIR / "BUILD.txt"
    lines = build_txt.read_text(encoding="utf-8").splitlines() if build_txt.is_file() else []
    src = lines[0].strip() if lines else ""
    if src and src != version and src in SUPPORTED_GAME_VERSIONS:
        mine, theirs = _load(loc_dir_for(version), name), _load(loc_dir_for(src), name)
        shared = {
            k: v
            for k, v in shared.items()
            if not (k in mine and k in theirs and mine[k] != theirs[k])
        }
    return {**shared, **_load(_PYCLI / f"localization_zhs_{version}", name)}


def _load_lang(loc_dir: Path, name: str, lang: str, version: str | None = None) -> dict[str, str]:
    """The build's ``name`` table in ``lang``: every key the English one has,
    the Chinese text where the build's Chinese table (``zh_table``) has a
    non-empty one."""
    eng = _load(loc_dir, name)
    if lang == "eng":
        return eng
    zh = zh_table(name, version)
    return {k: zh.get(k) or v for k, v in eng.items()}


# ``{Energy:energyIcons()}`` / ``{Stars:starIcons()}``: icon runs whose count
# is a stat ``required_stat_vars`` leaves out.
_ICON_VAR = re.compile(r"\{(\w+):(?:energyIcons|starIcons)\(\)\}", re.IGNORECASE)
_ENERGY_RUN = re.compile(r"\[energy\](\d+)\[/energy\]")
_STAR_RUN = re.compile(r"\[star\](\d+)\[/star\]")
# The unit-only glyph after a literal cost ("一张0{energyPrefix…}复制品").
_ENERGY_PREFIX = re.compile(r"\{energyPrefix:energyIcons\(\d*\)\}")


# ``{HitCount:choose(2):两次|…}``: the var and the value its first branch is for.
_CHOOSE_FIRST = re.compile(r"\{(\w+):choose\(([^)|]*)")


def infer_vars(template: str, plain: str) -> dict[str, str]:
    """``{var_lower: value}`` read off an English text: ``template``'s simple
    placeholders (``{X}``, ``{X:diff()}``, ``{X:choose(N):word|{X…}unit}``)
    matched by position against the resolved ``plain`` (markup stripped).
    A count the wiki printed as a literal ``X`` reads ``X``. Empty when the
    shapes disagree. Twin of ``sts2-workbench`` ``localization::infer_vars``."""
    strip = lambda t: re.sub(r"\[[^\]]*\]", "", t)  # noqa: E731
    t, p = strip(template), strip(plain)
    slots: list[tuple[str, str | None, tuple[str, str, str] | None]] = []
    rest = t
    while (i := rest.find("{")) >= 0:
        depth, end = 0, None
        for k in range(i, len(rest)):
            if rest[k] == "{":
                depth += 1
            elif rest[k] == "}":
                depth -= 1
                if depth == 0:
                    end = k
                    break
        if end is None:
            return {}
        inner = rest[i + 1 : end]
        name, _, fmt = inner.partition(":")
        if not name or not name.isalnum():
            return {}
        choose = None
        if fmt.startswith("plural:") and "{" not in fmt:
            # `{HitCount:plural:time|times}`: a word, no value; match either.
            slots.append((rest[:i], None, ("plural", *fmt[len("plural:") :].split("|", 1))))
            rest = rest[end + 1 :]
            continue
        if fmt and fmt != "diff()":
            m = re.fullmatch(r"choose\(([^)]*)\):([^|{]*)\|(\{.*\})(.*)", fmt, re.S)
            if not m:
                return {}
            choose = (m.group(1), m.group(2), m.group(4))
        slots.append((rest[:i], name.lower(), choose))
        rest = rest[end + 1 :]
    slots.append((rest, None, None))
    out: dict[str, str] = {}
    at = p
    for lit, var, choose in slots:
        if not at.startswith(lit):
            return {}
        at = at[len(lit) :]
        if var is None and choose and choose[0] == "plural":
            words = sorted(choose[1:], key=len, reverse=True)
            word = next((w for w in words if at.startswith(w)), None)
            if word is None:
                return {}
            at = at[len(word) :]
            continue
        if var is None:
            continue
        if choose and choose[1] and at.startswith(choose[1]):
            at = at[len(choose[1]) :]
            out[var] = choose[0]
            continue
        m = re.match(r"-?\d+|X", at)
        if not m:
            return {}
        at = at[m.end() :]
        if choose:
            if not at.startswith(choose[2]):
                return {}
            at = at[len(choose[2]) :]
        out[var] = m.group(0)
    return out


def english_unknown(text: str | None) -> bool:
    """True when an English text leaves a value unknown: a raw ``{Var}`` or
    the wiki's literal ``X`` count."""
    return bool(text) and ("{" in text or re.search(r"(?<![\w])X(?![\w])", text) is not None)


def zh_render(
    template: str | None,
    value: Callable[[str], Any],
    *,
    upgraded: bool = False,
    icons: bool = True,
    english: str | None = None,
    english_plain: str | None = None,
    unknown_as_x: bool = False,
) -> str | None:
    """A Chinese template filled with ``value(var)`` for each var it reads,
    markup kept, or None when one has no value (the caller keeps the
    English text then, never a half-filled one).

    ``icons``: keep the energy / star runs as ``[energy]N[/energy]`` (the card
    face draws them); False spells them out ("2点能量"), for text drawn by
    plain rich text, as the English plain text spells "2 Energy".

    ``english``: the build's English template. A ``choose`` var it does not
    read and the build has no value for takes the chooser's first branch:
    the Chinese table is the newest build's, and an older build that printed
    "hits twice" outright has no ``HitCount`` to choose by.

    ``english_plain``: that template resolved (the wiki's plain text). A var
    the build lists no value for is read back off it by position
    (``infer_vars``; the Rust twin is ``localization::infer_vars``). An icon
    run with no count reads just the resource ("能量"), as the English plain
    text does; a ``choose`` on a non-number selector with no value takes its
    last (default) branch.

    ``unknown_as_x``: the English text itself leaves the value unknown (a raw
    ``{SelfStrength}`` or the wiki's ``X``): print a missing one as ``X``,
    the game's own mark for an unknown count, rather than give up.
    """
    if not template:
        return None
    if not icons:
        template = _ENERGY_PREFIX.sub("能量", template)
    # Single player, out of combat: the multiplayer branches
    # ("{IsMultiplayer:所有人|}") and targeting previews are off.
    flags = {"ismultiplayer": False, "istargeting": False}
    needed = render.required_stat_vars(
        template, in_combat=False, upgraded=upgraded, extra_flags=flags
    )
    needed |= {m.lower() for m in _ICON_VAR.findall(template)}
    stats: dict[str, Any] = {}
    first = {m.group(1).lower(): m.group(2) for m in _CHOOSE_FIRST.finditer(template)}
    english_vars = {m.lower() for m in re.findall(r"\{(\w+)", english or "")}
    inferred = infer_vars(english, english_plain) if english and english_plain else {}
    icon_vars = {m.lower() for m in _ICON_VAR.findall(template)}
    for var in needed:
        v = value(var)
        if v is None:
            v = inferred.get(var)
        if v is None and english is not None and var in first and var not in english_vars:
            v = first[var]
        if v is None and var in icon_vars:
            # No count: the run reads just the resource, like the English.
            template = re.sub(
                r"\{" + var + r":energyIcons\(\)\}", "能量", template, flags=re.IGNORECASE
            )
            template = re.sub(
                r"\{" + var + r":starIcons\(\)\}", "辉星", template, flags=re.IGNORECASE
            )
            continue
        if v is None and var in first and not first[var].lstrip("-").isdigit():
            v = ""  # a non-number selector with no value: the default branch
        if v is None and unknown_as_x:
            v = "X"
        if v is None:
            return None
        stats[var] = int(v) if isinstance(v, str) and v.lstrip("-").isdigit() else v
    try:
        out = render.resolve_card_text(
            template, stats, upgraded=upgraded, extra_flags=flags, keep_markup=True
        )
    except (TypeError, ValueError):
        return None  # a formatter that needs a number got the `X` mark
    if not icons:
        # "额外获得1点{Energy:energyIcons()}": the text already says the number.
        out = re.sub(r"(\d+)点\[energy\]\1\[/energy\]", r"\1点能量", out)
        out = _ENERGY_RUN.sub(r"\1点能量", out)
        out = _STAR_RUN.sub(r"\1点辉星", out)
    return out


def _snake(class_name: str) -> str:
    """``PerfectFit`` -> ``PERFECT_FIT`` (the loc key stem)."""
    return re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", class_name).upper()


def _show_amount(version: str) -> dict[str, bool]:
    """Which enchantments draw their amount (C# ``ShowAmount``), from the
    build's wiki data; a missing key is the base class's ``false``."""
    path = _WIKI_DATA / version / "enchantments.json"
    if not path.is_file():
        # Older builds' wiki data predates the field; the newest one's flags
        # describe the same classes.
        candidates = sorted(_WIKI_DATA.glob("*/enchantments.json"))
        if not candidates:
            return {}
        path = candidates[-1]
    rows = json.loads(path.read_text(encoding="utf-8"))
    return {_snake(r["class_name"]): bool(r.get("show_amount")) for r in rows}


def _pairs(loc: dict[str, str]) -> dict[str, dict[str, str]]:
    """``{KEY.title, KEY.description, KEY.extraCardText}`` -> ``{KEY: {...}}``."""
    out: dict[str, dict[str, str]] = {}
    for full, text in loc.items():
        key, _, field = full.partition(".")
        if field in ("title", "description", "extraCardText"):
            out.setdefault(key, {})[field] = text
    return out


def build(version: str, lang: str = "eng") -> dict[str, Any]:
    """The glossary for ``version`` in ``lang``, from its vendored
    localization (``zhs``: the Chinese tables laid over it key by key)."""
    public_dir_for(lang)  # validate
    loc_dir = loc_dir_for(version)
    show = _show_amount(version)
    enchantments: dict[str, dict[str, Any]] = {}
    for key, e in sorted(_pairs(_load_lang(loc_dir, "enchantments", lang, version)).items()):
        if "title" not in e or key.startswith(("MOCK_", "DEPRECATED_")):
            continue
        enchantments[key] = {
            "title": e["title"],
            "description": e.get("description", ""),
            "extra": e.get("extraCardText"),
            "show_amount": show.get(key, False),
            "image": f"enchantments/{key.lower()}.png",
        }
    keywords = {
        key: {"title": e["title"], "description": e.get("description", "")}
        for key, e in sorted(_pairs(_load_lang(loc_dir, "card_keywords", lang, version)).items())
        if "title" in e
    }
    tips = {
        key: {"title": e["title"], "description": e.get("description", "")}
        for key, e in sorted(_pairs(_load(loc_dir, "static_hover_tips")).items())
        if "title" in e
        and not _CONTEXT_VAR.search(e["title"])
        and not _CONTEXT_VAR.search(e.get("description", ""))
    }
    if lang == "zhs":
        # The English table picks the tips; a Chinese text that reads run
        # context the English one does not keeps the English.
        zh_tips = _pairs(_load_lang(loc_dir, "static_hover_tips", lang, version))
        for key, tip in tips.items():
            z = zh_tips.get(key, {})
            reads_context = lambda d: any(  # noqa: E731
                re.search(r"\{[A-Za-z]+", d.get(f, "") or "") for f in ("title", "description")
            )
            # ...unless the English reads it too (then neither is filled).
            if "title" in z and (not reads_context(z) or reads_context(tip)):
                tip["title"], tip["description"] = z["title"], z.get("description", "")
    owned_loc = load_owned_localization(_WIKI_DATA, version=version)
    owned = {kind: _owned(kind, atlas, version, owned_loc, lang) for kind, atlas in _OWNED.items()}
    return {
        "enchantments": enchantments,
        "keywords": keywords,
        "tips": tips,
        "upgrades": _upgrades(version, lang),
        **owned,
    }


def zh_card_loc(
    loc: dict[str, str], version: str | None = None
) -> tuple[dict[str, str], dict[str, str]]:
    """``(zh card templates by card id, loc with zh keyword titles)``: what
    the Chinese card faces read. ``loc`` is the build's English map."""
    zh_cards = zh_table("cards", version)
    templates = {
        k[: -len(".description")]: v
        for k, v in zh_cards.items()
        if k.endswith(".description") and v
    }
    kw = zh_table("card_keywords", version)
    zloc = dict(loc)
    for key in [k for k in loc if k.startswith("card_keyword.")]:
        title = kw.get(f"{key[len('card_keyword.') :].upper()}.title")
        if title:
            zloc[key] = title
    return templates, zloc


def zh_keyword_lines(cid: str, upgraded: bool, zloc: dict[str, str]) -> tuple[list[str], list[str]]:
    """``keyword_card_lines`` with the Chinese titles and full stop."""
    period = _load(ZH_LOC_DIR, "card_keywords").get("PERIOD") or "。"
    before, after = keyword_card_lines(cid, upgraded, zloc)
    fix = lambda x: x[:-1] + period if x.endswith(".") else x  # noqa: E731
    return [fix(x) for x in before], [fix(x) for x in after]


def _upgrades(version: str, lang: str = "eng") -> dict[str, dict[str, Any]]:
    """Every card's upgraded face, as ``enrich_card`` draws a deck card."""
    loc, _ = build_workbench_loc(_WIKI_DATA, version=version)
    ids = sorted({k.rsplit(".", 1)[0] for k in loc if k.endswith(".upgraded_description_plain")})
    zh_templates, zloc = zh_card_loc(loc, version) if lang == "zhs" else ({}, loc)
    out: dict[str, dict[str, Any]] = {}
    for cid in ids:
        template = loc.get(f"{cid}.description")
        stats = _upgraded_stats(cid, template, loc)
        if stats is not None:
            body = render.resolve_card_text(template, stats, upgraded=True, keep_markup=True)
        else:
            plain = plain_description(cid, True, loc) or ""
            body = render.resolve_card_text(plain, None, keep_markup=True)
        before, after = keyword_card_lines(cid, True, loc)
        zh_body: str | None = None
        if lang == "zhs" and not template and not zh_templates.get(cid):
            zh_body = ""  # keyword lines only, in both languages
        elif lang == "zhs":
            zh_body = zh_render(
                zh_templates.get(cid),
                lambda var, cid=cid: card_var_value(cid, var, True, loc),
                upgraded=True,
                english=template or "",
                english_plain=plain_description(cid, True, loc) or None,
                unknown_as_x=english_unknown(plain_description(cid, True, loc)),
            )
        if zh_body is not None:
            body = zh_body
            before, after = zh_keyword_lines(cid, True, zloc)
        lines = [render.resolve_card_text(x, None, keep_markup=True) for x in before]
        lines += [body] if body else []
        lines += [render.resolve_card_text(x, None, keep_markup=True) for x in after]
        entry: dict[str, Any] = {"description": "\n".join(lines)}
        cost = loc.get(f"{cid}.upgraded_cost")
        if cost is not None:
            entry["cost"] = int(cost) if cost.lstrip("-").isdigit() else cost
        out[cid] = entry
    return out


def _upgraded_stats(cid: str, template: str | None, loc: dict[str, str]) -> dict[str, Any] | None:
    """The upgraded values of every var ``template`` reads, from the manifest,
    or None when one has none (a ``Calculated*`` var, an unknown one): then
    the card's pre-resolved plain text is the face. Filling the template keeps
    the game's ``[gold]`` keyword markup, which the plain text has lost."""
    # ``{Energy:energyIcons()}`` / ``starIcons()`` draw icons the renderer
    # (and ``{singleStarIcon}``) leaves out; the plain text spells them
    # ("2 Energy", "★").
    if not template or "Icons(" in template or "Icon}" in template:
        return None
    stats: dict[str, Any] = {}
    for var in render.required_stat_vars(template, in_combat=False, upgraded=True):
        if var.startswith("calculated"):
            return None
        value = card_var_value(cid, var, True, loc)
        if value is None:
            return None
        stats[var] = int(value) if value.lstrip("-").isdigit() else value
    return stats


def _row_values(row: dict[str, Any]) -> dict[str, Any]:
    """A wiki relic / potion row's base values by lowercased var name, also
    under ``<name>power`` (the template's ``{VigorPower}`` is the wiki's
    ``Vigor``), as ``bundle_workbench_data._owned_vars`` hands the Rust zh
    overlay."""
    out: dict[str, Any] = {}
    for v in row.get("vars") or []:
        if not isinstance(v, dict) or not v.get("type"):
            continue
        base = v.get("base_value")
        if isinstance(base, bool) or not isinstance(base, (int, float)):
            continue
        name = str(v["type"]).lower()
        out.setdefault(name, base)
        out.setdefault(f"{name}power", base)
    return out


# The owned kinds and their art atlas.
_OWNED = {"relics": "relic_atlas", "potions": "potion_atlas"}


def _owned(
    kind: str, atlas: str, version: str, loc: dict[str, str], lang: str = "eng"
) -> dict[str, dict[str, str]]:
    """``kind``'s rows from the build's wiki data, described in plain text
    (``zhs``: the Chinese title, and the Chinese template filled with the
    row's base values)."""
    path = _WIKI_DATA / version / f"{kind}.json"
    if not path.is_file():
        return {}
    zh = zh_table(kind, version) if lang == "zhs" else {}
    out: dict[str, dict[str, str]] = {}
    for row in json.loads(path.read_text(encoding="utf-8")):
        key, title = row.get("loc_key"), row.get("title")
        if not key or not title or key.startswith(("MOCK_", "DEPRECATED_")):
            continue
        desc = plain_owned_description(key, loc) or row.get("description") or ""
        if zh:
            title = zh.get(f"{key}.title") or title
            values = _row_values(row)
            desc = (
                zh_render(
                    zh.get(f"{key}.description"),
                    values.get,
                    icons=False,
                    english=loc.get(f"{key}.description") or row.get("description"),
                    english_plain=desc,
                    unknown_as_x=english_unknown(desc),
                )
                or desc
            )
        out[key] = {
            "title": title,
            "description": desc,
            "image": f"{atlas}/{row.get('image') or key.lower()}.png",
        }
    return dict(sorted(out.items()))


def generate(version: str, lang: str = "eng") -> Path:
    """Write ``version``'s glossary in ``lang``; returns the path written."""
    out = out_path_for(version, lang)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(
        json.dumps(build(version, lang), indent=1, ensure_ascii=False, sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    return out


def main(argv: list[str] | None = None) -> None:
    ap = argparse.ArgumentParser(description="Generate the workbench hover-tip glossary.")
    ap.add_argument("--version", choices=SUPPORTED_GAME_VERSIONS, help="one build (default: all)")
    ap.add_argument(
        "--lang", choices=LANGS, default="eng", help="eng (default) or zhs (Simplified Chinese)"
    )
    args = ap.parse_args(argv)
    versions = [args.version] if args.version else list(SUPPORTED_GAME_VERSIONS)
    if args.lang == "zhs" and not ZH_LOC_DIR.is_dir():
        print(f"skip zhs: no {ZH_LOC_DIR.name}/")
        return
    for v in versions:
        if not loc_dir_for(v).is_dir():
            print(f"skip {v}: no vendored localization")
            continue
        print(generate(v, args.lang))


if __name__ == "__main__":
    main()
