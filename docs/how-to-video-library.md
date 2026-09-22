# Choro content marketing hub

## Top-level navigation

**Tutorials**, **Short Videos**, and **Social Media Plan** are separate main-menu destinations. The existing HTML filename stays unchanged so saved links still work. Open `http://127.0.0.1:8769/how-to-video-library.html#shorts` for shorts or `#social-plan` for the plan, with the local launcher running; the same hashes work on a direct-file copy.

Social Media Plan embeds the existing `social-media-plan.html`, loaded on first selection, with a full-page link. Plan contents are not duplicated. Links back to the library leave the frame instead of nesting the hub. The launcher explicitly serves only the plan and its linked `social-launch/README.md` record in addition to existing media routes. Same-origin framing is allowed for the plan only; the hub still rejects framing. Restart the launcher after changing its routes or the hub HTML. Direct-file iframe behavior remains subject to browser permissions; the full-page link is available as a fallback.

Short Videos contains 24 audience-first concepts in five benefit-led groups: one playable upbeat reference, 17 existing proposed scripts, and six new idea-only outlines. The additions cover saved-work continuity, backlog-to-result, concrete visual references, narrow-screen quality, mid-task direction changes, and Choro's broader product-home story. Idea-only entries contain a hook, suggested product moment, payoff and proof requirements; they have no drafted narration, runtime or media. Nothing new was recorded, generated or scheduled. Once produced and approved, 24 shorts could cover eight weeks at three posts per week. Studio still requires verification against the new native tool before production. Planned runtimes on scripted concepts are edit targets, never counted as finished media. No voice/avatar credits were spent for this library change.

`short-video-concepts` is the editorial manifest, separate from the playable media allowlists. Its optional `mediaId` references a real entry in `social-video-files`. Existing tutorial totals are unchanged. “Earlier vertical cuts” links to the retained archive at `#tiktok`; old archive deep links still work, but the archive is no longer a tutorial-category tab. Search, modal keyboard focus, direct links, playback cleanup and file/server fallbacks have DOM tests; visual browser playback is not established by those tests.

## Current finishing pass — September 22, 2026

Local-voice update: the two review entries now point to **free local Kokoro narration, no avatar** exports in `local-voice-reviews-20260922`: Bandmate chat (65.426 seconds) and saved Git workflow (67.060 seconds). Both preserve the complete real screen demonstrations and intro/outro; no HeyGen or ElevenLabs requests were made. Full decoding passed, and representative identity/result/merge frames were inspected. The previous paid-narration cuts and failure receipts are retained. Scene-isolated scripts, WAVs, source maps and timing files allow later Alex replacement, which will still need alignment to the final speech. These are review drafts, not newly approved final tutorials.

Fourteen lessons have no complete playable demonstration: four Companion, four Mobile and six native Studio lessons (including the Getting Started design overview). UI control was requested for fresh Demo/Companion/Simulator recording under the current repository preference; no new app interaction has taken place in this pass. The mobile verification/fix route is still absent from the relay allowlist in the current source; no product code was changed. Older connection/control notes below are historical, not fresh runtime checks.

### Earlier paid-narration checkpoint

Latest update: **50 completed tutorials plus two full screen-and-narration review cuts** (52 playable tutorials). Connect an External Task Board is finished (1:08). Start a Chat with a Bandmate has the corrected persistent identity footage (1:02). Save a Git Workflow: Dev to Main is now recorded and edited through the actual private Demo PR #2 merge and Choro Merged result (1:05). Both review cuts include Alex D narration and approved intro/outro, but their new HeyGen renders failed with `SPACE_ENCRYPTION_DISABLED` (workspace customer-managed encryption key disabled). One authorized Bandmate retry failed identically; no further identical retries are being submitted. No merge permission or recording blocker remains for these three lessons. The library marks the two missing-avatar exports accurately, rather than completed. No product source was edited by this recording pass.

### Earlier September 22 status (historical)

The paid Alex D / HeyGen upgrade is complete for all 36 existing non-Design drafts. Together with the original 14, there are 50 playable tutorial entries: **49 completed and one revision-flagged prior take**. “Follow and Manage Your Agents” now has a verified 1:32 fresh retake covering two working projects, Needs attention, answering, completion, manual Done and finding the result through Search. “Start a Chat with a Bandmate” still needs the persistent conversation identity. Original recordings and old exports remain on disk. Full decoding, audio alignment, local transcript comparison and sampled-frame review passed for the new status export. Ten other non-Design lessons remain incomplete. Native Design Studio is a later requested phase. Product-code changes are not authorized; code-dependent lessons remain pending.

The **Earlier vertical cuts** archive contains 74 playable 9:16 exports: 72 tutorial-derived cuts and two versions of the standalone social-first test, **“Stop explaining which button. Point to it.”** The upbeat version is first (28.2 seconds), followed by the original (25.4 seconds), measured from their MP4s. The second version retains the same genuine demo, uses upbeat speech cues without local slowdown and a medium-expressiveness avatar, and adds the spoken/on-screen motto “Your home for building products.” Each standalone version used one fresh ElevenLabs narration and one HeyGen avatar job. The earlier 72 cuts reuse tutorial voice/avatar assets. The first five Band cuts also have full-canvas revisions and free-download outros. The user liked the upbeat direction; the archive as a whole is not marked creatively approved. It uses `social-video-files`, separate from tutorial counts, with preview, local video link and explicit Finder action. No clips have been posted externally.

