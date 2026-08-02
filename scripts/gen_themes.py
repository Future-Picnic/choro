#!/usr/bin/env python3
"""Generate the Choro theme JSON from token dicts (the source of truth).

Each theme is defined by ~19 design tokens; this maps them onto the ~106 keys
the gpui-component Theme expects. Regenerate with:  python3 scripts/gen_themes.py
Output: crates/ide-app/assets/themes/choro.json  (Choro Dark first = default).
"""
import json, pathlib

# ---- Choro palettes, as design tokens (matches choro-design-system.html) ----
PALETTES = [
    ("Choro Dark", dict(
        sink="#111013", nav="#151417", base="#19181C", surface="#1F1E23",
        surface2="#26252B", focus="#232128", line="#232228", line2="#322F38",
        t1="#EDE9EA", t2="#B9B2B4", t3="#8B8691", t4="#5B5763",
        acc="#CAC9EE", acc2="#D9D8F6", on_acc="#303049",
        amber="#E4B876", sage="#96C9A4", rose="#DE6E6E", sky="#85B8DF")),
    ("Choro Indigo", dict(
        sink="#1A1B21", nav="#1E1F26", base="#23242C", surface="#2A2B35",
        surface2="#333541", focus="#2F303B", line="#343640", line2="#464954",
        t1="#DDE3F2", t2="#AEB9D0", t3="#808CA3", t4="#566276",
        acc="#CAC9EE", acc2="#D9D8F6", on_acc="#303049",
        amber="#DDB374", sage="#91BE9B", rose="#D7777E", sky="#83ADCF")),
    ("Choro Twilight", dict(
        sink="#1B181C", nav="#201C21", base="#262126", surface="#2D272D",
        surface2="#352E36", focus="#312A32", line="#342D36", line2="#463C48",
        t1="#EFEAEC", t2="#C3B9BE", t3="#918794", t4="#67606B",
        acc="#CAC9EE", acc2="#D9D8F6", on_acc="#303049",
        amber="#E4B876", sage="#96C9A4", rose="#DE7278", sky="#85B8DF")),
    ("Choro Dusk", dict(
        sink="#262325", nav="#2B282A", base="#302D2F", surface="#383436",
        surface2="#423E40", focus="#3D393B", line="#474145", line2="#585055",
        t1="#F0ECEB", t2="#CDC4C3", t3="#9D9498", t4="#746C70",
        acc="#CAC9EE", acc2="#D9D8F6", on_acc="#303049",
        amber="#E4B876", sage="#96C9A4", rose="#DE777D", sky="#85B8DF")),
    ("Choro Light", dict(
        mode="light",
        sink="#E9E2E2", nav="#EFE8E7", base="#F6F1EF", surface="#FCF8F6",
        surface2="#ECE6EA", focus="#FFF9F7", line="#E2DADD", line2="#D3C8CE",
        t1="#2A252A", t2="#5D555B", t3="#81777F", t4="#AAA0A6",
        acc="#CAC9EE", acc2="#B9B7E3", on_acc="#303049",
        amber="#9C6428", sage="#367849", rose="#AD414D", sky="#3B719B",
        on_amber="#FFF9F7", on_sage="#FFF9F7", on_rose="#FFF9F7",
        on_sky="#FFF9F7", overlay="#00000066")),
    ("Amethyst", dict(
        sink="#101113", nav="#141518", base="#18191D", surface="#1E2025",
        surface2="#25272E", focus="#22242A", line="#222429", line2="#313440",
        t1="#E9EAEE", t2="#B2B4BC", t3="#7C7F8A", t4="#51545E",
        acc="#A48FE6", acc2="#BCA9F2", on_acc="#201843",
        amber="#E2C069", sage="#93C89F", rose="#E07E86", sky="#7FB5DE")),
    ("Ember", dict(
        sink="#101113", nav="#141518", base="#18191D", surface="#1E2025",
        surface2="#25272E", focus="#22242A", line="#222429", line2="#313440",
        t1="#E9EAEE", t2="#B2B4BC", t3="#7C7F8A", t4="#51545E",
        acc="#E8A075", acc2="#F5B78F", on_acc="#26150B",
        amber="#E2C069", sage="#93C89F", rose="#E07E86", sky="#7FB5DE")),
    ("Mint", dict(
        sink="#0D0D0F", nav="#111113", base="#151517", surface="#1B1B1E",
        surface2="#222226", focus="#1F1F22", line="#212125", line2="#2F2F35",
        t1="#EDEDEF", t2="#B3B3BA", t3="#7D7D86", t4="#515158",
        acc="#5FDBB4", acc2="#7FE9C8", on_acc="#08221A",
        amber="#E5B567", sage="#86D3A2", rose="#E07A7A", sky="#78B7DD")),
]

