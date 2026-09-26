# Choro content marketing hub

## Mobile refresh — September 25, 2026

**Setup lessons now finished:** **Pair Your iPhone with Choro** (52.1s, `mobile-pair-v2`) and **Choose What Your Phone Can Do** (51.1s, `mobile-access-v2`) are completed with current Simulator/Demo recordings, fresh Alex D / ElevenLabs and HeyGen, approved titles and shared outro. Pairing identifiers and codes are masked. The permission control cycles levels when clicked; it is not a menu. The original pairing remained Full access, while the temporary recording pairing was left View only and its app stopped. No product code or relay configuration changed. Successful live flows were recorded, but connection intermittency was observed outside those takes and is documented separately, not claimed fixed.

**Current total: 68 of 70 tutorials complete; all eight Mobile-related lessons (including the Getting Started overview) are finished.** The two unrelated outstanding lessons are Studio comparison and Companion agent-following. The six-video checkpoint below is historical.

Six current-interface Mobile exports are completed: **Respond to Questions, Plans, and Approvals** (53.7s), **Review Results and Ask for Fixes on Mobile** (59.3s), **Meet Choro Remote** (51.2s), **Check Agent Progress from Your Phone** (50.7s), **Start or Continue Agent Work on Mobile** (66.8s), and the Getting Started overview **Take Choro with You on Mobile** (53.3s). The library now points to their `mobile-*-v2` exports, with playback, script, file and folder actions. Four reuse existing approved narration/avatar assets; the two new lessons use fresh Alex D / ElevenLabs and HeyGen. Original versions remain preserved.

The current Simulator connected to Demo through its existing pairing. Genuine question, plan, approval, follow-up, fix and verification results were captured; Mac shots match the phone follow-up. Sparse phone captures were normalized to CFR before trimming. All six passed full decoding, narration recognition, audio timing and sampled visual review; human creative/lip-articulation review remains. Native captures are 1206×2622 phone and 1280×820 Mac, framed into 4K without added source detail.

**66 of 70 tutorials complete.** Two Mobile setup lessons remain unrecorded pending confirmation of a temporary Simulator pairing and its View only / Control / Full access demonstration. Existing pairing permissions were not changed. Studio comparison and Companion agent-following remain separate outstanding lessons; historical checkpoints below are not current totals.

## Top-level navigation

**Tutorials**, **Short Videos**, **Social Media Plan**, **Competitors**, and **Influencers** are separate main-menu destinations. The existing HTML filename stays unchanged so saved links still work. Open `http://127.0.0.1:8769/how-to-video-library.html#shorts` for shorts, `#social-plan` for the plan, `#competitors` for the comparison, or `#influencers` for the creator outreach list, with the local launcher running; the same hashes work on a direct-file copy.

