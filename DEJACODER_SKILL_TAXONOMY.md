# DejaCoder Skill Taxonomy (DCQL + MCP)

## Goal

Define a skill-first operating model for DejaCoder in Codex:

- Keep MCP tools as focused primitives.
- Use multiple narrow skills to orchestrate those primitives.
- Reserve raw, broad DCQL access for expert paths.

This gives higher reliability and lower context clutter than one large, universal skill.

## Core Principle

Use a layered model:

1. MCP tools = capability surface (`execute_dcql`, `code_query`, `code_grep`, `smart_read`, `get_insights`, `expand_result`, `validate_dcql`, `dcql_help`).
2. Skills = intent-specific policies over those tools.
3. Hooks (optional) = routing/policy guardrails (default output mode, deny noisy calls, enforce query validation).

## Recommended Skill Families

### 1) Locate + Read

- Skill name: `dc-locate-read`
- Primary user intent: "Where is X?" "Show me implementation of Y."
- Primary tools: `code_grep`, `code_query`, `smart_read`
- DCQL subset: `entity_type`, `name`, `file_scope`, `limit`, optional `where`
- Defaults:
  - `output_mode = locations|minimal`
  - `limit <= 20`
- Avoid:
  - Raw `execute_dcql` unless `code_query` cannot express the query.

### 2) Call Chain Tracing

- Skill name: `dc-call-chain`
- Primary user intent: "Who calls this?" "What does this call?" "Trace path A -> B."
- Primary tools: `code_query`, `smart_read`, optional `execute_dcql`
- DCQL subset:
  - `calls`, `called_by`
  - constrained multi-hop call traversal queries
  - optional `file_scope`
- Defaults:
  - first pass `summary|minimal`
  - expand only selected nodes via `expand_result`
- Avoid:
  - full-graph dumps with unbounded traversal.

### 3) Data/Parameter Flow

- Skill name: `dc-parameter-flow`
- Primary user intent: "How does parameter/value X move through the system?"
- Primary tools: `execute_dcql`, `smart_read`
- DCQL subset:
  - function/method relationships
  - call edges plus argument/parameter mapping predicates (where available)
  - bounded depth path queries
- Defaults:
  - bounded hops (for example 2-4)
  - `summary|minimal` first, then targeted expansion
- Avoid:
  - loading full source for every node in the path.

### 4) Change Impact Analysis

- Skill name: `dc-impact-analysis`
- Primary user intent: "If we change X, what breaks?"
- Primary tools: `code_query`, `execute_dcql`, `get_insights`
- DCQL subset:
  - reverse call graph (`called_by`)
  - membership (`member_of`), inheritance (`inherits`)
  - metrics filters (`churn`, `complexity`, API/publicness if available)
- Defaults:
  - produce ranked impact tiers (`direct`, `nearby`, `broad`)
  - return IDs/locations before details
- Avoid:
  - mixing unrelated metrics in first pass.

### 5) Architecture Mapping

- Skill name: `dc-architecture-map`
- Primary user intent: "Explain module/component architecture."
- Primary tools: `code_query`, `get_insights`, selective `execute_dcql`
- DCQL subset:
  - module/file/class relationships
  - dependency edges between components
  - aggregate/summary queries
- Defaults:
  - summary-only maps first
  - drill into one subsystem at a time
- Avoid:
  - turning architecture requests into raw grep outputs.

### 6) Semantic Similarity + Structural Constraints

- Skill name: `dc-semantic-analogs`
- Primary user intent: "Find code like this" with guardrails.
- Primary tools: `execute_dcql` (semantic clauses), `code_query`, `smart_read`
- DCQL subset:
  - similarity/vector predicates
  - plus structure constraints (`entity_type`, metrics, relationships)
- Defaults:
  - retrieve small candidate set, then rerank with structural filters
  - provide top-k rationale, not giant candidate lists
- Avoid:
  - pure similarity without structural narrowing.

### 7) Refactor Candidate Discovery