# Default dark inks that read on the desaturated dark-theme semantic fills.
# A palette may override them when its semantic colors are dark enough to need
# light contrast ink (as Choro Light does).
INK = dict(danger="#2A0D10", warning="#241A08", success="#08221A", info="#071722")


def a(hex6, alpha):  # append an 8-bit alpha to a #RRGGBB
    return hex6 + alpha


def colors(p):
    return {
        "background": p["base"],
        "foreground": p["t1"],
        "border": p["line"],
        "input.border": p["line2"],
        "ring": p["acc"],
        "caret": p["acc"],
        "muted.background": p["surface"],
        "muted.foreground": p["t3"],
        "accent.background": p["surface2"],
        "accent.foreground": p["t1"],
        "accordion.background": p["base"],
        "sidebar.background": p["nav"],
        "sidebar.foreground": p["t2"],
        "sidebar.border": p["line"],
        "sidebar.accent.background": p["surface2"],
        "sidebar.accent.foreground": p["t1"],
        "sidebar.primary.background": p["acc"],
        "sidebar.primary.foreground": p["on_acc"],
        "title_bar.background": p["sink"],
        "title_bar.border": p["line"],
        "tab_bar.background": p["nav"],
        "tab_bar.segmented.background": p["base"],
        "tab.background": a(p["base"], "00"),
        "tab.foreground": p["t3"],
        "tab.active.background": p["surface2"],
        "tab.active.foreground": p["t1"],
        "popover.background": p["focus"],
        "popover.foreground": p["t1"],
        "overlay": p.get("overlay", "#000000B3"),
        "window.border": p["line"],
        "drag_border": p["acc"],
        "drop_target.background": a(p["acc"], "33"),
        "primary.background": p["acc"],
        "primary.foreground": p["on_acc"],
        "primary.hover.background": p["acc2"],
        "primary.active.background": p["acc2"],
        "secondary.background": p["surface"],
        "secondary.foreground": p["t1"],
        "secondary.hover.background": p["surface2"],
        "secondary.active.background": p["line2"],
        "danger.background": p["rose"],
        "danger.foreground": p.get("on_rose", INK["danger"]),
        "danger.hover.background": a(p["rose"], "E6"),
        "danger.active.background": p["rose"],
        "warning.background": p["amber"],
        "warning.foreground": p.get("on_amber", INK["warning"]),
        "warning.hover.background": a(p["amber"], "E6"),
        "warning.active.background": p["amber"],
        "success.background": p["sage"],
        "success.foreground": p.get("on_sage", INK["success"]),
        "success.hover.background": a(p["sage"], "E6"),
        "success.active.background": p["sage"],
        "info.background": p["sky"],
        "info.foreground": p.get("on_sky", INK["info"]),
        "info.hover.background": a(p["sky"], "E6"),
        "info.active.background": p["sky"],
        "link.foreground": p["sky"],
        "link.hover.foreground": p["sky"],
        "link.active.foreground": p["sky"],
        "list.background": p["base"],
        "list.hover.background": p["surface2"],
        "list.active.background": a(p["acc"], "26"),
        "list.active.border": p["acc"],
        "list.even.background": p["nav"],
        "list.head.background": p["nav"],
        "table.background": p["base"],
        "table.hover.background": p["surface2"],
        "table.active.background": a(p["acc"], "26"),
        "table.active.border": p["acc"],
        "table.even.background": p["nav"],
        "table.head.background": p["nav"],
        "table.head.foreground": p["t3"],
        "table.row.border": p["line"],
        "selection.background": a(p["acc"], "4D"),
        "scrollbar.background": a(p["base"], "00"),
        "scrollbar.thumb.background": a(p["line2"], "B3"),
        "scrollbar.thumb.hover.background": p["line2"],
        "skeleton.background": p["surface2"],
        "switch.background": p["surface2"],
        "slider.bar.background": p["acc"],
        "slider.thumb.background": p["acc"],
        "progress_bar.background": p["acc"],
        "group_box.background": p["surface"],
        "group_box.foreground": p["t2"],
        "description_list_label.background": p["surface2"],
        "description_list_label.foreground": p["t2"],
        "tiles.background": p["nav"],
        "bullish.background": p["sage"],
        "bearish.background": p["rose"],
        "chart_1": p["acc"],
        "chart_2": p["acc2"],
        "chart_3": p["sage"],
        "chart_4": p["amber"],
        "chart_5": p["rose"],
        "base.blue": p["sky"],
        "base.blue.light": p["sky"],
        "base.cyan": p["sky"],
        "base.cyan.light": p["sky"],
        "base.green": p["sage"],
        "base.green.light": a(p["sage"], "CC"),
        "base.magenta": p["acc"],
        "base.magenta.light": p["acc2"],
        "base.red": p["rose"],
        "base.red.light": a(p["rose"], "CC"),
        "base.yellow": p["amber"],
        "base.yellow.light": a(p["amber"], "CC"),
    }


