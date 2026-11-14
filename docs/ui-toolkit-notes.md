# UI toolkit notes: Arabic in egui 0.37

PhotoVault's primary user writes Arabic, so text rendering was investigated
in depth. These are the findings and the solution used.

## What egui 0.37 does by itself

* **Shaping: yes.** `epaint` shapes each text run with
  [`harfrust`](https://crates.io/crates/harfrust) (a Rust port of HarfBuzz),
  so Arabic letters join correctly.
* **Runs.** A line is split into runs **wherever the font changes**. Each run
  is shaped with `guess_segment_properties()`, so a run of Arabic script is
  shaped right-to-left (its glyphs come out reversed). Runs are then placed
  left to right. Adjacent `LayoutJob` sections with identical formats are
  merged first.
* **Bidi: no.** There is no Unicode Bidirectional Algorithm and no
  right-alignment of RTL paragraphs.

These findings were measured, not guessed: the dev view
`PHOTOVAULT_VIEW=bidi` and glyph x-positions from egui galleys were used to
compare strategies.

Consequences without help:
- Digits inside Arabic text are reversed (`2025` shows as `5202`) when the
  Arabic font also contains digits (Noto Sans Arabic does).
- Mixed Arabic/Latin lines show their runs in the wrong order.
- Wrapped RTL paragraphs break into lines in the wrong order.

## The solution (`src/bidi.rs`, `src/ui/bidi.rs`)

1. **Display fonts are Latin-first** (`Noto Sans`, then `Noto Sans Arabic`).
   Every maximal sequence of Arabic-script characters therefore becomes its
   own egui run, and only those runs get reversed by the shaper. Spaces,
   digits and Latin text stay left-to-right.
2. **Visual reordering.** `visual_line` applies UBA rules L1–L4 with
   `unicode-bidi`: the line is put into full visual order (with bracket
   mirroring), then each Arabic-script sequence is reversed back so that the
   shaper's own reversal restores it. The resulting string renders correctly
   in *any* egui widget (labels, buttons, painter text). This is verified in
   unit tests against a reference UBA rendering.
3. **Isolates.** `t!("key", value = …)` wraps each inserted value in
   FSI…PDI, so `93.1 KB`, file names or titles keep their own direction
   inside a translated sentence.
4. **Wrapping.** Long text is wrapped word by word from the *logical* text
   and every row is then reordered with its paragraph's bidi levels
   (`paragraphs`, `para`). RTL paragraphs are right-aligned. Translated
   strings remember their logical original, so `t!()` output can be wrapped
   correctly too.
5. **Editing.** Text fields use an **Arabic-first** `editor` font family, so a
   sentence typed in Arabic is one RTL run and reads correctly while typing,
   and fields with RTL content are right-aligned. Arabic notes and titles are
   shown in a bidi-correct reading view and switch to the editor on click.
6. **RTL layout.** With the Arabic UI language the layout mirrors: the
   sidebar moves to the right, rows run right-to-left (`ui::row`), columns are
   right-aligned, and the grid, viewer arrows, segmented controls and toggles
   flip.

## Remaining limitations

| Situation | Behaviour |
|---|---|
| Display of any text (pure, mixed, numbers, brackets) | ✅ correct order, RTL paragraphs right-aligned |
| Typing pure Arabic | ✅ correct while typing |
| Typing a line that mixes Arabic with digits or Latin | ⚠️ shown out of order **while editing** only; the reading view is correct |
| Caret movement inside mixed RTL/LTR text | ⚠️ logical order |
| Explicit bidi controls typed by the user | stored as typed, hidden on display |

The data is always stored exactly as typed (UTF-8, logical order).

## Alternatives evaluated

| Toolkit | Arabic/bidi | Trade-offs |
|---|---|---|
| **egui 0.37 + PhotoVault bidi layer (chosen)** | display ✅, editing mostly ✅ | Pure Rust, one small binary, immediate mode. |
| **Slint** | text layout with shaping; RTL support was still maturing at the time of writing | Declarative UI; would require rewriting `src/ui`. |
| **Tauri v2** | full browser text engine: shaping, bidi and RTL editing ✅ | System webview; needs a web frontend; Linux depends on WebKitGTK. |

If perfect mixed-direction *editing* becomes essential, the best upgrade is a
Tauri v2 frontend over the unchanged `photovault` library crate. Native bidi
support in egui would also remove the editing caveat.
