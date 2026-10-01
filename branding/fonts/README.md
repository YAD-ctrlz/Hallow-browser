# Hallow corporate fonts

Bundled into `/usr/lib/hallow/fonts`, where Gecko loads them for the browser
only (they are not installed system-wide).

| File | Family | Used for | Source | License |
| --- | --- | --- | --- | --- |
| `IBMPlexSans-Variable.ttf`, `IBMPlexSans-Italic-Variable.ttf` | IBM Plex Sans | browser interface: menus, tabs, toolbars, Settings (`patches/0004`) | [google/fonts `ofl/ibmplexsans`](https://github.com/google/fonts/tree/main/ofl/ibmplexsans) | SIL OFL 1.1, `OFL-IBMPlexSans.txt` |
| `SpaceGrotesk-Variable.ttf` | Space Grotesk | home / new tab page (`ui/hallow-home.css`) and the wordmark | [google/fonts `ofl/spacegrotesk`](https://github.com/google/fonts/tree/main/ofl/spacegrotesk) | SIL OFL 1.1, `OFL-SpaceGrotesk.txt` |

`branding/wordmark.svg` is "Hallow" set in Space Grotesk Bold and converted to
outlines, so it renders without the font.
