# Keep dictation and agent activity visible together

Agent activity is independent of Ottid's own capture, transcription and command
cards. Keep every active session card visible while dictating, with session names
and project labels from PR #40. An approval request also keeps live dictation
visible instead of replacing it.

The foreground group stays nearest the creature, both above and below it. With
both groups present, reserve 60% of the island's available height for foreground
cards and 40% for the scrollable session list (capped at 180px). Each group stays
in normal layout flow; neither overlaps the other. Long dictation retains its
newest lines. Approval overflow is keyboard-accessible and scrollable within its
own region, including when another foreground card is present. Existing agent
visibility preferences still control activity.

Regression checks render Hebrew dictation, multiple named sessions and an
approval together for both placements. Browser layout verification with eight
sessions and long Hebrew dictation in a 300px island: no session/text overlap,
session region 120px, dictation 82px, total island 300px. Screenshot inspection
also checks that approval content stays clipped inside its scrollable region.
The temporary browser fixture is removed before building the real app.
