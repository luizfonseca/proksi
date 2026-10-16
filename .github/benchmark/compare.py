#!/usr/bin/env python3
"""Compares the performance of two proksi binaries (e.g. main and a pull request).

Each binary proxies the same load to the same upstream, in alternating rounds on the
same machine, for plain HTTP and HTTPS. The report (markdown) has, for each scenario,
the median over the rounds of: requests per second, latency (p50 and p99), CPU time
per 10k requests and peak memory (RSS) of the proksi process.

Uses only the standard library, and `oha` (https://github.com/hatoo/oha) for the load.

    compare.py --base ./proksi-main --head ./proksi-pr --oha ./oha \
        --upstream 127.0.0.1:8080 --out report.md
"""

import argparse
import json
import os
import platform
import signal
import socket
import ssl
import statistics
import subprocess
import tempfile
import threading
import time
import urllib.request
from pathlib import Path

MARKER = "<!-- proksi-benchmark -->"

# Relative changes considered regressions (GitHub runners are noisy)
THRESHOLD = 0.10

CONFIG = """
worker_threads = {workers}

server {{
  https_address = "127.0.0.1:{https_port}"
  http_address = "127.0.0.1:{http_port}"
}}

logging {{
  enabled = true
  level = "warn"
  access_logs_enabled = true
}}

lets_encrypt {{
  enabled = false
  email = "bench@proksi.test"
}}

paths {{
  lets_encrypt = "{dir}/le"
}}

host "localhost" {{
  listen = ["127.0.0.1:{https_port}"]
  tls {{
    certificate {{
      pem = "{dir}/cert.pem"
      key = "{dir}/key.pem"
    }}
  }}

  location "/" {{
    upstream "{upstream}" {{}}
  }}
}}

host "*" {{
  tls = false
  listen = ["127.0.0.1:{plain_port}"]

  location "/" {{
    upstream "{upstream}" {{}}
  }}
}}
"""

SCENARIOS = {
    "HTTP": "http://127.0.0.1:{plain_port}/",
    "HTTPS": "https://localhost:{https_port}/",
}


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def write_certificate(directory):
    subprocess.run(
        [
            "openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2",
            "-subj", "/CN=localhost",
            "-keyout", str(directory / "key.pem"),
            "-out", str(directory / "cert.pem"),
        ],
        check=True,
        capture_output=True,
    )


