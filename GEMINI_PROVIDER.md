Gemini is temporarily hidden from provider and model pickers pending Google approval. Its integration, model definitions, and saved-chat support remain in place. Restore visibility through `AgentKind::is_visible_in_picker` in `crates/ide-core/src/agents.rs` when requested. The instructions below apply once Gemini is visible again.

Choro's Gemini provider uses Google's official Antigravity ACP server with Google subscription sign-in. Google ended personal Gemini CLI subscription access on June 18, 2026; enterprise Gemini CLI access is separate.

Select Gemini in the model picker and start a chat. Choro downloads Google's ACP server into its own provider cache on first use. Approve **Sign in with Google**, then sign in with the Google account attached to your AI Pro or Ultra subscription. Google also supports a free allowance. No Gemini API key or Google Cloud project is required for personal subscription sign-in.

The models offered are Gemini 3.1 Pro High and Gemini 3.8 Flash Medium/High. Availability and limits are controlled by Google and your plan. Existing chats keep their provider session ID and resume through ACP.

Quick Ask and text generation reuse the same Google login after you connect through a Gemini chat. They require Google's read-only planning mode. Text generation rejects every tool request; Quick Ask allows reads and rejects actions capable of changing files. Chat access modes are resolved through Choro's approval cards. Completed provider diffs feed Choro's existing change ownership records.

The provider files and Google's authentication state live under Choro's configuration directory at `data/providers/google`. Choro does not read or copy OAuth token contents. Set `GEMINI_ACP_CLI` to an already extracted official `agy_acp_server.par` (or `.exe`) to use your own installation; keep Google's `localharness_external` sidecar next to it. Terminal conversations use Google's separate `agy` CLI, which must be installed and signed in independently.

Sources:

- [Google's Gemini CLI migration announcement](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/)
- [Subscription authentication for Google's ACP integration](https://antigravity.google/docs/ide/extensions/zed/)
- [Google's current models](https://antigravity.google/docs/models)
- [Official Google ACP distribution in the ACP registry](https://github.com/agentclientprotocol/registry/blob/main/antigravity-acp/agent.json)
