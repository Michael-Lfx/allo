# Technical Solution Specification Standard

> **Scope**: Unified authoring standards and quality red lines for technical design documents, ADRs, refactoring proposals, and implementation plans across `docs/` and its subdirectories (such as `docs/agent-store/` and `docs/architecture/`).  
> **Golden Principle**: **"Readable architecture upfront, complete archives at the back; rigorous Mermaid syntax, closed-loop decisions and acceptance."**

---

## 1. Core Purpose & Two-Tier Architecture

Technical documentation is the primary vehicle for cross-role collaboration, architectural evolution, and auditability. Past documents tended to fall into two traps: unstructured dumps of jargon without clear context, or overly aggressive summaries during refactoring that discarded critical low-level contracts and historical evidence.

This standard establishes a **Two-Tier Archival Model**:

```text
┌──────────────────────────────────────────────────────────────┐
│ 1. Readable Technical Proposal (Front half: 5-min executive read)│
│   ├── §1 Background & Pain Points (Domain, problem, matrix)   │
│   ├── §2 Solution & Architecture (Mermaid topology, invariants)│
│   ├── §3 Subsystem Design (Sequence, structs, state machines) │
│   ├── §4 Key Decisions & Trade-off Matrix (ADR, choices & why)│
│   └── §5 Acceptance Criteria & Verification Matrix (TC ladder)│
├──────────────────────────────────────────────────────────────┤
│ 2. Appendix: Historical & Technical Reference Archive (Back)  │
│   > Notice: Verbatim, complete preservation of pre-rewrite text│
│   [100% full original text, tables, schemas, and meeting notes]│
└──────────────────────────────────────────────────────────────┘
```

---

## 2. Standard Document Structure (6 Core Modules)

Every technical solution document must follow this sequence:

### 2.1 Header & Metadata
```markdown
# [Feature / Module Name] · Technical Solution

> **Status**: 🧊 Baseline / ✅ Implemented / 🔧 Partially Implemented / 📋 Planned / 🗄️ Historical
> **Core Principle**: [1~2 sentence summary of highest invariants and design rules]
```

### 2.2 §1 Background & Core Pain Points
- **Business Context**: Target user, integration host, and problem definition;
- **Current State & Key Pain Points**: Numbered bottlenecks (performance, conceptual confusion, security risks, or missing interfaces);
- **Capability Matrix**: Markdown table comparing "Current Behavior" vs. "Target in Proposal".

### 2.3 §2 Solution Overview & Architectural Topology
- **End-to-End Topology**: Mermaid `flowchart TD` or `flowchart LR` delineating system boundaries (External Client, Host App Server, Upstream Engines/Services);
- **Goals vs. Explicit Non-Goals**: Clear boundaries on what is implemented in this phase and what is strictly deferred;
- **Architectural Invariants**: Hard rules such as "credentials never leave host" and "effective permissions intersection".

### 2.4 §3 Core Design & Subsystem Specifications
- **Module Responsibilities**: Granular breakdown of services, providers, and handlers;
- **Interactive Sequence Flow**: Multi-party or multi-step interactions illustrated with Mermaid `sequenceDiagram` (with `autonumber` and explicit actors);
- **Data Structures & Schemas**: Authoritative Rust structs, TypeScript interfaces, or JSON-RPC schemas;
- **State Machine Matrix**: Valid state enums, transition conditions, and irreversible terminal states.

### 2.5 §4 Key Decisions & Trade-off Matrix
Numbered decision table detailing alternatives considered and why they were rejected:

| ID | Decision Item | Selected Solution | Rejected Alternatives & Rationale |
|---|---|---|---|
| **D1** | [Decision name] | **[Chosen conclusion]** | ❌ [Alternative]: [Specific architectural flaw, security risk, or cost] |

### 2.6 §5 Acceptance Criteria & Verification Ladder
- **Acceptance Criteria**: Numbered items (`S1`, `TC-AS-001`) with clear prerequisites and assertable outcomes;
- **Verification Ladder**: Ordered minimal sequence of tests (unit -> integration -> real environment).

---

## 3. Mermaid Diagramming Standards (VS Code & Cross-Renderer Compatibility)

In VS Code, GitHub, and web renderers, Mermaid has strict syntax and topology constraints. Parse errors cause diagrams to fall back to raw code blocks. **The following rules are mandatory:**

### 3.1 Strict Syntax Rules

1. **Never use `-->>` arrows in Flowcharts**:
   - ❌ **Forbidden**: `A -->> B` (Only valid in `sequenceDiagram`; throws fatal parse error in `flowchart`);
   - ✅ **Correct**: Use solid `-->`, dotted `-.->`, or thick `==>`.
2. **Always quote edge labels containing special characters**:
   - ❌ **Forbidden**: `A -->|1. skill/files & skill/file| B` (`&` is Mermaid's node-join operator and breaks the parser);
   - ❌ **Forbidden**: `A -->|call(name, args)| B` (Parentheses and commas disrupt tokenization);
   - ✅ **Correct**: `A -->|"1. skill/files or skill/file"| B` or `A -->|"2. connector/call(name, args)"| B`.
3. **Never nest square brackets inside rhombus nodes `{}`**:
   - ❌ **Forbidden**: `Gate{"[connector_proxy] enabled?"}` (Confuses rectangular node delimiters);
   - ✅ **Correct**: `Gate{"Policy Gate: connector_proxy enabled?"}`.
4. **Use `<br/>` for multiline text, never `\n`**:
   - ❌ **Forbidden**: `Node["Line 1\nLine 2"]`;
   - ✅ **Correct**: `Node["Line 1<br/>Line 2"]`.

### 3.2 Architectural Flow & Topology Rules

1. **Arrows must adhere to true proxy boundaries**:
   - In a proxy architecture, upstream child processes (e.g. Stdio) or external HTTP servers must NEVER point directly back to the client;
   - All responses must route back through the Host Router for sanitization, credential stripping, and normalization before reaching the client.
2. **Avoid deep cross-subgraph back-edges**:
   - Large backward jumps across multiple nested subgraphs trigger layout tangling in Dagre;
   - Use Flowchart for high-level module layout, and SequenceDiagram for multi-step request/response flows.

---

## 4. Preservation of Historical Reference Archives in Git

When refactoring or improving existing technical documentation:

1. **100% Verbatim Appendices**:
   - After the main solution text, append:
     ```markdown
     ---

     ## 附录：原始技术底稿与历史归档 (Historical & Technical Reference Archive)

     > **归档说明**：以下完整保留重构前的原始技术底稿、历次讨论与历史记录全文，供历史追溯、协议字段详细对照与技术审计。

     ---

     [Full verbatim pre-rewrite markdown text]
     ```
   - Never use placeholder summaries (e.g. `(Retain original §4.1...)`); never discard DTO schemas, meeting notes, or test logs.
2. **Git Diff Audit Rule**:
   - Run `git diff --stat <path>` before opening a PR;
   - Proper refactoring must show **massive insertions and zero or near-zero deletions** (`Insertions >> 0, Deletions ≈ 0`).