Production receipts and reproducible scripts: `/Users/lirangabai/Movies/Choro Tutorials/playlist-final-20260921/`. The historical notes below describe earlier batches; use the HTML manifests for current availability.

From the project root:

```sh
python3 docs/serve-video-library.py
```

Open the printed `http://127.0.0.1:8769/how-to-video-library.html#start` address. Keep the terminal running; Ctrl+C stops it. Use `--port 0` if that port is occupied. Restart after editing the HTML/manifest.

Click a completed lesson to preview its latest export, open the video separately, open its folder in Finder, or copy the local video link. Playback stops when the modal closes. The storyboard remains below the player; unfinished lessons have no video controls.

The completed tutorials are 4K HEVC; the source screen recordings retain their original captured detail. The earlier 36 Kokoro/no-avatar drafts remain preserved but have been superseded by Alex D / HeyGen versions. Only the Bandmate prior take still uses a revision-flagged draft mapping. Each modal includes playback, actual runtime, a separate video link, a Finder action and narration storyboard.

Ten non-Design lessons still need complete recordings. Project Talk remains excluded; its comparison is Quick Ask or Agent? The rebuilt Demo previously completed the three Band demonstrations, Preview feedback, an env edit/save/restore and custom Orbit module creation. September 22 mobile checks currently show Mac unavailable despite Demo Remote access being On. Git Workflow setup and PR review are recorded, but creating/merging the private Demo PR still requires its action-time confirmation. Jira connection and fresh mobile pairing/access changes must preserve existing credentials and devices. Current evidence is in the lesson modals and `REMAINING-RECORDINGS.md`; old Keychain failures are historical. Design Studio is a subsequent requested phase. The shortcut-customization export retains its specific caveat: saving/restoring a binding was shown, but successful invocation was not demonstrated.

Use Safari on this Mac for HEVC playback, or Finder → QuickTime if another browser lacks HEVC support. Copied links work only on this Mac while the launcher is running; they are not public sharing links.

The page's `completed-video-files` JSON maps 49 completed tutorial exports. Meet Choro links to the 1:46 philosophy-first revision, with landing-page-inspired diagrams before the real app appears. Its modal includes the matching ten-scene script; the earlier export remains preserved. Getting Started still has the Companion overview and new Design Studio overview unfinished. Serving the library makes no copies or uploads. The default folder is `~/Movies/Choro Tutorials`; override it with `--library-root /path/to/library` if the library moves.

Opening the HTML directly still shows the storyboards and local video links. Direct-file playback depends on browser permissions. Finder access requires the launcher; another static HTTP server cannot read arbitrary local video files.

The launcher binds only to 127.0.0.1, serves the hub, the explicit plan documents above and allowlisted video/poster files, supports byte-range seeking, and requires a per-process token plus same-origin POST for Finder actions. It does not expose a general shell or file browser.

## Checks

`python3 -B docs/test_video_library.py` checks media ranges, scope restrictions, missing files and Finder routing with the OS command mocked (no Finder UI). Tiny fixtures remain in an isolated temporary directory.

`node docs/test-video-library.cjs` requires an available `jsdom` installation (or `NODE_PATH` pointing to one). It checks all manifest mappings and all 66 modals in local-server, direct-file and ordinary-HTTP modes. It does not establish browser codec support or visually test playback.

“Shape an Idea with Docs” is completed (1:25), directly before “Work from Context: Tasks and Docs.” Its modal includes the approved assistant/drafting storyboard, a preview, direct file link and folder control for the verified clear-controls 4K export. “Take Choro with You on Mobile” is also completed (0:50): genuine iOS Simulator pairing to Demo, a phone reply, and its matching Mac continuation/result. Its modal links to the verified clear-layout 4K export. Pairing credentials and internal review messages are excluded from the video.

## Following agents and Companion

“Dictate Prompts and Messages” now links to its verified 60.233-second shortcut-first revision: ⌘L hold/release, ⌘⇧L hands-off start/stop, then the microphone alternative. Both shortcuts have large centered callouts. Genuine prior microphone footage is reused; keyboard gestures are explained rather than filmed, and staged example text is disclosed. The previous export remains preserved on disk.

“Start a Chat with a Bandmate” also needs a footage revision: show the newer Bandmate profile chip in the composer and the same identity beneath the conversation title. Its previous video is preserved and labelled separately from the unrecorded planned storyboard.

“Follow and Manage Your Agents” now links to the verified September 22 replacement in `playlist-final-20260921/agents-follow-statuses-v2`, with the matching seven-scene script. Its old take is preserved. The prior claim that sidebar All reveals Done work was not supported by this run, so the lesson demonstrates Search instead. “Follow Agents with Desktop Companion” still has no complete video. No product-code fixes are authorized; code-dependent lessons stay pending.

## Companion playlist

Added Meet Choro Companion to Getting Started, immediately after the first-agent flow. The dedicated Companion tab has only three lessons: Follow Agents with Desktop Companion (agent states, character activity and live CPU / RAM), Set the Mood with Companion Music (Spotify playlists and playback), and Show or Hide Choro Companion. The earlier follow lesson moved from Agents without duplication. All four have planned scene/narration tables, no exports. Existing videos are unchanged. New script folders: companion-meet, companion-music, companion-visibility; agents-companion is retained and expanded with process monitoring. Companion intro treatment remains to be selected before production; the outro stays shared. Getting Started has 16 lessons, 14 completed; library total 66 across 11 playlists.
