# Choro default Bandmates and skill library

Research and implementation: 14 September 2026. Catalog version 1.

## Outcome and scope

Choro includes 15 editable default Bandmates and 25 offline skill packages in the normal app and MCP binaries. AI Slop Reviewer is included. A bandmate remains a saved configuration, not a single skill or a persistent personality: its model, job, outcome, and several complementary skills are captured for each new chat or delegated assignment.

This builds on the managed delegation implementation described in [Bandmates and managed delegation](choro-experts.md). The new work supplies useful defaults, bandmate-owned skills, references to installed skills and Riffs, frozen supporting resources, and a smaller settings form. It does not create a separate provider account, install upstream plugins, or export native subagent configurations.

## Bandmate lineup

Fable 5.1 uses the installed Claude account with Medium effort. GPT-5.6 Sol uses the installed Codex account with High effort. These are editable starting choices, not automatic model substitutions. An unavailable model produces the normal provider error; change the Bandmate explicitly to repair it.

| Bandmate | Skills | Default model | Selection reason |
|---|---|---|---|
| Product Planner | [Product Planning](https://github.com/phuryn/pm-skills/blob/18468a95b427e70e258b51389796367c6f684e7d/pm-execution/skills/create-prd/SKILL.md), [User Stories & Acceptance](https://github.com/phuryn/pm-skills/blob/18468a95b427e70e258b51389796367c6f684e7d/pm-execution/skills/user-stories/SKILL.md) | GPT-5.6 Sol | A bounded PRD and user-journey approach; enough structure to implement without a compulsory planning ceremony. |
| UI Designer | [Frontend Design](https://github.com/anthropics/skills/blob/34040c9c568585f6929bedeaad110ad08f079624/skills/frontend-design/SKILL.md), [Impeccable · Choro edition](https://github.com/pbakaus/impeccable/blob/cb56ed6c19a07329a9fa0cd4e657bee040156593/plugin/skills/impeccable/SKILL.md) | Fable 5.1 | Anthropic provides visual direction and implementation; Impeccable adds focused critique and refinement. Each has a distinct job. |
| Frontend Engineer | [React Best Practices](https://github.com/vercel-labs/agent-skills/blob/063bee94c3f4df8453406c830b0a7df0f2860278/skills/react-best-practices/SKILL.md), [React Composition Patterns](https://github.com/vercel-labs/agent-skills/blob/063bee94c3f4df8453406c830b0a7df0f2860278/skills/composition-patterns/SKILL.md) | GPT-5.6 Sol | Vercel provides concrete, referenced React performance and component API guidance; both are conditional on the existing stack. |
| Backend Engineer | [Domain Modeling & Contracts](https://github.com/mattpocock/skills/blob/3cca18b368ae95cdbdebbff572ccafa662551015/skills/engineering/domain-modeling/SKILL.md), [Postgres Best Practices](https://github.com/supabase/agent-skills/blob/8331f910845103c08d51f6ca1d86ebb7d1f745e3/skills/supabase-postgres-best-practices/SKILL.md) | GPT-5.6 Sol | Contract and invariant reasoning is broadly applicable; Supabase provides specific database evidence for Postgres projects. |
| Debugger | [Systematic Debugging · Choro edition](https://github.com/obra/superpowers/blob/b36e0829c6d0140e93cfef2ca599b1b07d4a7797/skills/systematic-debugging/SKILL.md), [Verification Before Completion](https://github.com/obra/superpowers/blob/b36e0829c6d0140e93cfef2ca599b1b07d4a7797/skills/verification-before-completion/SKILL.md) | GPT-5.6 Sol | Obra’s evidence-first debugging is useful across stacks. Verification should prove the fix without repeatedly rerunning unrelated checks. |
| Code Reviewer | [Code Review · Choro edition](https://github.com/getsentry/skills/blob/c2f99a5b04b4cd992ec3022d7c2c3e23e938d241/skills/code-review/SKILL.md), [Requirements Review](https://github.com/mattpocock/skills/blob/3cca18b368ae95cdbdebbff572ccafa662551015/skills/engineering/code-review/SKILL.md) | GPT-5.6 Sol | Sentry’s narrow review method identifies actionable bugs; the requirements review separately checks whether the requested behavior exists. |
| QA Tester | [Test Scenarios](https://github.com/phuryn/pm-skills/blob/18468a95b427e70e258b51389796367c6f684e7d/pm-execution/skills/test-scenarios/SKILL.md), [Web App Testing · Choro edition](https://github.com/anthropics/skills/blob/34040c9c568585f6929bedeaad110ad08f079624/skills/webapp-testing/SKILL.md) | GPT-5.6 Sol | User journeys supply meaningful cases; Anthropic’s browser testing approach supplies execution guidance when applicable. |
| Security Reviewer | [Differential Security Review · Choro edition](https://github.com/trailofbits/skills/blob/ce9ae2e2dc2de7ea05f4a8a6e636ccf576f83c79/plugins/differential-review/skills/differential-review/SKILL.md) | GPT-5.6 Sol | Trail of Bits has a security-specific threat and diff-review method. The Choro edition removes mandatory subagents and report files. |
| Performance Engineer | [Web Performance](https://github.com/addyosmani/web-quality-skills/blob/afa8da942115f2961fdbfa80807ea0b232ff6c00/skills/performance/SKILL.md), [React Best Practices](https://github.com/vercel-labs/agent-skills/blob/063bee94c3f4df8453406c830b0a7df0f2860278/skills/react-best-practices/SKILL.md) | GPT-5.6 Sol | Addy Osmani provides measurement and browser performance guidance; Vercel adds React-specific rules when relevant. |
| Accessibility Bandmate | [Accessibility](https://github.com/addyosmani/web-quality-skills/blob/afa8da942115f2961fdbfa80807ea0b232ff6c00/skills/accessibility/SKILL.md), [Web Interface Guidelines](https://github.com/vercel-labs/agent-skills/blob/063bee94c3f4df8453406c830b0a7df0f2860278/skills/web-design-guidelines/SKILL.md) | Fable 5.1 | Addy Osmani provides deeper accessibility patterns; Vercel adds a concise interface check. Automated checks are not a conformance certificate. |
| DevOps Engineer | [CI Efficiency · Choro edition](https://github.com/github/awesome-copilot/blob/1899b18da3fa5183652f86165917d553cba1850a/skills/github-actions-efficiency/SKILL.md), [Rollout Planning · Choro edition](https://github.com/github/awesome-copilot/blob/1899b18da3fa5183652f86165917d553cba1850a/skills/devops-rollout-plan/SKILL.md) | GPT-5.6 Sol | GitHub’s collection provides concrete CI and deployment planning patterns; the edition preserves existing infrastructure and approval boundaries. |
| Researcher | [Primary Source Research · Choro edition](https://github.com/mattpocock/skills/blob/3cca18b368ae95cdbdebbff572ccafa662551015/skills/engineering/research/SKILL.md) | GPT-5.6 Sol | Matt Pocock’s method favors reading primary evidence and resolving the actual question; a single focused skill avoids duplicate research frameworks. |
| UX Writer | [UX Writing · Impeccable edition](https://github.com/pbakaus/impeccable/blob/cb56ed6c19a07329a9fa0cd4e657bee040156593/skill/reference/clarify.md), [Humanizer](https://github.com/blader/humanizer/blob/9862685f575c65a8247f90369951df1b3416e3d6/SKILL.md) | Fable 5.1 | Impeccable’s clarification principles fit product states and labels; Humanizer helps remove generic prose while preserving meaning. |
| Documentation Writer | [Documentation · Choro edition](https://github.com/getsentry/skills/blob/c2f99a5b04b4cd992ec3022d7c2c3e23e938d241/skills/doc-coauthoring/SKILL.md), [Clear Writing · Choro edition](https://github.com/softaworks/agent-toolkit/blob/3027f20f3181758385a1bb8c022d4041dfb4de84/skills/writing-clearly-and-concisely/SKILL.md) | Fable 5.1 | The coauthoring approach supplies reader, purpose, and correctness checks; clear-writing guidance improves readability without a long interview. |
| AI Slop Reviewer | [Frontend Design](https://github.com/anthropics/skills/blob/34040c9c568585f6929bedeaad110ad08f079624/skills/frontend-design/SKILL.md), [Impeccable · Choro edition](https://github.com/pbakaus/impeccable/blob/cb56ed6c19a07329a9fa0cd4e657bee040156593/plugin/skills/impeccable/SKILL.md), [Humanizer](https://github.com/blader/humanizer/blob/9862685f575c65a8247f90369951df1b3416e3d6/SKILL.md) | Fable 5.1 | Reuses the strongest design and editing tools to find generic visual choices and empty copy. It assesses artifacts, not whether a person used AI. |

## Research method and tradeoffs

Candidates were discovered through skills.sh and upstream repositories, then reviewed by reading actual SKILL.md instructions, referenced resources, and license declarations. Repository stars and skill installs were treated as adoption signals. Repository stars describe the whole collection; they do not measure a particular skill or establish that it improves a model. No comparative quality benchmark was performed. The choices below are a curated starting set, with human outcome testing still needed.

The main filters were: a clear job, specific instructions with useful examples or references, credible maintenance or domain expertise, proportionate workflow cost, no required new accounts, and compatibility with Choro’s lead/child authority. Popularity alone did not override conflicts with the requested lightweight experience.

Observed repository adoption on the research date:

| Upstream collection | Repository stars observed | Why it matters here |
|---|---:|---|
| [addyosmani/web-quality-skills](https://github.com/addyosmani/web-quality-skills) | 2,786 | Performance and accessibility expertise with concrete references. |
| [anthropics/skills](https://github.com/anthropics/skills) | 176,178 | Official skill examples; Frontend Design and web testing. |
| [blader/humanizer](https://github.com/blader/humanizer) | 47,803 | Established prose-editing patterns, scoped to preserve facts and voice. |
| [getsentry/skills](https://github.com/getsentry/skills) | 991 | Smaller adoption, but a known engineering maintainer and a narrow code-review workflow; also distributes doc-coauthoring under Apache. |
| [github/awesome-copilot](https://github.com/github/awesome-copilot) | 38,979 | Broad community collection; only two small operational workflows selected. |
| [mattpocock/skills](https://github.com/mattpocock/skills) | 261,523 | Contract, requirements, and primary-source research methods. |
| [obra/superpowers](https://github.com/obra/superpowers) | 286,334 | Widely adopted debugging and verification principles. |
| [pbakaus/impeccable](https://github.com/pbakaus/impeccable) | 67,875 | Established design refinement toolkit; adapted to Choro’s coordinator. |
| [phuryn/pm-skills](https://github.com/phuryn/pm-skills) | 26,303 | Focused product planning and acceptance methods. |
| [softaworks/agent-toolkit](https://github.com/softaworks/agent-toolkit) | 2,464 | Selected only for a compact writing method; the full toolkit is not installed. |
| [supabase/agent-skills](https://github.com/supabase/agent-skills) | 2,599 | Maintainer-specific Postgres practices. |
| [trailofbits/skills](https://github.com/trailofbits/skills) | 7,067 | Security specialist authorship matters more than general-market popularity. |
| [vercel-labs/agent-skills](https://github.com/vercel-labs/agent-skills) | 31,173 | Maintainer-specific React guidance and interface checks. |

The skills.sh directory separately showed strong adoption for [Frontend Design](https://skills.sh/anthropics/skills/frontend-design), [React Best Practices](https://skills.sh/vercel-labs/agent-skills/vercel-react-best-practices), [Impeccable](https://skills.sh/pbakaus/impeccable/impeccable), and [Supabase Postgres Best Practices](https://skills.sh/supabase/agent-skills/supabase-postgres-best-practices). These counts change and are not embedded into ranking logic.

Alternatives and excess workflow removed:

- Full Impeccable includes engine/hook and multi-agent workflows. Choro ships a clearly labeled instruction edition with local review, polish, and copy references. It does not claim the upstream engine or scoring system is present.
- Full Superpowers and several Matt Pocock workflows mandate additional agents, chained skills, issue trackers, or repeated formal phases. Choro uses only the assigned task’s relevant method.
- Generic anti-slop kits that required an additional hosted/MCP service, fixed interrogation loops, or blanket design bans were not selected. AI Slop Reviewer instead uses Frontend Design, Impeccable, and Humanizer with explicit respect for the user’s chosen style.
- A small standalone accessibility checklist was passed over for more established, reference-backed accessibility material.
- Sentry’s code review was preferred over its security-review workflow; Choro’s security role uses Trail of Bits and explicitly includes risks from authenticated users.
- Platform-specific material is conditional: attaching Postgres or React guidance never authorizes changing the project’s database or framework.

## Exact shipped skill editions

Original means the upstream entrypoint and selected supporting Markdown resources are retained. Adapted means Choro’s edited instructions are explicitly marked, attributed, and licensed. The complete file list and SHA-256 digests are in [catalog.json](../crates/ide-core/assets/experts/catalog.json). Every source URL is pinned to a revision. Bundled files are validated offline; startup performs no downloads.

| Skill | Edition | License | Scope of changes |
|---|---|---|---|
| [Frontend Design](https://github.com/anthropics/skills/blob/34040c9c568585f6929bedeaad110ad08f079624/skills/frontend-design/SKILL.md) | upstream | Apache-2.0 | Upstream instructions retained; attribution and license files added. |
| [Impeccable · Choro edition](https://github.com/pbakaus/impeccable/blob/cb56ed6c19a07329a9fa0cd4e657bee040156593/plugin/skills/impeccable/SKILL.md) | adapted | Apache-2.0 | Focused instruction edition. Replaces installer/engine/hooks and native-agent orchestration with local references and the existing Choro task. Preserves brief-first design and bounded verification. |
| [React Best Practices](https://github.com/vercel-labs/agent-skills/blob/063bee94c3f4df8453406c830b0a7df0f2860278/skills/react-best-practices/SKILL.md) | upstream | MIT | Upstream instructions retained; attribution and license files added. |
| [React Composition Patterns](https://github.com/vercel-labs/agent-skills/blob/063bee94c3f4df8453406c830b0a7df0f2860278/skills/composition-patterns/SKILL.md) | upstream | MIT | Upstream instructions retained; attribution and license files added. |
| [Postgres Best Practices](https://github.com/supabase/agent-skills/blob/8331f910845103c08d51f6ca1d86ebb7d1f745e3/skills/supabase-postgres-best-practices/SKILL.md) | upstream | MIT | Upstream instructions retained; attribution and license files added. |
| [Product Planning](https://github.com/phuryn/pm-skills/blob/18468a95b427e70e258b51389796367c6f684e7d/pm-execution/skills/create-prd/SKILL.md) | adapted | MIT | Condenses the eight-section PRD template for ordinary tasks; removes mandatory document creation and invented planning detail. |
| [User Stories & Acceptance](https://github.com/phuryn/pm-skills/blob/18468a95b427e70e258b51389796367c6f684e7d/pm-execution/skills/user-stories/SKILL.md) | adapted | MIT | Removes fixed acceptance-criterion counts, mandatory design links, sprint assumptions, and unsupported independence claims. |
| [Domain Modeling & Contracts](https://github.com/mattpocock/skills/blob/3cca18b368ae95cdbdebbff572ccafa662551015/skills/engineering/domain-modeling/SKILL.md) | adapted | MIT | Retains domain precision and code cross-checks; removes automatic CONTEXT/ADR creation and adds concise implementation-contract guidance. |
| [Systematic Debugging · Choro edition](https://github.com/obra/superpowers/blob/b36e0829c6d0140e93cfef2ca599b1b07d4a7797/skills/systematic-debugging/SKILL.md) | adapted | MIT | Preserves evidence-led root-cause work; removes mandatory skill chaining, fixed phase gates, broad diagnostic examples, and architectural escalation after an arbitrary count. |
| [Verification Before Completion](https://github.com/obra/superpowers/blob/b36e0829c6d0140e93cfef2ca599b1b07d4a7797/skills/verification-before-completion/SKILL.md) | adapted | MIT | Retains evidence-before-claims. Removes mandatory full reruns on every message and ceremonial revert/reapply loops; prior evidence remains valid until relevant changes invalidate it. |
| [Code Review · Choro edition](https://github.com/getsentry/skills/blob/c2f99a5b04b4cd992ec3022d7c2c3e23e938d241/skills/code-review/SKILL.md) | adapted | Apache-2.0 | Removes Sentry-specific escalation/approval conventions, keeps actionable review criteria, and supports dirty working changes. |
| [Requirements Review](https://github.com/mattpocock/skills/blob/3cca18b368ae95cdbdebbff572ccafa662551015/skills/engineering/code-review/SKILL.md) | adapted | MIT | Keeps standards/spec comparison; removes parallel native agents, issue-tracker setup, and fixed committed-only diff assumptions. |
| [Test Scenarios](https://github.com/phuryn/pm-skills/blob/18468a95b427e70e258b51389796367c6f684e7d/pm-execution/skills/test-scenarios/SKILL.md) | adapted | MIT | Keeps observable acceptance testing; removes full-document templates and redundant per-click output. |
| [Web App Testing · Choro edition](https://github.com/anthropics/skills/blob/34040c9c568585f6929bedeaad110ad08f079624/skills/webapp-testing/SKILL.md) | adapted | Apache-2.0 | Provider-neutral browser tooling replaces the Python helper requirement; explicit condition waits replace blanket networkidle; respects existing UI permissions. |
| [Differential Security Review · Choro edition](https://github.com/trailofbits/skills/blob/ce9ae2e2dc2de7ea05f4a8a6e636ccf576f83c79/plugins/differential-review/skills/differential-review/SKILL.md) | adapted | CC-BY-SA-4.0 | Preserves risk-sensitive diff review, history, callers, and evidence. Removes native-agent routing and mandatory report files. Adds explicit authenticated authorization and redaction guidance. |
| [Web Performance](https://github.com/addyosmani/web-quality-skills/blob/afa8da942115f2961fdbfa80807ea0b232ff6c00/skills/performance/SKILL.md) | upstream | MIT | Upstream instructions retained; attribution and license files added. |
| [Accessibility](https://github.com/addyosmani/web-quality-skills/blob/afa8da942115f2961fdbfa80807ea0b232ff6c00/skills/accessibility/SKILL.md) | upstream | MIT | Upstream instructions retained; attribution and license files added. |
| [Web Interface Guidelines](https://github.com/vercel-labs/agent-skills/blob/063bee94c3f4df8453406c830b0a7df0f2860278/skills/web-design-guidelines/SKILL.md) | adapted | MIT | Pins remote guidelines locally and limits the review to assigned scope. Distinguishes conventions from accessibility requirements. |
| [CI Efficiency · Choro edition](https://github.com/github/awesome-copilot/blob/1899b18da3fa5183652f86165917d553cba1850a/skills/github-actions-efficiency/SKILL.md) | adapted | MIT | Removes automatic live test pushes and fixed top-three/savings requirements; preserves checks and separates local edits from external actions. |
| [Rollout Planning · Choro edition](https://github.com/github/awesome-copilot/blob/1899b18da3fa5183652f86165917d553cba1850a/skills/devops-rollout-plan/SKILL.md) | adapted | MIT | Replaces the ten-section mandatory rollout artifact with risk-proportionate planning; removes invented gates, calendar rules, and automatic communications. |
| [Primary Source Research · Choro edition](https://github.com/mattpocock/skills/blob/3cca18b368ae95cdbdebbff572ccafa662551015/skills/engineering/research/SKILL.md) | adapted | MIT | Removes mandatory background-agent launch and repository document; adds version/source verification and decision-focused stopping. |
| [UX Writing · Impeccable edition](https://github.com/pbakaus/impeccable/blob/cb56ed6c19a07329a9fa0cd4e657bee040156593/skill/reference/clarify.md) | adapted | Apache-2.0 | Extracts the focused clarify guidance as a self-contained skill; removes command chaining and unrelated design work. |
| [Humanizer](https://github.com/blader/humanizer/blob/9862685f575c65a8247f90369951df1b3416e3d6/SKILL.md) | upstream | MIT | Upstream instructions retained; attribution and license files added. |
| [Clear Writing · Choro edition](https://github.com/softaworks/agent-toolkit/blob/3027f20f3181758385a1bb8c022d4041dfb4de84/skills/writing-clearly-and-concisely/SKILL.md) | adapted | MIT | Keeps the practical composition guidance; removes mandatory subagent fallback and large reference-manual loading. |
| [Documentation · Choro edition](https://github.com/getsentry/skills/blob/c2f99a5b04b4cd992ec3022d7c2c3e23e938d241/skills/doc-coauthoring/SKILL.md) | adapted | Apache-2.0 | Audience, context, and reader checks from Sentry’s Apache-licensed distribution of Anthropic’s doc-coauthoring workflow. Removes compulsory interview phases and independent reader-agent loops. |

## User experience

### Configure a bandmate

Settings → Band starts with the default lineup. Edit name, when to use it, provider/model/effort, job instructions, and expected outcome. The model controls are compact dropdowns, instruction editors have bounded heights, and expected outcome can be expanded when needed. Existing shared button builders are used throughout.

Add skill offers four sources:

1. **Create a skill for this Bandmate:** give it a name, when-to-use description, and instructions. It belongs to that Bandmate and is saved with the profile. Saving the inner skill stages the edit; Save bandmate persists the profile. Cancel Bandmate discards the draft.
2. **Choose a Choro skill:** search the bundled library, inspect its instructions and adaptation notes, and open the pinned source.
3. **Link an installed skill:** select an available skill from the current provider’s catalog. Source files are read and frozen at chat start.
4. **Link a Riff:** attach an enabled existing Riff. New tasks follow the latest Riff contents; running tasks keep their frozen content. This does not change the independently selected chat Riff.

Detach changes only the Bandmate configuration. It does not remove the installed skill or shared Riff. Switching provider retains Choro/custom/Riff selections; incompatible installed links are marked for repair instead of silently discarded. Missing bundled IDs and disabled/missing Riffs are also visible and detachable.

### Start or delegate a task

For an ordinary chat, choose a bandmate from the new-chat / picker, keep the task text editable, and submit. For existing chats, /delegate opens the explicit assignment flow. Choosing a bandmate in an existing chat does not change the lead’s model.

For natural language, submit: “Improve this coffee website. Delegate the visual design to UI Designer and ask AI Slop Reviewer to review the result. Do the remaining work yourself.” The lead discovers only the authorized Bandmates, defines scope and dependencies, and calls the Choro delegation tools. Review can wait for the design result; independent implementation can run concurrently.

UI Designer receives the original assignment, the lead’s brief and acceptance criteria, relevant background, the frozen Bandmate setup, and an isolated copy of current files. Frontend Design guides the visual direction; the relevant Impeccable reference guides review or refinement. The agent does not mechanically execute every attached skill. Supporting references remain available from the local frozen package.

The parent conversation shows the task group and actual activity. Opening a bandmate uses its own chat, draft, permissions, working copy, and Preview ownership. Questions route through the lead; human approvals remain user-facing. The lead integrates the Bandmate’s contribution, resolves conflicts, verifies the combined change, and finishes the run.

## Storage and runtime changes

- Built-in profiles use stable UUIDs and a catalog-version marker. Seeding is transactional and preserves name collisions, user edits, renames, disabled profiles, and archive tombstones.
- BandmateProfile has additive, serde-defaulted fields for built-in origin, bundled skill IDs, custom skills, and linked Riff IDs. Existing records still deserialize. No additional table/schema migration is needed beyond the managed-delegation schema.
- Each new Bandmate snapshot includes exact instruction text and bounded supporting text files. Linked packages are captured twice with bounded retries so changes during capture cannot silently create a mixed snapshot.
- At launch, Codex and Claude use the same frozen instruction package. References are materialized under a content-addressed expert-skill-cache, with an interprocess writer lock and content checks. Altered cache content produces a repairable error, not silent replacement.
- Required packages are validated before child working-copy preparation. Historical chats use captured content even if Settings, the original skill, or a shared Riff later changes.
- Limits: 32 selected skill sources per Bandmate; 128 KB of skill entrypoint text; 4 MiB and 512 text files per package; 8 MiB of supporting resources per Bandmate. Binary resources and internal symlinks are rejected for linked packages. Installed plugins requiring files outside their package or additional runtime services are not converted into self-contained plugins.
- Export/import preserves profile additions and frozen resources in chat/task records. Imported runs remain inactive. Importing an older workspace reseeds missing defaults without replacing imported profiles. Future tasks with external installed-skill/Riff links still require those links to be available.
- Normal builds expose Bandmates by default. CHORO_EXPERTS=0 is an explicit troubleshooting opt-out. This does not auto-start a run, resume interrupted work, clean up files, or add provider credentials.

## Managed delegation plan retained

The lead remains the orchestrator. Choro owns durable scheduling, input/result deliveries, cancellation, recovery, and integration. Task dependencies form a DAG. One active run per parent, three concurrent Bandmates per run and six globally, one delegation level, and three automatic revisions are the default limits. Consultation ends at Accepted; implementation ends at Integrated, followed by lead verification.

Native provider spawning is disabled while managed runs are active. Bandmates work in independent Git metadata, seeded from the lead’s actual dirty working copy. Integration applies baseline-to-result changes into current parent files through a journal, preserving the source index. Stop durably pauses managed work; restart offers Resume instead of continuing automatically. Changes, conversations, and private copies remain preserved.

Plan-mode parents are limited to consultation/planning; implementation and integration pause when Plan mode applies. OpenCode delegation, non-Git implementation copies, native helper integration, and recursive delegation remain outside this release.

## Review and verification

### Review scope and fixes

The implementation received a local code review and an independent, read-only Claude Sonnet review, followed by a focused review of fixes and documentation. The reviewer received the current feature files, not the unrelated worktree. The review tool had no editing or repository tools. No production application or database was used for acceptance.

| Finding | Resolution | Evidence |
|---|---|---|
| Delegation required the original skill path even for bundled/frozen skills | Prepare validates and materializes the recorded package before child creation | `prepare_launches_bundled_skills_from_the_frozen_snapshot` |
| An older import could retain the destination seed marker but remove all defaults | Import invalidates that marker and reseeds without overwriting imported profiles | `importing_a_legacy_workspace_into_a_seeded_store_restores_defaults` |
| A source or cache file could become a symlink between checking and reading | Reads and creates use descriptor-relative opens with no-follow on every component | `open_rejects_replaced_parent_and_final_symlinks_for_reads_and_writes` |
| Empty directory trees could evade file-count limits | Independent traversal count and depth limits; root scans are counted too | `deeply_nested_empty_resource_directories_are_bounded` |
| Canonicalizing the entrypoint could follow a redirected SKILL.md | Resolve folder aliases, then reject a symlink entrypoint | `installation_folder_aliases_work_but_redirected_entrypoints_do_not` |
| A missing bundled skill could be invisible in the editor and impossible to repair | Show the missing reference with a Detach action | Code inspection; interaction remains in the manual check below |
| Simultaneous launches could see partially written shared cache files | Interprocess lock covers writing and checking the complete package | `concurrent_experts_share_frozen_packages_without_partial_reads` |
| Entry text alone did not preserve referenced material | Snapshot bounded supporting files and reconstruct them after import/restart | Core round-trip/archive tests plus authenticated provider fixture |
| Bundled source attribution and future skill updates were unclear | Pinned per-file manifest, readable bundled sources/licenses, original/adapted labels, and explicit new-chat version wording | Offline catalog validator; documentation review |

Claude confirmed the file-open and traversal-bound fixes. Its bundled-update concern is intended product behavior: new chats use this app version, existing chats keep their snapshot. Its import collision concern does not apply because the import transaction clears profile rows before inserting. The validator is included in `scripts/verify.sh`, contrary to an initial concern based on omitted script context.

A follow-up concern claimed pathname-based directory enumeration could ingest outside file contents. Enumeration remains pathname-based and bounded, but each actual content read reopens every component with no-follow checks; a symlink in the enumerated path therefore fails before contents can enter the snapshot. The claimed content-ingestion bypass was rejected after inspecting that path. These mechanisms are package integrity checks, not a sandbox against all actions an authorized provider can perform.

The review also reduced the verification skill itself: it now requires relevant current evidence without compulsory full reruns on every status message or ceremonial revert/reapply steps. This is recorded as an adapted edition rather than presented as unchanged upstream content.

### Automated and provider evidence

The final verification script checks formatting, all workspace targets, core/app/MCP tests, Claude bridge tests, Velotype tests, bundled skill provenance, and production app/MCP builds. The completed test stages report:

| Check | Result |
|---|---|
| ide-core | 339 passed; 4 opt-in tests ignored |
| ide-app | 719 passed; 3 opt-in tests ignored |
| ide-mcp | 28 passed |
| Claude bridge | 14 passed |
| Velotype | 694 passed in each of its two compiled test targets |
| Skill creator format validation | All 25 packages passed |
| Offline catalog validator | All 15 Bandmates and 25 pinned packages passed |
| Rust formatting; documentation relative links; bundle shell syntax | Passed |
| Production release build and signed final demo | Passed; full verification script completed in 141 seconds; macOS deep/strict signature check passed |

An additional authenticated provider run used `CHORO_ACCEPTANCE_BUILTINS=1` with `real_managed_providers_return_structured_results` in a new disposable `CHORO_DATA_DIR`. It passed in 257 seconds. It used the shipped UI Designer and Backend Engineer model/skill setups, plus a fixture-only proof skill. Each provider had to read a unique token from a frozen supporting reference after the original installation had been changed, include it in the contribution, submit a structured result, integrate, and resume the same session for a second revision. Codex and Claude both passed. The fixture is local text work, not a visual quality benchmark.

Reproduce the opt-in skill handoff check with existing provider authentication:

```sh
cargo build -p ide-mcp
CHORO_ACCEPTANCE_BUILTINS=1 CHORO_EXPERTS=1 \
CHORO_DATA_DIR="/tmp/choro-delegation-acceptance-$(uuidgen)" \
cargo test -p ide-app real_managed_providers_return_structured_results \
  -- --ignored --nocapture
```

Normal non-Bandmate chats and existing handoffs keep their previous behavior. Historical snapshots created before supporting-file freezing contain only the entry text that was captured at that time; resources absent from those older snapshots cannot be reconstructed from history. Start a new task to capture a complete current package.

### Human acceptance still to do

- Open the fresh demo’s Bandmates settings, inspect UI Designer and AI Slop Reviewer, and create and edit a custom setup — models, saved instructions, source details, and custom skills should remain clear and usable at the actual window size.
- Add a bandmate-owned skill plus an installed skill and a Riff; save, reopen, and start a chat — the selected setup should apply while the independent chat Riff and draft still behave normally.
- Delegate design and a subsequent AI Slop Reviewer pass on a disposable copy of the small coffee page — the lead should choose dependencies, both chats should be visible, questions should route correctly, and integrated output should improve the actual page.
- Stop delegated work, restart the demo, and Resume — no work should restart automatically or disappear. Check the side-panel draft, keyboard focus, and task Preview while switching between chats.

These interactive checks are deliberately left for the user. The implementation pass did not launch or control the running production app, install over an existing bundle, or use production project data for provider acceptance.

## Ready-to-test build

The feature is enabled in the normal production binaries. A fresh isolated demo was also built for manual testing:

- App: `target/release/bundle/Choro Bandmates Demo 20260914T074642Z-31314.app`
- Demo data on first launch: `/tmp/choro-experts-20260914T074642Z-31314`
- Demo log: `/tmp/choro-experts-20260914T074642Z-31314.log`
- Build: local ad-hoc signing; deep/strict bundle verification passed. All shipped Bandmate source and license files match the reviewed catalog exactly.

The demo has its own app identity, data directory, Chromium profile, and Choro Keychain service names; relay is disabled. It uses the existing Codex/Claude authentication. No production project data was copied. The app was not launched during this pass, and no existing application was replaced. Use a disposable copy of the coffee page for the visual delegation check.

The bundled library also ships readable source, license, and notice files under `Contents/Resources/licenses/expert-skills`, including the adapted share-alike security-review source.
