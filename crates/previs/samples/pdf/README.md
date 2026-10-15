# The PDF corpus

Screenplay PDFs the `pdf` module is tested against — one script through
several generators, since every tool sets the columns a few points apart and
embeds its fonts its own way. All from this crate's own text; nothing here is
anyone's script.

| File | Made with | What it exercises |
|---|---|---|
| `../cold-eggs.pdf` (the page's sample) | [screenplain](https://github.com/vilcans/screenplain) 0.11 (ReportLab) from `cold-eggs-unwrapped.fountain` — the sample with each paragraph on one line, so the generator wraps it at its own column | Courier Prime as WinAnsi TrueType; a title page; the same board as the other three samples, byte for byte |
| `cold-eggs-afterwriting.pdf` | [afterwriting](https://afterwriting.com) CLI (PDFKit) from `../cold-eggs.fountain` | a CID font (Identity-H with ToUnicode); the source's own line breaks kept |
| `cold-eggs-chromium.pdf` | screenplain's HTML printed by headless Chromium (`print-chromium.mjs`, Playwright) | Type 3 fonts (Skia); narrower columns, 9 pt |
| `cold-eggs-cups.pdf` | `cupsfilter` on the Fountain file | one column: the Fountain text printed as it is, with padded lines |
| `cold-eggs-soffice.pdf` | LibreOffice Writer, `--convert-to pdf`, on the Fountain file renamed `.txt` | one column, Liberation Mono 10 pt |
| `cold-eggs-scan.pdf` | page one of the screenplain PDF rendered to PNG (`pdftoppm -r 150`), wrapped by `cupsfilter` | no text layer — refused as a scan |
| `gull-point-screenplain.pdf` | screenplain from `gull-point.fountain` | ten numbered scenes, scene numbers in both margins, page numbers, `CUT TO:`, dual dialogue set side by side, a centered line, lyrics |
| `gull-point-afterwriting.pdf` | afterwriting from `gull-point.fountain` | the same ten scenes without margin numbers; lines drawn twice on a page |

`gull-point.fountain` is a synthetic script (a seeded generator wrote it) long
enough to break across pages. To regenerate a file, run the tool named above
on the Fountain source; the tests pin element counts and the sample's bytes,
not the PDFs' bytes.
