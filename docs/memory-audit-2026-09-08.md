Alfred agent memory audit, 8 September 2026

Alfred has a sound storage foundation, but its recall and learning policies need work before we can call the memory dependable. Keep the SQLite model. Prioritize relevant retrieval, evidence for learned claims, a restricted curator, and realistic evaluation.

This audit covers the current working tree, based on commit `85f0d64`, including existing uncommitted changes. It uses source inspection, existing tests, a synthetic SQLite experiment, and primary research/documentation. It does not measure production retrieval quality or inspect the user's actual memory database. Application code and settings were not changed.

The existing design gets several things right. Memories have user, workspace, and workflow scopes; types; provenance; active/superseded/retracted states; expiry; and explicit replacement links. Pinned context is bounded at 6,000 bytes. Automatic recall is separately bounded at eight items and 6,000 bytes. Automatic suggestions require approval, and approval applies canonical changes transactionally. Run history remains available separately. These are useful foundations to retain.

LangChain's documentation distinguishes ongoing conversation state from durable memory and discusses both collections of individual facts and background memory updates. Alfred already implements substantial parts of that design. The documentation also calls out the harder problems of updating collections and preserving relationships between facts. [Memory overview](https://docs.langchain.com/oss/python/concepts/memory).

The changes I recommend, in priority order:

1. **Fix natural-language recall and allow an empty result.** `prepare_agent_prompt` passes the base prompt into recall. The retriever takes its last 8,000 bytes, then `plain_text_fts_query` takes the first 12 whitespace-separated terms from that slice and joins them with `AND`. Every selected term must match. There is no removal of common words or semantic matching. If matching fails, the retriever still adds up to ten recent candidates, regardless of relevance, then renders up to eight.

   A synthetic experiment used the repository's actual FTS schema and a Python translation of its query builder. The memory "Production releases require approval from Maya" matched "production releases". It did not match "Please prepare the production release checklist" or "Who needs to sign off before we ship?". The lexical misses were executed in SQLite. From the Rust fallback logic, if that rule is older than ten unrelated visible memories, recency cannot recover it. This is a concrete counterexample, not an estimate of production accuracy.

   Build a dedicated retrieval query from the current task and relevant event fields. Preserve exact identifiers, expand useful terms, and use ranked partial matching. The connected-app trigger currently arrives separately from the search query, so event-specific entities can be missed too. Add a relevance gate and permit zero automatic memories. Keep recency as supporting information or an explicit continuation policy. Compare semantic retrieval against this improved lexical baseline before choosing an embedding dependency.

   Evidence: [query builder](../src-tauri/src/db/history.rs), function `plain_text_fts_query`; [retriever](../src-tauri/src/db/memory_retrieval.rs), functions `ranked_candidates` and `candidate_score`; [runner](../src-tauri/src/runner/mod.rs), function `prepare_agent_prompt`.