- Skill name: `dc-refactor-candidates`
- Primary user intent: "Find best refactor targets."
- Primary tools: `get_insights`, `code_query`, `execute_dcql`
- DCQL subset:
  - high complexity + high churn + high fan-in/fan-out patterns
  - dead code / ownership risk / concurrency risk presets where available
- Defaults:
  - top-N ranked list with explicit scoring dimensions
  - include only minimal location + key metrics
- Avoid:
  - reading full bodies for every candidate before ranking.

### 8) Raw DCQL Expert Mode

- Skill name: `dcql-expert`
- Primary user intent: explicit custom query authoring.
- Primary tools: `validate_dcql`, `dcql_help`, `dcql_completions`, `execute_dcql`
- DCQL subset: unrestricted (full language).
- Defaults:
  - always `validate_dcql` before execution
  - start with `output_mode = summary|minimal`
  - require explicit user ask for broad/full outputs
- Avoid:
  - bypassing validation when query is complex or generated.

## Why Many Focused Skills Beat One Mega Skill

- Each skill can enforce small, deterministic query templates.
- Intent routing becomes easier and more predictable.
- Context cost drops because each skill controls output mode and expansion strategy.
- You can evolve one workflow (for example call-chain) without destabilizing others.

## Quiet MCP Strategy (Skills-First)

Keep the MCP layer "quiet" and let skills drive behavior:

- MCP tools expose concise descriptions and stable schemas.
- Skills define when and how to call tools.
- Prefer `minimal|locations|summary` outputs by default.
- Use `expand_result` for progressive disclosure.
- Gate expensive `include_context` and large `limit` behind explicit need.

## Routing Matrix (Intent -> Skill)

- "where is / show me / locate": `dc-locate-read`
- "who calls / what calls / trace call chain": `dc-call-chain`
- "parameter flow / value propagation / data movement": `dc-parameter-flow`
- "if I change this, what breaks": `dc-impact-analysis`
- "explain architecture / component map": `dc-architecture-map`
- "find similar implementation": `dc-semantic-analogs`
- "refactoring targets / risky hotspots": `dc-refactor-candidates`
- "I want to write this query exactly": `dcql-expert`

## Suggested Rollout

1. Start with 4 skills: `dc-locate-read`, `dc-call-chain`, `dc-impact-analysis`, `dcql-expert`.
2. Add `dc-parameter-flow` once argument mapping coverage is reliable for your target languages.
3. Add `dc-semantic-analogs` after tuning embedding retrieval thresholds.
4. Add `dc-refactor-candidates` and `dc-architecture-map` when ranking heuristics stabilize.

## Skill Deployment (Codex)

### Required Skill Shape

Each skill must be a folder containing `SKILL.md` with YAML frontmatter:

```markdown
---
name: dc-call-chain
description: Trace callers/callees with bounded depth.
---
...skill instructions...
```

If frontmatter is missing or invalid, the skill will not load.

### Install Locations

Codex discovers skills from these locations:

- Project-local: `.codex/skills/<skill-name>/SKILL.md`
- Repo-scoped: `.agents/skills/<skill-name>/SKILL.md` (between project root and current working dir)
- User-global (preferred): `~/.agents/skills/<skill-name>/SKILL.md`
- User-global (legacy, still supported): `~/.codex/skills/<skill-name>/SKILL.md`

### Quick Deploy Flow

1. Create skill folder in one of the install locations.
2. Add `SKILL.md` with valid frontmatter + instructions.
3. Restart Codex (or force skills reload via app-server `skills/list` with `forceReload=true`).
4. Verify it appears in the available skills list in session context.

### Enable/Disable via Config

You can disable a skill explicitly in user config:

```toml
[[skills.config]]
path = "/absolute/path/to/SKILL.md"
enabled = false
```

Set `enabled = true` to re-enable.

## Skill Authoring Guidance for This Taxonomy

- Keep each `SKILL.md` under ~300-500 lines.
- Put examples and heavy docs in `references/`.
- Put deterministic query builders/scripts in `scripts/`.
- Keep per-skill tool allowlist in instructions (and optionally enforce via pre-tool hook policy).
- Emit concise, structured outputs: IDs/locations first, then opt-in deep context.
