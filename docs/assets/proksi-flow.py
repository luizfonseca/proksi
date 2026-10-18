"""Writes docs/assets/proksi-flow-{light,dark}.svg, requests going through proksi, animated:
`python3 docs/assets/proksi-flow.py docs/assets`"""
import sys

CYCLE = 9.0  # seconds

THEMES = {
    "light": dict(
        text="#1f2328", muted="#59636e", line="#d1d9e0", box="#ffffff", box_line="#d1d9e0",
        proksi="#ecfdf5", proksi_line="#10b981", row="#ffffff", row_line="#a7f3d0",
        accent="#059669", request="#10b981", response="#6366f1", cache="#f59e0b",
    ),
    "dark": dict(
        text="#e6edf3", muted="#9198a1", line="#3d444d", box="#151b23", box_line="#3d444d",
        proksi="#0b2a22", proksi_line="#10b981", row="#0f1f1a", row_line="#1f5f4a",
        accent="#34d399", request="#34d399", response="#818cf8", cache="#fbbf24",
    ),
}

CLIENTS = [("browser", 130), ("mobile app", 210), ("curl", 290)]
UPSTREAMS = [("app:3000", 100), ("api-1:4000", 180), ("api-2:4000", 240), ("10.0.0.12:8080", 320)]
ROWS = ["TLS · Let's Encrypt", "routing · host &amp; path", "rules · plugins", "cache"]
PROKSI_IN, PROKSI_OUT, MID = (350, 210), (610, 210), 210

# (client, upstream or None for a cache hit, start in seconds)
REQUESTS = [(0, 0, 0.0), (1, 1, 1.4), (2, None, 2.8), (0, 2, 4.2), (1, 3, 5.6), (2, 1, 7.0)]
LEG = 0.7  # seconds per path


def client_path(i):
    _, y = CLIENTS[i]
    x0, (x1, y1) = 200, PROKSI_IN
    return f"M{x0},{y} C{x0 + 80},{y} {x1 - 80},{y1} {x1},{y1}"


def upstream_path(i):
    _, y = UPSTREAMS[i]
    (x0, y0), x1 = PROKSI_OUT, 740
    return f"M{x0},{y0} C{x0 + 70},{y0} {x1 - 70},{y} {x1},{y}"


def fraction(t):
    return f"{(t % CYCLE) / CYCLE:.4f}"


def trip(path, start, color, reverse=False):
    """A dot along `path`, from `start` for one leg of the cycle"""
    end = start + LEG
    a, b = fraction(start), fraction(end)
    assert float(a) < float(b), (path, start)
    points = "1;1;0;0" if reverse else "0;0;1;1"
    return f"""
    <circle class="packet" r="5" fill="{color}" opacity="0">
      <animateMotion dur="{CYCLE}s" repeatCount="indefinite" calcMode="linear"
        keyPoints="{points}" keyTimes="0;{a};{b};1"><mpath href="#{path}"/></animateMotion>
      <animate attributeName="opacity" dur="{CYCLE}s" repeatCount="indefinite" calcMode="discrete"
        values="0;1;0" keyTimes="0;{a};{b}"/>
    </circle>"""


def flash(starts, length=0.5):
    """`values`/`keyTimes` lighting a row from each of `starts` for `length`, fading in and out
    (one animation: several on the same attribute would override each other)"""
    points = [(0.0, 0)]
    for start in sorted(s % CYCLE for s in starts):
        a, b = start / CYCLE, (start + length) / CYCLE
        assert a > points[-1][0] and b < 1, start
        points += [(a, 0), ((2 * a + b) / 3, 1), (b, 0)]
    points.append((1.0, 0))
    values = ";".join(str(v) for _, v in points)
    times = ";".join(f"{t:.4f}" for t, _ in points)
    return f'values="{values}" keyTimes="{times}"'


