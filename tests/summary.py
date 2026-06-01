#!/usr/bin/env python3

import json
import subprocess
import sys
from pathlib import Path


WORKDIR = Path(".")
FLAKE = Path("examples")
OTEL_VALUE_KEYS = ("stringValue", "intValue", "doubleValue", "boolValue")


def otel_attrs(items):
    attrs = {}
    for item in items:
        value = item.get("value", {})
        for key in OTEL_VALUE_KEYS:
            if key in value:
                attrs[item["key"]] = value[key]
                break
    return attrs


def load_spans(path):
    if not path.exists():
        return []

    text = path.read_text(encoding="utf-8", errors="replace")
    spans = []
    decoder = json.JSONDecoder()
    index = 0
    while index < len(text):
        while index < len(text) and text[index].isspace():
            index += 1
        if index >= len(text):
            break
        export, index = decoder.raw_decode(text, index)
        for resource_span in export.get("resourceSpans", []):
            resource_attrs = otel_attrs(resource_span.get("resource", {}).get("attributes", []))
            service = resource_attrs.get("service.name", "unknown")
            for scope_span in resource_span.get("scopeSpans", []):
                for span in scope_span.get("spans", []):
                    spans.append({
                        "name": span.get("name", ""),
                        "service": service,
                        "status": span.get("status", {}).get("code", "OK"),
                        "attrs": otel_attrs(span.get("attributes", [])),
                        "start": int(span.get("startTimeUnixNano", 0)) // 1_000_000,
                        "end": int(span.get("endTimeUnixNano", 0)) // 1_000_000,
                    })
    return spans


def mermaid_text(value):
    return str(value or "unknown").replace(":", "-").replace(",", ";").replace("\n", " ")


def add_trace_diagram(lines, spans):
    if not spans:
        lines.append("_No spans were exported._")
        return

    lines += [
        "```mermaid",
        "gantt",
        "    title Nixie E2E Trace",
        "    dateFormat x",
        "    axisFormat %H:%M:%S",
        "    todayMarker off",
    ]
    task = 0
    for service in sorted({span["service"] or "unknown" for span in spans}):
        lines.append(f"    section {mermaid_text(service)}")
        service_spans = sorted((span for span in spans if (span["service"] or "unknown") == service), key=lambda span: (span["start"], span["name"]))
        for span in service_spans:
            task += 1
            status = span["status"] or "OK"
            ok = status in ("OK", "STATUS_CODE_UNSET")
            label = mermaid_text(" ".join(
                [span["name"]] + [f"{key}={value}" for key, value in span["attrs"].items()]
            ))
            if not ok:
                label = f"{label} [{mermaid_text(status)}]"
            duration = max(1, span["end"] - span["start"])
            lines.append(f"    {label} :{'done' if ok else 'crit'}, span{task}, {span['start']}, {duration}ms")
    lines.append("```")


def hosts_diff(flake):
    result = subprocess.run(
        ["git", "-C", str(flake), "diff", "--no-ext-diff", "--", "hosts.json"],
        capture_output=True,
        text=True,
    )
    return result.stdout.splitlines()


def build_summary(result):
    lines = ["## Nixie E2E Test", "", f"**{result.upper()}**", "", "```diff"]
    lines += hosts_diff(FLAKE)
    lines += ["```", ""]
    try:
        add_trace_diagram(lines, load_spans(WORKDIR / "trace.json"))
    except Exception as err:
        lines.append(f"_Failed to render trace summary: {err}_")
    return "\n".join(lines) + "\n"


def main():
    result = sys.argv[1] if len(sys.argv) > 1 else "unknown"
    print(build_summary(result), end="")


if __name__ == "__main__":
    main()
