# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

## Users

Developers and technical leads who coordinate several local coding-agent conversations across repositories, worktrees, tasks, documentation, and source-control workflows.

## Product Purpose

Choro is a native macOS workspace for directing concurrent coding agents without losing the state, intent, or safety boundary of each conversation.

## Positioning

Choro brings parallel agent work into one coherent local control surface. Voice provides fast composer dictation and a separate project-level thinking partner; it is not an invisible automation system.

## Operating Context

The product is used as a dense desktop workspace, often for long sessions with several chats running at once. Voice may be used while the user is reading code, moving between chats, or temporarily away from the keyboard. Audio environments vary, and downloaded speech models must remain optional until the user explicitly activates voice.

## Capabilities and Constraints

- Native Rust and GPUI application targeting macOS 13 or later.
- Local-first speech recognition with Moonshine Small Streaming English and Smart Turn v3.2.
- Models download only after first voice activation and add roughly 165 MiB to local application data, not the initial app download.
- Existing authenticated Codex or Claude command-line sessions provide coordinator reasoning; voice introduces no separate API key.
- Quick Ask opens from ⌘⌥A for fast questions without creating an agent. It can investigate with read-only project access, terminal commands, web requests, databases, and services, but it cannot create, edit, move, rename, or delete files or perform Git mutations. It uses the agent chat's composer and message-rendering foundation—including multiline text, pasted or dropped image attachments, Markdown lists, tables, code blocks, message metadata, activity, and attention states—while omitting agent-only work such as file changes, Code Review, and Ship. It can use the active project or a General scope, and its provider/model can be changed per session.
- Completed Quick Ask answers are kept in one local, global history beneath Add Project until the user clears it. Ask History opens as a searchable center workspace: compact conversation rows lead to a full transcript, and an explicit Continue Conversation action restores only that archived session into the disposable Quick Ask panel. Settings controls the default Quick Ask model and history clearing.
- Project Talk is a read-only, project-level thinking partner. It can explain the active project, inspect recent Git history, and discuss an approach to a feature, but it cannot edit files or control existing agents.
- Composer dictation always leaves editable text in the selected chat before the user sends it.
- Command-L is push-to-talk dictation from anywhere in the workspace: hold to record, release to transcribe, and insert the result directly at the current cursor in the open agent chat. It never invokes coordinator reasoning or sends without an explicit spoken command.
- Holding Command-L starts one-shot dictation and releasing it inserts the transcript. Command-Shift-L independently toggles hands-off dictation; each 200 ms speech pause inserts one phrase. Ending a phrase with “send” removes the command and submits the composer in either dictation mode. Both shortcuts are remappable in Settings.
- Project Talk is a separate explicit action on ⌘⇧A and is remappable in Settings. It uses bounded project descriptors, relevant source excerpts, working-tree status, recent commits, and per-project conversation history. A conversation continues turn by turn until the user ends it or two minutes pass without speech; the microphone is closed while Choro reads or answers.
- Saying “create a plan for that” in Project Talk ends the voice session and starts a new agent in Plan mode with the project discussion as context. No other spoken discussion starts or controls an agent.
- Background noise and speech that produces no recognized words are recoverable turns: Choro stays silent, shows that it is still listening, and keeps the session open.
- Project Talk never writes to an agent chat. Dictation is available only through Command-L or the top microphone control, keeping discussion and composer input as separate modes.
- After speech starts, a stricter trailing-speech barrier prevents steady background volume from extending the turn indefinitely.
- Raw microphone audio is not retained. Text transcripts are local and user-clearable.
- The first release is English-only and turn-based. It does not depend on a cloud realtime voice API.

## Brand Commitments

Preserve Choro's calm, compact, tool-like visual language and existing design tokens. Voice should remain observable and interruptible, with clear listening, thinking, speaking, download, and error states. Feature controls use the shared UI builders rather than introducing a parallel visual system.

## Evidence on Hand

The source application and its current interaction patterns are the primary evidence. No external marketing claims or user-research artifacts are present in this workspace.

## Product Principles

1. Make parallel work legible: always identify which conversation voice is referring to or changing.
2. Keep authority explicit: listening begins by direct user action, chat writing requires an explicit voice command, and coordinator tools remain narrowly bounded.
3. Prefer local and recoverable behavior: keep audio ephemeral, persist only useful transcript text, and queue prompts through existing chat paths.
4. Degrade gracefully: communicate model-download and capability states without blocking ordinary text workflows.
5. Keep the keyboard path first-class: every voice action has a visible, editable, or interruptible UI equivalent.

## Accessibility & Inclusion

Voice controls require accessible names, keyboard shortcuts, visible state beyond color, reduced-motion-safe feedback, and compatibility with VoiceOver. English-only recognition must be stated clearly rather than inferred. Speech output is optional and never the sole carrier of important status.
