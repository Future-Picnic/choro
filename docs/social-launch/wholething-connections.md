# Wholething connection status — September 22, 2026

Workspace: **Choro**, https://app.wholething.media/launches.

**Verified result: four connected channels.** X, Facebook and Instagram were connected during this setup. YouTube was already listed as Choro and was left untouched; Wholething does not expose its exact channel ID in the channel card.

| Platform | Status | Remaining work |
|---|---|---|
| YouTube | Existing Choro connection listed | Expected @chorodev; exact underlying channel ID not exposed by Wholething |
| X @Chorodev | Connected | None for connection; publishing was not tested |
| Facebook Choro | Connected | Page ID `61594239040077`; publishing was not tested |
| Instagram @choro.dev | Connected via Facebook Business | Publishing was not tested |
| TikTok | Developer app draft prepared; not connected | Public policy URLs, accepted domain verification, review demo and platform approval |
| Reddit | Not connected | Reddit Data API / Responsible Builder approval required before app credentials can be issued |
| LinkedIn | On hold | Owner previously chose to wait until launch |

[Verified four-channel dashboard](previews/wholething-four-channels-20260922.png).

## Configuration completed

The official [Postiz provider guide](https://docs.postiz.com/self-host/providers/overview) explains that social providers require developer apps and server environment variables, even though unconfigured providers still appear in Add Channel. The [configuration reference](https://docs.postiz.com/self-host/configuration/reference) requires restarting the application after changes.

- Found the existing DigitalOcean `wholething-production` deployment at `/opt/wholething`, serving `https://app.wholething.media`.
- Created a dedicated **Wholething Postiz** X developer app, configured OAuth read/write, Native App and the exact `/integrations/social/x` callback. Backed up the previous environment, installed credentials securely in production and recreated only the app container. Confirmed successful restart and @Chorodev authorization. No credentials are stored in this repository.
- Corrected the existing Meta app **Ritmus Postiz Test**: saved App Domains `wholething.media` and `app.wholething.media`, and production callback paths `/integrations/social/facebook` and `/integrations/social/instagram`. An additional `/integrations/social/instagram-business` entry added during setup remains, but the live Instagram flow uses `/instagram`.
- Linked @choro.dev to the Choro Facebook Page. Meta automatically created a private **Choro Business portfolio**, containing only those two assets, with one person holding full control. Instagram inbox access remains disabled. No OnlinePianist assets were authorized.
- Completed Facebook and Instagram OAuth and independently verified all four channels in Wholething.

## TikTok remaining setup

Created developer access and a **Wholething Postiz** production app draft with the approved Choro icon, Web platform, Login Kit, Content Posting API, Direct Post, the Postiz callback and required scopes. **TikTok credentials have not been installed in production.**

Added TikTok's verification TXT record at apex `@` of `wholething.media` in Hostinger, TTL 3600. Hostinger confirmed the save and public DNS returned the record. TikTok still reported that the signature could not be found on the final check; the cause of its rejection is not confirmed. The record remains in place for the next verification attempt.

To finish, provide Wholething's public Terms of Service and Privacy Policy URLs, obtain TikTok's ownership-verification acceptance, and prepare the required end-to-end demo for app review. The app remains a draft and has not been submitted. See the [Postiz TikTok guide](https://docs.postiz.com/self-host/providers/tiktok).

## Reddit remaining setup

Reddit's live legacy-app registration flow required Data API / Responsible Builder approval. No app credentials were issued and no request or support ticket was submitted. Any access request must accurately describe the intended use; a moderation use case must not be invented to obtain access. The intended community is **r/chorodev**, not the earlier private r/usechoro.

No posts, schedules, messages, follows, upgrades or LinkedIn changes were made during this integration task. Existing content and connections were preserved.