def svg(t):
    out = [f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 960 420" width="960" height="420" role="img" aria-labelledby="title desc">
  <title id="title">Requests going through Proksi</title>
  <desc id="desc">Clients send requests to Proksi, which handles TLS, chooses a route by host and path, applies rules and plugins, answers from its cache or proxies to upstreams, balancing between them.</desc>
  <style>
    text {{ font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 14px; fill: {t['text']}; }}
    .label {{ font-family: system-ui, -apple-system, "Segoe UI", sans-serif; font-size: 13px; fill: {t['muted']}; letter-spacing: .06em; text-transform: uppercase; }}
    .title {{ font-family: system-ui, -apple-system, "Segoe UI", sans-serif; font-size: 22px; font-weight: 700; fill: {t['accent']}; }}
    .wire {{ fill: none; stroke: {t['line']}; stroke-width: 2; stroke-dasharray: 6 6; animation: flow 1.2s linear infinite; }}
    @keyframes flow {{ to {{ stroke-dashoffset: -12; }} }}
    @media (prefers-reduced-motion: reduce) {{ .packet, .glow {{ display: none; }} .wire {{ animation: none; }} }}
  </style>
  <defs>"""]
    for i in range(len(CLIENTS)):
        out.append(f'    <path id="c{i}" d="{client_path(i)}"/>')
    for i in range(len(UPSTREAMS)):
        out.append(f'    <path id="u{i}" d="{upstream_path(i)}"/>')
    out.append("  </defs>")

    out.append('  <text class="label" x="125" y="80" text-anchor="middle">Clients</text>')
    out.append('  <text class="label" x="835" y="60" text-anchor="middle">Upstreams</text>')
    for i in range(len(CLIENTS)):
        out.append(f'  <use href="#c{i}" class="wire"/>')
    for i in range(len(UPSTREAMS)):
        out.append(f'  <use href="#u{i}" class="wire"/>')

    for name, y in CLIENTS:
        out.append(f'  <rect x="50" y="{y - 22}" width="150" height="44" rx="10" fill="{t["box"]}" stroke="{t["box_line"]}" stroke-width="1.5"/>')
        out.append(f'  <text x="125" y="{y + 5}" text-anchor="middle">{name}</text>')
    for name, y in UPSTREAMS:
        out.append(f'  <rect x="740" y="{y - 22}" width="190" height="44" rx="10" fill="{t["box"]}" stroke="{t["box_line"]}" stroke-width="1.5"/>')
        out.append(f'  <text x="835" y="{y + 5}" text-anchor="middle">{name}</text>')
    out.append(f'  <path d="M932,158 q14,0 14,22 v60 q0,22 -14,22" fill="none" stroke="{t["muted"]}" stroke-width="1.5"/>')
    out.append(f'  <text class="label" x="952" y="214" text-anchor="middle" transform="rotate(90 952 210)" style="font-size:11px">balanced</text>')

    # proksi
    out.append(f'  <rect x="350" y="50" width="260" height="320" rx="18" fill="{t["proksi"]}" stroke="{t["proksi_line"]}" stroke-width="2"/>')
    out.append(f'  <text class="title" x="480" y="92" text-anchor="middle">Proksi</text>')
    for r, name in enumerate(ROWS):
        y = 120 + r * 58
        out.append(f'  <rect x="372" y="{y}" width="216" height="42" rx="9" fill="{t["row"]}" stroke="{t["row_line"]}" stroke-width="1.5"/>')
        color = t["cache"] if name == "cache" else t["accent"]
        out.append(f'  <rect class="glow" x="372" y="{y}" width="216" height="42" rx="9" fill="none" stroke="{color}" stroke-width="2.5" opacity="0">')
        if name == "cache":
            starts = [s + LEG + 0.36 for c, u, s in REQUESTS if u is None]
        else:
            starts = [s + LEG + 0.12 * r for c, u, s in REQUESTS]
        out.append(f'    <animate attributeName="opacity" dur="{CYCLE}s" repeatCount="indefinite" calcMode="linear" {flash(starts, 0.45)}/>')
        out.append("  </rect>")
        out.append(f'  <text x="480" y="{y + 26}" text-anchor="middle">{name}</text>')

    # requests and their responses
    for c, u, s in REQUESTS:
        out.append(trip(f"c{c}", s, t["request"]))
        if u is None:
            out.append(trip(f"c{c}", s + LEG + 0.5, t["cache"], reverse=True))
        else:
            out.append(trip(f"u{u}", s + LEG + 0.35, t["request"]))
            out.append(trip(f"u{u}", s + 2 * LEG + 0.6, t["response"], reverse=True))
            out.append(trip(f"c{c}", s + 3 * LEG + 0.6, t["response"], reverse=True))

    # legend
    for k, (color, name) in enumerate([(t["request"], "request"), (t["response"], "response"), (t["cache"], "from the cache")]):
        x = 360 + k * 120
        out.append(f'  <circle cx="{x}" cy="400" r="5" fill="{color}"/>')
        out.append(f'  <text class="label" x="{x + 12}" y="404" style="text-transform:none;letter-spacing:0">{name}</text>')
    out.append("</svg>\n")
    return "\n".join(out)


for name, theme in THEMES.items():
    with open(f"{sys.argv[1]}/proksi-flow-{name}.svg", "w") as f:
        f.write(svg(theme))
