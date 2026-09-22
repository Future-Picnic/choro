# Liran 3 voice auditions

Audio assets for [the audition page](../liran3-voice-options.html). All five takes use the same spoken script, ElevenLabs voice **Liran 3** (`cCsdUv3MQ3CbtyEEn4lM`), and model `eleven_v3`. Shared settings: similarity `0.75`, style `0`, speaker boost enabled.

| File | Direction | Speed | Stability |
| --- | --- | --- | --- |
| `natural.mp3` | No delivery tags; original sample | 1.00 | 0.5 |
| `relaxed.mp3` | Conversational, relaxed | 1.00 | 0.5 |
| `upbeat.mp3` | Excited | 1.05 | 0.5 |
| `calm.mp3` | Calm | 0.93 | 1.0 |
| `american.mp3` | American accent, conversational, relaxed | 1.00 | 0.5 |

Direction tags guide generation; they do not guarantee an accent or a particular performance.

## Provenance

The original natural sample is `/Users/lirangabai/Movies/Choro Tutorials/liran3-voice-test-20260917/tts_Hi,_i_20260917_175444.mp3`.

The four additional takes were generated in these source folders, respectively:

- `/Users/lirangabai/Movies/Choro Tutorials/liran3-five-options-20260917/relaxed/`
- `/Users/lirangabai/Movies/Choro Tutorials/liran3-five-options-20260917/upbeat/`
- `/Users/lirangabai/Movies/Choro Tutorials/liran3-five-options-20260917/calm/`
- `/Users/lirangabai/Movies/Choro Tutorials/liran3-five-options-20260917/american/`

## Use and design

Keep this entire `liran3-voice-audio` folder alongside `liran3-voice-options.html` when sharing. Open the HTML to compare samples and mark a favorite. Playback pauses the other samples. Selection exists only in the current page: no local storage or backend is used, and the choice is not transmitted.

The page extends [the existing video library](../how-to-video-library.html): dark slate (`#0c0e12`), lavender (`#b7a7ed`), system UI typography, and compact rows inside a rounded group. Below 760px, audio moves beneath each row's title and selection control. This is a scoped surface within the existing visual system; it does not require changes to root `DESIGN.md`.

## Verification scope

The code-only finish review verdict was **ship**: all five audio files decoded, and JavaScript playback, selection, and error handlers were tested. Browser visual checks and subjective listening were not performed; the review does not assess voice likeness or accent quality.