class Process:
    """A running proksi, with its CPU time and peak memory"""

    def __init__(self, binary, config_path):
        self.process = subprocess.Popen(
            [binary, "-c", str(config_path)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        self.peak_rss_kb = 0
        self._sampling = False

    def cpu_seconds(self):
        pid = self.process.pid
        if platform.system() == "Linux":
            fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
            ticks = os.sysconf("SC_CLK_TCK")
            # utime and stime (fields 14 and 15 of /proc/<pid>/stat)
            return (int(fields[11]) + int(fields[12])) / ticks
        out = subprocess.run(
            ["ps", "-o", "time=", "-p", str(pid)], capture_output=True, text=True
        ).stdout.strip()
        seconds = 0.0
        for part in out.split(":"):
            seconds = seconds * 60 + float(part)
        return seconds

    def rss_kb(self):
        out = subprocess.run(
            ["ps", "-o", "rss=", "-p", str(self.process.pid)], capture_output=True, text=True
        ).stdout.strip()
        return int(out or 0)

    def sample_memory(self):
        """Samples the RSS in the background until `stop_sampling`"""
        self._sampling = True

        def sample():
            while self._sampling:
                self.peak_rss_kb = max(self.peak_rss_kb, self.rss_kb())
                time.sleep(0.2)

        self._thread = threading.Thread(target=sample, daemon=True)
        self._thread.start()

    def stop_sampling(self):
        self._sampling = False
        self._thread.join()

    def stop(self):
        self.process.send_signal(signal.SIGINT)
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()


def wait_until_ready(urls, process, timeout=30):
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    deadline = time.time() + timeout
    while time.time() < deadline:
        if process.process.poll() is not None:
            raise RuntimeError("proksi exited before becoming ready")
        try:
            for url in urls:
                urllib.request.urlopen(url, timeout=2, context=context).read()
            return
        except OSError:
            time.sleep(0.2)
    raise RuntimeError("proksi did not become ready")


def load(oha, url, duration, connections):
    out = subprocess.run(
        [
            oha, url, "-z", duration, "-c", str(connections),
            # proksi listens on 127.0.0.1 (`localhost` may resolve to ::1 first)
            "--ipv4", "--no-tui", "--insecure", "--output-format", "json",
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(out)


def run_round(binary, args, directory):
    """One run of `binary` for every scenario"""
    ports = {
        "https_port": free_port(),
        "http_port": free_port(),
        "plain_port": free_port(),
    }
    config_path = directory / "proksi.hcl"
    config_path.write_text(
        CONFIG.format(dir=directory, upstream=args.upstream, workers=args.workers, **ports)
    )
    urls = {name: url.format(**ports) for name, url in SCENARIOS.items()}

    results = {}
    for name, url in urls.items():
        # A fresh process per scenario, so memory is measured per scenario
        process = Process(binary, config_path)
        try:
            wait_until_ready([url], process)
            process.sample_memory()
            load(args.oha, url, args.warmup, args.connections)
            cpu_before = process.cpu_seconds()
            summary = load(args.oha, url, args.duration, args.connections)
            cpu = process.cpu_seconds() - cpu_before
            process.stop_sampling()
            requests = sum(summary["statusCodeDistribution"].values())
            if not requests:
                raise RuntimeError(f"no responses from {url}: {summary['errorDistribution']}")
            results[name] = {
                "rps": summary["summary"]["requestsPerSec"],
                "p50_ms": summary["latencyPercentiles"]["p50"] * 1000,
                "p99_ms": summary["latencyPercentiles"]["p99"] * 1000,
                "cpu_ms_per_10k": cpu * 1000 / max(requests, 1) * 10_000,
                "peak_rss_mb": process.peak_rss_kb / 1024,
                "success": summary["summary"]["successRate"],
            }
        finally:
            process.stop()
    return results


def median(rounds, scenario, metric):
    return statistics.median(r[scenario][metric] for r in rounds)


# (metric, label, format, whether higher is better)
METRICS = [
    ("rps", "Requests/s", "{:,.0f}", True),
    ("p50_ms", "Latency p50", "{:.2f} ms", False),
    ("p99_ms", "Latency p99", "{:.2f} ms", False),
    ("cpu_ms_per_10k", "CPU per 10k requests", "{:.0f} ms", False),
    ("peak_rss_mb", "Peak memory", "{:.1f} MB", False),
]


def report(base_rounds, head_rounds, args):
    lines = [MARKER, "## Performance", ""]
    regressions = []
    for scenario in SCENARIOS:
        lines += [
            f"**{scenario}**",
            "",
            "| | main | this PR | change |",
            "| --- | ---: | ---: | ---: |",
        ]
        for metric, label, fmt, higher_is_better in METRICS:
            base = median(base_rounds, scenario, metric)
            head = median(head_rounds, scenario, metric)
            change = (head - base) / base if base else 0.0
            worse = -change if higher_is_better else change
            flag = " ⚠️" if worse > THRESHOLD else ""
            if flag:
                regressions.append(f"{scenario} {label.lower()}")
            lines.append(
                f"| {label} | {fmt.format(base)} | {fmt.format(head)} | {change:+.1%}{flag} |"
            )
        failures = [
            r[scenario]["success"] for r in base_rounds + head_rounds if r[scenario]["success"] < 1
        ]
        if failures:
            lines.append("")
            lines.append(f"Some requests failed (lowest success rate: {min(failures):.2%}).")
        lines.append("")

    summary = (
        f"⚠️ Possible regression: {', '.join(regressions)}."
        if regressions
        else "No change above ±10%."
    )
    lines.insert(3, summary + "\n")
    lines += [
        "<details><summary>How this is measured</summary>",
        "",
        f"Medians of {args.rounds} alternating rounds per binary, each with a {args.warmup} warmup "
        f"and {args.duration} of load at {args.connections} connections (`oha`), proxying to "
        f"nginx on the same runner. proksi runs with {args.workers} worker threads and access logs on. "
        "Both binaries are built the same way (release, thin LTO). "
        "GitHub runners are noisy: changes under ±10% are usually noise.",
        "",
        "</details>",
    ]
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True, help="proksi binary to compare against")
    parser.add_argument("--head", required=True, help="proksi binary being tested")
    parser.add_argument("--oha", default="oha")
    parser.add_argument("--upstream", required=True, help="upstream address (host:port)")
    parser.add_argument("--out", required=True)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--duration", default="15s")
    parser.add_argument("--warmup", default="3s")
    parser.add_argument("--connections", type=int, default=50)
    parser.add_argument("--workers", type=int, default=2)
    parser.add_argument("--raw", help="also write the measurements as JSON")
    args = parser.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        directory = Path(tmp)
        write_certificate(directory)
        base_rounds, head_rounds = [], []
        for round_index in range(args.rounds):
            # Alternate which binary goes first, so drift affects both equally
            order = [("base", args.base), ("head", args.head)]
            if round_index % 2:
                order.reverse()
            for side, binary in order:
                print(f"round {round_index + 1}/{args.rounds}: {side}", flush=True)
                results = run_round(binary, args, directory)
                (base_rounds if side == "base" else head_rounds).append(results)

    Path(args.out).write_text(report(base_rounds, head_rounds, args))
    if args.raw:
        Path(args.raw).write_text(json.dumps({"base": base_rounds, "head": head_rounds}, indent=2))
    print(Path(args.out).read_text())


if __name__ == "__main__":
    main()
