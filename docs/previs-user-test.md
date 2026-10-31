# Previs user test — the observation sheet

**Status:** the protocol for `world-strategy.md` §15.4, written 2026-10-14.
Five to ten sessions; this sheet is what makes them comparable. Nothing is
measured by the page or the app — the page uploads nothing, and the app's
history records edits, not people — so the measurements are taken by
watching, with a stopwatch and this sheet.

## Who, and with what

People who previs today — indie directors, ADs, film students, a DP if one
will sit — each with **their own script pages**: a Final Draft file (`.fdx`),
Fountain, or Markdown. (A PDF is the one file the page can't read yet;
`todo/TODO-previs-pdf.md`. Ask for the `.fdx` the day before.) One scene of
theirs; a second only if they ask for it. Not our sample: the measurement is
the gap between *their* pages and a board they'd show someone.

## The two steps under test

1. **Script → board** — localgpt.md in a browser tab, or `localgpt-previs`
   on the command line.
2. **Revising shots** — the zip unzipped and one scene opened live
   (`localgpt-gen --live scene-N.world`), the shot strip, look-through
   (1–9), revisions by asking through an agent on the folder (`localgpt
   world mcp`), the board (`GET /board`) and two treatments compared
   (`?vs=`). `website/content/docs/gen/previs.md` is the walk.

Posing, animatics and the generated look are **not** under test. If they
come up, write down the words and move on.

## Setup (before they arrive)

- A machine with the app built, a terminal open, the page open in a tab, and
  the agent configured and checked once (ask it for the shot list of a
  sample scene; it should answer from `GET /world`).
- Their file received the day before, so a parser failure is found before
  the session, not during it — and written down as a finding either way.
- A stopwatch. One sheet per person, on paper or in a text file outside the
  repository: the sheet names a person.

## What you say

"Here's the page. Get a board for one scene of your script, and make it
something you'd show your DP. I'll answer questions about the tool, not
about the shots." Then nothing, until they've been stuck for a full minute.

## The sheet (one per person)

| Measure | How | Why it's the number |
|---|---|---|
| **T1 — minutes to a first board** | stopwatch from the file dropped (or the command typed) to the board on screen | §15.4's first measure |
| **T2 — minutes to "I'd show this"** | from the first board to the moment they'd show it to the DP (ask every few minutes: "would you show this yet?") | the real cost of step 2 |
| **R — revisions until then** | every ask to the agent, and every drag in the strip, counted separately | §15.4's second measure |
| **Failed asks** | an ask the agent got wrong or refused — the exact words, and what it did | the prompt and `AGENTS.md` are the fix |
| **Numbers read** | did they read a lens, a distance, a camera height — or only the frames? | the measured-plan position (§15.2) is worth holding only if someone reads the measurements |
| **Second scene?** | started one unasked / asked to / stopped | §15.4's pass or fail |
| **What they'd bring back** | the one thing they said they'd want next time, verbatim | the backlog, in their words |

Also note: their writing tool (Final Draft, Highland, WriterDuet, Word), the
scene's shape (speakers, lines, `INT.`/`EXT.`), the machine, and whether the
stand-ins' look came up — §15.1 claims the greybox isn't a handicap in
previs; this is where that claim is checked.

## Afterwards (ten minutes)

Four questions, answers verbatim:

1. "What did the board get wrong about your scene?"
2. "What did you have to do that you shouldn't have had to?"
3. "Would you use this for the next thing you shoot? For what?"
4. "Would you want any of this on a phone or an iPad: on set, on a scout,
   away from the desk? To do what?" (added 2026-10-31, for
   `rfcs/authoring/mobile-remote.md`)

## Reading the sheets

- Median T1 over a few minutes, or any parser failure on a real file → step
  1 is the problem: importers, headings, encodings.
- Median R past half a dozen, or T2 past twenty minutes → step 2 is the
  problem: the agent's revisions, the strip, how the board reads.
- Fewer than half start a second scene → per §15.4, the problem is in these
  two steps, not in posing or animatics; those keep waiting.
- Nobody reads a number → the measured plan needs a different surface (the
  shot list on the board, a recce sheet), not more numbers.
- Anyone wants it on an iPad or a phone, for a set or a scout → steps 3 and
  4 of [mobile-remote.md](rfcs/authoring/mobile-remote.md) have a user;
  nobody does → they keep waiting.

Publish the counts and the verbatim answers, with names off; keep the sheets.
