# Context Map

A structured view of context files within the workspace for token efficiency and cost optimization.

## Directory Tree & Inventory of Markdown Files

```
.
├── README.md           # ~15,000 bytes (~3,750 tokens)
├── context_map.md      # Context file inventory & token cost
├── docs/superpowers/plans/2026-09-15-rust-traceroute-rewrite.md # ~22,000 bytes (~5,500 tokens)
├── docs/superpowers/plans/2026-09-16-clean-shutdown-and-linked-spans.md # ~10,500 bytes (~2,600 tokens)
├── docs/superpowers/plans/2026-09-16-pr-comments-and-hardening.md # ~20,400 bytes (~5,100 tokens)
├── docs/superpowers/plans/2026-09-16-serde-yml-migration-and-hardening.md # ~9,800 bytes (~2,450 tokens)
├── docs/superpowers/specs/2026-09-15-rust-traceroute-rewrite-design.md # ~5,500 bytes (~1,375 tokens)
└── token_usage.md      # Token usage dashboard
```

## Details & Estimated Costs

| File | Size (bytes) | Estimated Tokens | When Loaded | Why Loaded |
| :--- | :--- | :--- | :--- | :--- |
| `README.md` | ~15,000 | ~3,750 | Initial project onboarding | Understand existing architecture, CLI usage, parameters, and flags |
| `context_map.md` | ~1,200 | ~300 | Context management | Audit and optimize token usage across files |
| `token_usage.md` | ~1,800 | ~450 | Ongoing dashboard | Track and monitor token consumption metrics |
| `docs/.../2026-09-15-rust-traceroute-rewrite-design.md` | ~5,500 | ~1,375 | Architecture & design reference | Reference architectural decisions, schemas, and specifications |
| `docs/.../2026-09-15-rust-traceroute-rewrite.md` | ~22,000 | ~5,500 | Implementation execution | Step-by-step TDD development guide |
| `docs/.../2026-09-16-pr-comments-and-hardening.md` | ~20,400 | ~5,100 | Code review & hardening | Detailed PR review resolution history |
| `docs/.../2026-09-16-serde-yml-migration-and-hardening.md` | ~9,800 | ~2,450 | Migration & hardening execution | Step-by-step implementation plan for serde_yml fork |

## Smart Loading System Guidelines

- **Task-Driven Loading:** Only load markdown documentation when actively working on related functional components.
- **Cache & Selective Slices:** Avoid repeatedly loading full documentation files; load specific slices when needed.
- **Zero-Waste Protocol:** Remove redundant documentation files and keep specs concise and high signal-to-noise.