def syntax(color, *, style=None, weight=None):
    value = {"color": color}
    if style is not None:
        value["font_style"] = style
    if weight is not None:
        value["font_weight"] = weight
    return value


def highlight(p):
    """A restrained, readable editor palette derived from the Choro tokens.

    The component library's fallback highlighter uses a near-black canvas and
    highly saturated green/yellow syntax colors. That works for its standalone
    gallery but splits Choro's editor into two unrelated surfaces. Keeping the
    editor palette token-derived makes the gutter, canvas, selection, syntax,
    and every shipped app theme move together.
    """
    return {
        "editor.foreground": p["t1"],
        "editor.background": p["base"],
        "editor.active_line.background": a(p["surface2"], "66"),
        "editor.line_number": p["t4"],
        "editor.active_line_number": p["t2"],
        "created": p["sage"],
        "deleted": p["rose"],
        "error": p["rose"],
        "hint": p["acc"],
        "info": p["sky"],
        "modified": p["amber"],
        "predictive": p["t4"],
        "success": p["sage"],
        "warning": p["amber"],
        "syntax": {
            "attribute": syntax(p["amber"]),
            "boolean": syntax(p["amber"]),
            "comment": syntax(p["t3"], style="italic"),
            "comment.doc": syntax(p["t3"], style="italic"),
            "constant": syntax(p["amber"]),
            "constructor": syntax(p["sky"]),
            "embedded": syntax(p["t2"]),
            "emphasis": syntax(p["t2"], style="italic"),
            "emphasis.strong": syntax(p["t1"], weight=600),
            "enum": syntax(p["acc"]),
            "function": syntax(p["sky"]),
            "hint": syntax(p["t4"]),
            "keyword": syntax(p["acc"]),
            "label": syntax(p["sky"]),
            "link_text": syntax(p["sky"]),
            "link_uri": syntax(p["sky"], style="italic"),
            "number": syntax(p["amber"]),
            "operator": syntax(p["t2"]),
            "predictive": syntax(p["t4"]),
            "preproc": syntax(p["rose"]),
            "primary": syntax(p["t1"]),
            "property": syntax(p["t2"]),
            "punctuation": syntax(p["t3"]),
            "punctuation.bracket": syntax(p["t2"]),
            "punctuation.delimiter": syntax(p["t3"]),
            "punctuation.list_marker": syntax(p["acc"]),
            "punctuation.special": syntax(p["amber"]),
            "string": syntax(p["sage"]),
            "string.escape": syntax(p["amber"]),
            "string.regex": syntax(p["sage"]),
            "string.special": syntax(p["amber"]),
            "string.special.symbol": syntax(p["amber"]),
            "tag": syntax(p["rose"]),
            "tag.doctype": syntax(p["t3"]),
            "text.literal": syntax(p["t2"]),
            "title": syntax(p["sky"], weight=600),
            "type": syntax(p["acc"]),
            "variable": syntax(p["t1"]),
            "variable.special": syntax(p["rose"]),
            "variant": syntax(p["acc"]),
        },
    }


doc = {
    "name": "Choro",
    "author": "Ritmus — the Choro design system, generated from design tokens",
    "url": "https://ritmus.studio",
    "themes": [
        {
            "name": name,
            "mode": p.get("mode", "dark"),
            "colors": colors(p),
            "highlight": highlight(p),
        }
        for name, p in PALETTES
    ],
}

out = pathlib.Path(__file__).resolve().parent.parent / "crates/ide-app/assets/themes/choro.json"
n = len(doc["themes"][0]["colors"])
names = [theme["name"] for theme in doc["themes"]]
assert n == 106, f"expected 106 theme keys, got {n}"
assert len(names) == len(set(names)), "theme names must be unique"
assert all(len(theme["colors"]) == n for theme in doc["themes"])
assert all(len(theme["highlight"]["syntax"]) == 40 for theme in doc["themes"])
assert doc["themes"][0]["name"] == "Choro Dark"
assert doc["themes"][0]["mode"] == "dark"
assert doc["themes"][1]["name"] == "Choro Indigo"
assert doc["themes"][1]["mode"] == "dark"
assert doc["themes"][2]["name"] == "Choro Twilight"
assert doc["themes"][2]["mode"] == "dark"
assert doc["themes"][3]["name"] == "Choro Dusk"
assert doc["themes"][3]["mode"] == "dark"
assert doc["themes"][4]["name"] == "Choro Light"
assert doc["themes"][4]["mode"] == "light"
out.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
print(f"wrote {out}  ({len(doc['themes'])} themes, {n} keys each)")
