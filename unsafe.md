# Unsafe code audit — working findings

This file is the **running notebook** of the unsafe audit. After all files in
`unsafe-todo.md` are processed, this file will be rewritten into a polished
report.

Per-finding entries use the following structure:

```
### <id> <Short title> — <severity>
- Location: `path/to/file.rs:LINE` (and others if relevant)
- Category: <Send/Sync | transmute | FFI | from_raw | …>
- Claim:    <what the unsafe block is meant to guarantee>
- Issue:    <what is wrong, missing, or under-documented>
- Severity: Critical | High | Medium | Low | Note
- Notes:    <optional caller analysis / suggested fix>
```

Severity rubric:

- **Critical** — known unsoundness reproducible from safe API surface, UB likely in practice.
- **High** — likely unsoundness; needs only a plausible caller to trigger.
- **Medium** — fragile invariant relying on undocumented caller behavior, or unsoundness only with `unsafe` callers; or platform-specific risk.
- **Low** — minor: missing `SAFETY:` comment, suboptimal API, defense-in-depth.
- **Note** — sound but worth recording (e.g. macro-generated `unsafe impl`).

---

## Findings
