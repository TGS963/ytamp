# Third-party code and assets

## fastpotify

The Winamp skin layer of ytamp is a port of the skin code of fastpotify
(github.com/crmne/fastpotify), used under the MIT license below.

```
MIT License

Copyright (c) 2026 Carmine Paolino

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### Ported files

Each file below carries a header comment that names this notice.

- `src/skin/zip.rs`, ported from `src/skin/zip.rs` of fastpotify
- `src/skin/sprites.rs`, ported from `src/skin/sprites.rs` of fastpotify
- `src/skin/layout.rs`, ported from `src/skin/layout.rs` of fastpotify
- `src/skin/font.rs`, ported from `src/skin/font.rs` of fastpotify
- `src/skin/config.rs`, ported from `src/skin/config.rs` of fastpotify
- `src/skin/mod.rs`, ported from `src/skin/mod.rs` of fastpotify
- `examples/default_skin.rs`, ported from `examples/default_skin.rs` of
  fastpotify
- `src/ui/winamp/view.rs`, ported from `src/ui/winamp/mod.rs` of
  fastpotify (the `View` blitter)
- `src/ui/winamp/mod.rs`, ported in part from `src/ui/winamp/mod.rs`
  and `src/winamp.rs` of fastpotify (sprite drawing, the marquee, and
  the skin picker menu)
- `src/ui/winamp/playlist.rs`, ported from `src/ui/winamp/playlist.rs`
  of fastpotify (the playlist window)
- `src/ui/winamp/pixel_text.rs`, ported from
  `src/ui/winamp/pixel_text.rs` of fastpotify, without its
  `system_fonts` probe and its bundled emoji face: the playlist shows
  track titles, not arbitrary Spotify text, so the bundled Inter face
  alone is enough

### Bundled skin

The file `assets/skins/builtin.wsz` is the built-in Winamp skin of ytamp.
`examples/default_skin.rs` draws the skin, in the dark and red palette of
ytamp, not the green palette of fastpotify. That generator is a port of
the fastpotify file of the same name. For this reason, the ported file
above still carries the MIT notice, even though it now draws a skin that
belongs to ytamp.

## Inter

`assets/fonts/InterVariable.ttf` draws the Winamp playlist's text
(`src/ui/winamp/pixel_text.rs`), the same file fastpotify bundles.
Inter is licensed under the SIL Open Font License 1.1, copied at
`assets/fonts/OFL.txt`.