2. **Restrict the background curator's capabilities.** The production `AdapterReviewAgent` calls the ordinary coding-agent adapter. The request carries the workflow working directory. The Claude adapter starts with `--permission-mode bypassPermissions` and permits up to 40 turns. It does not disable tools for review. Candidate validation happens after the adapter returns, so it cannot prevent actions the reviewer might already have taken.

   Give curation a dedicated execution mode that can only produce structured suggestions, with no filesystem, shell, or connected-app tools. Explicitly enforce input and execution limits. Retain approval for canonical memory changes. No malicious reviewer run was executed during this audit; the capability mismatch is established by the call path.

   Evidence: [review execution](../src-tauri/src/runner/mod.rs), `AdapterReviewAgent::run_review` and `run_memory_review_job`; [Claude adapter](../src-tauri/src/agents/claude_code.rs), `ClaudeCodeAdapter::run`. Anthropic documents persistent memory poisoning and recommends enforcing containment outside the model. Prompt labels alone cannot enforce that boundary. [How we contain Claude across products](https://www.anthropic.com/engineering/how-we-contain-claude).

3. **Require evidence for each learned claim and prevent circular learning.** The runner persists the fully composed agent prompt, including injected memories. The review digest then includes persisted input prompts and prioritizes the final output. Consequently, old memory and an agent's own assertions can reappear as evidence in a later review. The suggestion schema has a rationale and model-reported confidence, but no required source quotation, source event, or evidence type. Validation sets `source_node_id` to `None`.

   Persist original task input, injected memory references, observations, and model output as distinguishable fields. Require each candidate to cite an actual source span and classify it as a user statement, observed result, or inference. Show the evidence beside the approval action. Existing memory repeated by an agent must not count as independent confirmation. Model confidence should not be presented as a measured probability of truth. These are weaknesses in evidence handling; this audit did not observe a fabricated production memory.

   Evidence: [runner](../src-tauri/src/runner/mod.rs), assignments to `persisted_input_prompt`; [curation](../src-tauri/src/db/memory_curation.rs), `collect_digest_leaves`, `ReviewerSuggestionRaw`, and `validate_candidate_suggestion`.

4. **Keep saved outputs available without treating them as durable facts by default.** Output nodes default `saveToMemory` to true. Their full output is saved through the frontend event handler, and canonical inserts default confidence to 1.0. Recall has no memory-type eligibility filter, so reports and artifacts compete with preferences, constraints, and decisions. As output accumulates, it also feeds the recent fallback.

   Preserve saved reports in history/artifact storage and make searching them an explicit capability. Use curated claims for ordinary durable recall. The existing types can support this policy without replacing the database. Explicitly selected reports should remain usable.

   Evidence: [output defaults](../src/features/workflow/types.ts), `defaultOutputNodeData`; [output saving](../src/features/workflow/store.ts), `handleRunEvent`; [canonical defaults](../src-tauri/src/db/memories.rs), `plan_canonical_insert`; [retrieval filtering](../src-tauri/src/db/memory_retrieval.rs), `ranked_candidates`.

5. **Apply a total prompt budget to explicit Memory nodes too.** `format_memories_context` reads and appends the full body of every selected memory, including artifact files. Unlike pins and automatic recall, this path has no item or total size bound. Multiple nodes append to `context_prompt`. A large selected report can therefore defeat the limits elsewhere. Explicit memory also lacks an entry in the current memory-use reason enum, which records lexical, recent, and pinned use.

   Introduce a shared budget for all memory contributions. Send compact excerpts plus references for large bodies, preserve complete short claims where possible, and allow bounded follow-up reads. Record explicit selections and omissions in the same use trace. Anthropic describes retrieving a small amount of context initially and loading additional evidence through tools when needed. [Effective context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents).

   Evidence: [explicit formatter](../src-tauri/src/db/memories.rs), `format_memories_context` and `memory_full_body`; [runner](../src-tauri/src/runner/mod.rs), the `memory` branch; [trace schema](../src-tauri/src/db/schema.sql), `run_memory_uses`.

6. **Make corrections find the old claim reliably.** Duplicate detection normalizes whitespace but does not recognize equivalent claims expressed differently. The curator sees at most 12 existing memories through the same fragile retrieval path. A correction can therefore miss the old claim and produce another active memory. Approved review memories also receive neither `last_confirmed_at` nor `expires_at`.

   Retrieve possible duplicates and conflicting claims specifically for each proposed fact. Introduce an optional entity/fact key where useful, compare target versions before applying a pending replacement, and retain the existing supersede/retract mechanism. Record observation and validity dates for changing facts. Use review dates selectively for longer-lived knowledge; a universal expiry period would discard stable preferences unnecessarily.

   Evidence: [curation](../src-tauri/src/db/memory_curation.rs), `candidate_content_hash`, `candidate_existing_memory_context`, and `apply_approved_candidate`. LongMemEval explicitly evaluates knowledge updates, temporal reasoning, and abstention, alongside recall. [LongMemEval, ICLR 2025](https://arxiv.org/abs/2410.10813).

7. **Move output persistence into the backend run lifecycle.** The Output node's Rust branch creates the result but does not save memory. Saving happens in the frontend's `step_completed` handler. A missing listener, frontend reload, or absent workflow graph can therefore prevent the save. This is a source-established dependency; a live closed-window run was not tested.

   Persist requested output once in the backend using the run's graph snapshot and an idempotency key derived from run and node. The UI should display the result of persistence. This also makes scheduled execution independent of frontend event delivery.

   Evidence: [runner](../src-tauri/src/runner/mod.rs), `chooseOutput`/`output` branch; [frontend](../src/features/workflow/store.ts), `handleRunEvent`.

8. **Measure memory usefulness before expanding automation.** Existing tests exercise scope, lifecycle, ranking formulas, limits, approval transactions, and error behavior. The recall tests explicitly expect recent fallback for unmatched queries. Passing them establishes implementation behavior, but does not establish that memory improves agent answers.

   Start with 40–60 representative cases covering paraphrases, old relevant facts behind recent noise, corrections, expired facts, unrelated tasks, scope isolation, large outputs, circular evidence, and injected instructions. Measure useful-memory precision, recovery of necessary facts, incorrect-memory use, downstream task success, token cost, and latency. Compare memory disabled, current recall, improved lexical recall, and any proposed semantic retrieval. For learning, measure unsupported claims, duplicate proposals, and correction success.

   Add tasks that require remembering how a workflow works, not only isolated facts. The May 2026 LongMemEval-V2 preprint tests workflow knowledge, changing state, environment gotchas, and mistaken premises. Its results are evidence for broader evaluation, not a performance promise for Alfred. [LongMemEval-V2](https://arxiv.org/abs/2605.12493).

I would implement the restricted curator and the evaluation fixtures first, then repair recall and prompt budgets. Next come evidence-backed consolidation and backend output persistence. Once those pass evaluation, expose bounded `memory.search`, `memory.get`, and history lookup tools so agents can recover evidence that initial recall omitted. I did not find a first-class Alfred memory/history search tool in the inspected agent tool contract; frontend history search already provides reusable backend functionality.

Keep ongoing run state, durable facts/preferences, reusable workflow lessons, and saved outputs distinct in behavior. They can share SQLite and the existing library. Introduce additional agent-private scopes only when the product needs private memory for distinct agent identities; the current user/workspace/workflow scopes are a reasonable starting point.

Verification completed: `cargo test --locked --manifest-path src-tauri/Cargo.toml memory -- --test-threads=2` passed 64 tests; the separate `db::memories` run passed 12 tests, with overlap between these filters. Five focused Bun files passed 39 tests. The SQLite experiment confirmed the lexical counterexample. No live provider calls, production database changes, or UI QA were performed.
