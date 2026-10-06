# Complete the visible Ottid branding

The product rename left the old tongue artwork in the app, desktop icons and
the public site. Replace it with Ottid's puddle, hands, eyes and inner lamp.
Keep one canonical SVG, imported by the tinted Svelte component and used to
generate the desktop icons, including the tray. Only the lamp changes colour.

The site rename and MCP guide are merged on `gh-pages`; its logo uses the same
SVG and the obsolete "coming to the app" badge is removed in both languages.

Legacy install migration paths, previous release names, ADR history and the
`hey_lashon` model remain deliberate. The available wake classifier still
recognizes "Hey Lashon". Training for "Hey Ottid" has not produced a usable
classifier; changing its label or filename would misrepresent its behavior.

Validation: Svelte check, frontend tests and production build; regenerated
icons inspected visually. Native Windows build and PR CI must also pass.