**Influencers** links to the [YouTube creator outreach Google Sheet](https://docs.google.com/spreadsheets/d/1ShgXVGWHKBcDKf0Ii3fbD7LPj_qtRcwPWn_8RiLNDIo/edit?gid=0) and previews 12 channels spanning the compared tools. Aside researched and created the Sheet on September 23, 2026, reporting 118 distinct independent creators and 25 publicly sourced business email addresses. The Sheet is the editable source of truth for channel URLs, subscriber snapshots, relevant video links, tool coverage, email sources, and later outreach tracking. Blank email cells mean no public address was confirmed; do not infer one. The HTML preview is a dated snapshot, and no creators have been contacted.

Competitors has a feature-by-feature table for Choro, Nimbalyst, Orca, Emdash, Conductor, Superset, T3 Code, Codex desktop, and Claude Code Desktop. The latter two are first-party agent apps, included because their desktop workflows compete directly with Choro. The table compares agent choice, parallel work, docs, tasks, design, previews, database tools, review, shipping, scheduled runs, remote execution, cloud agent runtimes, and desktop platform support, then names each product's standout feature. Each rated feature cell has a ✓ for dedicated support, ◐ for partial or adjacent support, or × when no dedicated current feature is documented, with a larger mark above its reason. Product header links include locally embedded logos. Choro's column has an accent background and stays visible during desktop horizontal scrolling. **Show ratings only** hides explanations and the text-only Core focus and Killer feature rows; **Show all details** restores them. Specific availability caveats link to public sources. Choro's × ratings make current gaps visible: no in-app scheduler, no SSH runner or hosted cloud workspace, and no Windows/Linux desktop build. Its integrated agent roster is limited to Claude Code, Codex, and OpenCode. Its mobile cell is ◐ because iPhone agent control exists while Docs, Tasks, Studio, and push notifications are missing; Android readiness was not verified in the [September 22 mobile parity audit](mobile-parity-audit-2026-09-22.md). This comparison is for internal positioning and should be revalidated before external use; it does not change tutorial or completion counts. The comparison logos are embedded into the HTML so the direct-file copy works without extra assets. The local launcher allows `data:` images for those logos; other CSP directives remain unchanged.

The first comparison row, **View type**, uses category labels instead of ✓/◐/×. It describes how the user talks to the coding agent, not whether the app has a shell for commands. Choro, Orca, Emdash, Conductor, and Superset offer both chat UI and terminal agent sessions; Orca's chat and Conductor's terminal agent mode are experimental, and Emdash's chat depends on ACP-capable agents. Nimbalyst, T3 Code, Codex desktop, and Claude Code Desktop use chat UI for their desktop agent conversations, even though some include a separate terminal or have a separate CLI product. The category label remains visible in **Show ratings only**; the source-linked explanation hides with other details.

**Remote agent execution** means the agent runs on another machine, including an SSH host supplied by the user. **Cloud agent runtime** separately asks whether the product offers provider-hosted work that continues when the laptop is off. In that row, ✓ means vendor-hosted compute (Conductor, Codex, Claude), ◐ means the product can use a server or cloud account the user supplies (Orca, Emdash, Superset, T3 Code), and × means no cloud agent runtime is documented (Choro, Nimbalyst). Explanations state who pays for compute and whether the user can keep an existing agent subscription: Conductor charges for Pro cloud access but accepts agent subscriptions or API keys; Codex uses ChatGPT plan allowance or credits; Claude cloud counts against Claude subscription limits without a separate compute fee. Bring-your-own-host options can incur the user's own VM bill and use agent credentials on that host. Conductor's published pricing says extra compute usage fees are planned. Prices and access rules are a September 23, 2026 snapshot, not a promise of future terms. For Choro, managed cloud work would add infrastructure, storage, credential, and usage-management costs; retaining users' existing Claude or Codex access where permitted is a product preference, not a shipped capability.

The Competitors tab ends with **Choro gaps to track**, a short internal summary of the current comparison: managed cloud workspaces, user-owned remote hosts, scheduled agent runs, broader agent compatibility, fuller iPhone workflows, and Windows/Linux desktop support. Cloud comes first because it would let work continue when the Mac sleeps, while introducing compute, storage, credential, and usage costs. The list describes gaps, not committed roadmap items. It remains visible when the table is switched to ratings-only mode.

Logo sources: Choro uses `crates/ide-app/assets/brand/choro-riff.svg`; Codex and Claude use the existing `crates/ide-app/assets/agent-icons/` SVGs. Nimbalyst, Orca, Emdash, Conductor, Superset, and T3 Code use icon files published on their official websites. The embedded copies are fixed to the September 23, 2026 comparison snapshot.

Social Media Plan embeds the existing `social-media-plan.html`, loaded on first selection, with a full-page link. Plan contents are not duplicated. Links back to the library leave the frame instead of nesting the hub. The launcher explicitly serves only the plan and its linked `social-launch/README.md` record in addition to existing media routes. Same-origin framing is allowed for the plan only; the hub still rejects framing. Restart the launcher after changing its routes or the hub HTML. Direct-file iframe behavior remains subject to browser permissions; the full-page link is available as a fallback.

Short Videos contains 24 audience-first concepts in five benefit-led groups: **five newly finished promotional shorts awaiting review**, the existing upbeat reference, 12 other proposed scripts, and six idea-only outlines. The five new shorts have fresh upbeat ElevenLabs narration and HeyGen Alex, genuine recorded product evidence, full-screen Alex openings, circular Alex over the demonstration, lower captions, and the shared motto/free-download outro. They are independent promotional stories, not shortened tutorial scripts. No Choro Mobile video was made. Production stopped at the user's five-video review limit; nothing is published or scheduled. Unproduced runtimes remain edit targets, never finished-media counts.

### Five-short review batch — September 22

| Subject | Actual MP4 duration |
| --- | --- |
| You wrote the brief. Why explain it twice? | 26.067 seconds |
| A rough idea is enough to start. | 24.633 seconds |
| The specialist you need doesn't exist yet. That's okay. | 24.633 seconds |
| Try the idea without disturbing the main project. | 27.500 seconds |
| Change the headline. Keep the design. | 22.467 seconds |

Find the **Review new video** badges in Short Videos. Each modal includes the playable 1080×1920 export, actual script, local file link and folder action. Assets, source camera selections, paid-generation receipts, transcripts, full-decode checks, sampled-frame review and audio-envelope checks are preserved in `social-five-review-20260922`. The audio check verifies soundtrack timing, not mouth-shape quality. Creative approval remains with the user. No product code changed. Main tutorial totals are unchanged.

`short-video-concepts` is the editorial manifest, separate from the playable media allowlists. Its optional `mediaId` references a real entry in `social-video-files`. Existing tutorial totals are unchanged. “Earlier vertical cuts” links to the retained archive at `#tiktok`; old archive deep links still work, but the archive is no longer a tutorial-category tab. Search, modal keyboard focus, direct links, playback cleanup and file/server fallbacks have DOM tests; visual browser playback is not established by those tests.

## Current finishing pass — September 22, 2026

### Added September 23 — agent-to-agent questions

**Let Your Agents Ask Each Other** is now a completed **39.7-second** lesson in **Productivity & Shortcuts**, with fresh Choro Demo footage, Alex D / ElevenLabs, HeyGen, the approved title and shared outro. Type `#`, select the existing Coffee page review agent, choose **Question**, and see its genuine answer return to the original conversation. The picker is **same-project only**; the earlier cross-project claim was corrected after inspecting the live Demo and implementation. This is read-only consultation—not Quick Ask, a new Bandmate, or automatic implementation.

Full decoding, local narration recognition, audio-envelope timing and sampled final frames passed. Alex moves to the upper-right for the recipient shot so the response stays readable. A seeded-chat preflight failure and the first framing export are preserved separately; neither is passed off as the successful exchange. Native capture is 1280×820; 4K export does not add source detail. No product source changed. The library now lists **70 tutorials, 64 completed**, leaving the previous six unfinished lessons.

### Native Studio — eight finished Alex videos

The library now has **63 completed tutorials out of 69**. Eight newly finished videos use real native Studio footage, Alex D / ElevenLabs narration, HeyGen, approved playlist titles and the shared outro. Main project navigation is hidden or cropped so Studio's own canvas, sidebar and inspector have space.

| Video | Actual final MP4 duration |
| --- | --- |
| Design, Build, and Preview with Studio | 57.033 seconds |
| Meet Choro Studio | 70.233 seconds |
| Create a Design from Scratch | 56.833 seconds |
| Build and Use a Design System | 61.633 seconds |
| Create a Design from a Doc or Task | 42.833 seconds |
| Refine a Design with the Assistant | 35.500 seconds |
| Implement a Design with an Agent | 50.133 seconds |
| Use Visual References with an Agent | 47.767 seconds |

Each is marked Completed with a preview, local video link, folder action and matching scene/narration table. The Getting Started overview has a new native-Studio title. Visual References deliberately uses a Studio PNG export and a read-only agent request naming the project image; it is not an Assets-page walkthrough. The user chose to retain that lesson.

All eight passed full decoding, narration transcript comparison, audio-envelope alignment and sampled final-frame review. Design Systems also passed an Apple color-managed frame check. Native capture is 1532×980; the 4K export does not add source detail. Recordings, receipts and initial review cuts remain preserved under `design-studio-20260922` and `playlist-final-20260921/studio-*`.

**Six tutorials remain:** Compare the Design with the Live Result, Follow Agents with Desktop Companion, and four Mobile lessons. Studio Compare opens a split view but its live pane stays blank even though the same HTTP-200 page renders in ordinary Preview; the library shows this specific blocker. No product source was changed, and no Compare avatar was purchased. Companion/Mobile were not retested in this Studio pass. Promotional shorts are a separate backlog, not included in these tutorial totals. Earlier checkpoints below are historical.

### HeyGen recovery verified — five final exports

After the user reported HeyGen's fix, fresh Workflow and Bandmate jobs completed successfully. Both reuse the existing paid Alex D narration and recordings. Three newly recorded Companion lessons now also have Alex D narration and HeyGen, reusing the approved local-review picture edits, greeting and shared outro.

| Video | Actual final MP4 duration |
| --- | --- |
| Start a Chat with a Bandmate | 62.433 seconds |
| Save a Git Workflow: Dev to Main | 65.133 seconds |
| Meet Choro Companion | 38.300 seconds |
| Set the Mood with Companion Music | 49.467 seconds |
| Show or Hide Choro Companion | 24.433 seconds |

All five are linked as completed with preview, video link, folder action and matching scene text. Final files are `playlist-final-20260921/<lesson>/final-alex-d-4k.mp4`. Full decode, transcript comparison, audio synchronization and sampled final frames passed. Final audio offset is 0 ms; avatar audio offset is 20 ms. A Companion title also passed native Apple color-managed decoding. 4K export does not add detail to the retained lower-resolution recordings. No product source changed. Prior review videos and failure receipts remain preserved.

Current total: **55 completed Alex tutorials**. Eleven lessons still lack complete exports: one Companion agent-following lesson, four Mobile and six native Studio. Those are not marked finished by this avatar pass. The Design phase was paused when the user requested finishing the missing HeyGen versions first.

### Earlier Companion local-voice checkpoint

Three new **1080p local-voice review videos** use the user's real Companion recording: Meet Choro Companion (about 40 seconds), Set the Mood with Companion Music (about 50 seconds), and Show or Hide Choro Companion (about 25 seconds). They have the approved lavender title treatment and shared outro, no avatar, no subtitles, and no captured music. Preview links, folders, actual MP4 durations and updated narration tables are in the HTML. Source takes and scene-isolated narration remain available for later Alex replacement.

The music lesson shows saved playlist settings, mood selection, animation and playback controls; it does not claim a new playlist was added. Visibility uses the real Settings Off/On route, not a right-click demonstration. The overview includes an actual Needs attention banner; the complete Working/attention/result-following lesson is still pending.

Current total: **50 completed Alex tutorials + 5 playable local-voice reviews = 55 playable main tutorials**. Eleven lessons lack complete exports: one Companion agent-following lesson, four Mobile, and six native Studio. Mobile has fresh partial footage and a successful Full access check, not finished lessons. Earlier checkpoints below are historical.

### Earlier local-voice checkpoint

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
