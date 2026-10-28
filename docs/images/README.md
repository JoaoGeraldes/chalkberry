# Landing page images

Media for the landing page. If a file is missing, the page shows a dashed
box with its name, so nothing breaks.

| File | Section | What to show |
|---|---|---|
| `hero.webm` | Top of the page | Short muted loop of Chalkberry in use. Plays automatically. |
| `draw.webm` | Draw over anything | Lines in several colours, thicknesses and opacities over an app. |
| `erase.webm` | Erase one line | The crosshair cursor over a line being removed. |
| `toolbar.webm` | Toolbar | The toolbar close up, with the colour row open and a tooltip showing. |
| `screenshot.webm` | Screenshot your drawings | The crop screen: blue box and the save panel at the bottom. |
| `social.png` | Link previews (not on the page) | 1200 × 630 px image shown when the link is shared in Slack, LinkedIn, X, etc. |

Videos keep their own shape: if you replace one with a different size, update
`aspect-ratio` on its `<figure>` in `index.html` (width / height in px).

Keep videos light: WebM (VP9), muted, under ~2 MB. GitHub allows files up to
100 MB, but every visitor downloads the video on page load.
