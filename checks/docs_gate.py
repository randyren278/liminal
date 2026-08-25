#!/usr/bin/env python3
"""Stop the documentation from rotting the moment it is written.

Three checks, no network, no node, binary result:

  links    every relative markdown link target resolves to a real file
  mermaid  every ```mermaid block is non-empty, names a known diagram type,
           draws something, and balances its brackets and quotes
  claims   every path-like string the docs point at exists on disk

The third is the one that matters for an autonomous run. A README that
describes a module which was renamed, or never wired up, is worse than no
README: it is a confident false statement about the system. Rendering is
GitHub's job; this catches the failures that survive review.

    docs_gate.py README.md docs/ARCHITECTURE.md --min-diagrams 2
"""
from __future__ import annotations

import argparse
import pathlib
import re
import sys

LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
FENCE = re.compile(r"^```mermaid\s*$(.*?)^```\s*$", re.MULTILINE | re.DOTALL)
CODE_SPAN = re.compile(r"`([^`\n]+)`")
DIAGRAM_TYPES = ("flowchart", "graph", "sequenceDiagram", "stateDiagram-v2", "stateDiagram",
                 "classDiagram", "erDiagram", "gantt", "journey", "pie", "mindmap",
                 "timeline", "gitGraph", "C4Context")
PAIRS = {"[": "]", "(": ")", "{": "}"}
# A code span only counts as a claimed path if it looks like one: has a
# directory separator or a file extension, and no shell/prose noise.
PATHLIKE = re.compile(r"^[\w.\-/]+\.\w{1,6}$|^[\w.\-]+/[\w.\-/]*$")


def check_links(path: pathlib.Path) -> list[str]:
    """Relative link targets must resolve. Anchors are not validated."""
    problems = []
    for target in LINK.findall(path.read_text()):
        target = target.strip()
        if target.startswith(("http://", "https://", "mailto:", "#")):
            continue
        file_part = target.split("#", 1)[0]
        if not file_part:
            continue
        resolved = (path.parent / file_part).resolve()
        if not resolved.exists():
            problems.append(f"{path}: link '{target}' -> {resolved} does not exist")
    return problems


def balanced(body: str) -> str | None:
    """Report the first unbalanced delimiter, ignoring anything inside quotes."""
    stack: list[str] = []
    in_quotes = False
    for character in body:
        if character == '"':
            in_quotes = not in_quotes
            continue
        if in_quotes:
            continue
        if character in PAIRS:
            stack.append(character)
        elif character in PAIRS.values():
            if not stack or PAIRS[stack.pop()] != character:
                return f"unbalanced {character!r}"
    if in_quotes:
        return "unterminated quote"
    if stack:
        return f"unclosed {stack[-1]!r}"
    return None


def check_mermaid(path: pathlib.Path, minimum: int) -> list[str]:
    blocks = FENCE.findall(path.read_text())
    problems = []
    if len(blocks) < minimum:
        problems.append(
            f"{path}: {len(blocks)} mermaid block(s), expected at least {minimum}")
    for index, body in enumerate(blocks, start=1):
        lines = [line for line in body.strip().splitlines() if line.strip()]
        if not lines:
            problems.append(f"{path} block {index}: empty")
            continue
        if not lines[0].strip().startswith(DIAGRAM_TYPES):
            problems.append(
                f"{path} block {index}: unknown diagram type {lines[0].strip()!r}")
        if len(lines) < 2:
            problems.append(f"{path} block {index}: declares a diagram but draws nothing")
        fault = balanced(body)
        if fault:
            problems.append(f"{path} block {index}: {fault}")
    return problems


def check_claims(path: pathlib.Path, root: pathlib.Path) -> list[str]:
    """Every path-shaped code span must name a file or directory that exists.

    This is what catches docs describing a module that was renamed away, or one
    that was written about but never built.
    """
    problems = []
    for span in CODE_SPAN.findall(path.read_text()):
        candidate = span.strip().rstrip("/")
        if not candidate or not PATHLIKE.match(candidate):
            continue
        if (root / candidate).exists() or (path.parent / candidate).exists():
            continue
        problems.append(
            f"{path}: documents `{span.strip()}`, which does not exist in the repo")
    return problems


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("files", nargs="+", type=pathlib.Path)
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path("."),
                        help="repo root that documented paths resolve against")
    parser.add_argument("--min-diagrams", type=int, default=0,
                        help="minimum mermaid blocks required per file")
    parser.add_argument("--skip-claims", action="store_true",
                        help="do not check that path-shaped code spans exist on disk")
    args = parser.parse_args(argv)

    root = args.root.resolve()
    problems: list[str] = []
    for path in args.files:
        if not path.is_file():
            problems.append(f"{path}: file not found")
            continue
        problems.extend(check_links(path))
        problems.extend(check_mermaid(path, args.min_diagrams))
        if not args.skip_claims:
            problems.extend(check_claims(path, root))

    if problems:
        print(f"FAIL: {len(problems)} documentation problem(s):", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        return 1
    print(f"PASS: docs ok ({', '.join(str(f) for f in args.files)})")
    return 0


if __name__ == "__main__":  # pragma: no cover - CLI wrapper
    raise SystemExit(main())
