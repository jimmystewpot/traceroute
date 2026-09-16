# Context Map

A structured view of context files within the workspace for token efficiency and cost optimization.

## Directory Tree & Inventory of Markdown Files

```
.
├── README.md           # 4,458 bytes (~1,114 tokens)
├── context_map.md      # Context file inventory & token cost
├── docs/superpowers/plans/2026-09-15-rust-traceroute-rewrite.md # ~22,000 bytes (~5,500 tokens)
├── docs/superpowers/specs/2026-09-15-rust-traceroute-rewrite-design.md # ~5,500 bytes (~1,375 tokens)
└── token_usage.md      # Token usage dashboard
```

## Details & Estimated Costs

| File | Size (bytes) | Estimated Tokens | When Loaded | Why Loaded |
| :--- | :--- | :--- | :--- | :--- |
| `README.md` | 4,458 | ~1,115 | Initial project onboarding | Understand existing architecture, CLI usage, parameters, and flags |
| `context_map.md` | ~800 | ~200 | Context management | Audit and optimize token usage across files |
| `token_usage.md` | ~1,200 | ~300 | Ongoing dashboard | Track and monitor token consumption metrics |
| `docs/.../2026-09-15-rust-traceroute-rewrite-design.md` | ~5,500 | ~1,375 | Architecture & design reference | Reference architectural decisions, schemas, and specifications |
| `docs/.../2026-09-15-rust-traceroute-rewrite.md` | ~22,000 | ~5,500 | Implementation execution | Step-by-step TDD development guide |

## Smart Loading System Guidelines

- **Task-Driven Loading:** Only load markdown documentation when actively working on related functional components.
- **Cache & Selective Slices:** Avoid repeatedly loading full documentation files; load specific slices when needed.
- **Zero-Waste Protocol:** Remove redundant documentation files and keep specs concise and high signal-to-noise.
