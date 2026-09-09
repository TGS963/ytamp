use super::*;

impl WinampShell {
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts a skin on. Its textures are dropped, so the next frame
    /// rebuilds them from the new bitmaps. `App::reduce` calls this
    /// directly on `Action::SkinLoaded(Ok(_))`, since the decoded
    /// skin never passes through `State`.
    pub fn wear(&mut self, skin: Arc<Skin>) {
        self.skin = skin;
        self.textures.clear();
    }

    /// The skin's sheets as textures for `ctx`, made now if they are
    /// not yet, and remade if `ctx` is a different viewport than last
    /// time.
    pub(super) fn textures(&mut self, ctx: &egui::Context) -> HashMap<Sheet, egui::TextureId> {
        if self.texture_ctx.as_ref() != Some(ctx) {
            self.textures.clear();
            // The playlist's own textures belong to the same context.
            self.playlist_text.clear();
            self.texture_ctx = Some(ctx.clone());
        }
        for sheet in Sheet::ALL {
            if self.textures.contains_key(&sheet) {
                continue;
            }
            let bitmap = self.skin.sheet(sheet);
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [bitmap.width as usize, bitmap.height as usize],
                &bitmap.rgba,
            );
            let handle = ctx.load_texture(
                format!("winamp-{}", sheet.file_stem()),
                image,
                egui::TextureOptions::NEAREST,
            );
            self.textures.insert(sheet, handle);
        }
        self.textures
            .iter()
            .map(|(sheet, handle)| (*sheet, handle.id()))
            .collect()
    }

    /// The marquee's window onto `text` at `now`: the text itself
    /// when it fits, otherwise 31 characters that step one character
    /// at a time. A text change restarts the scroll from the top.
    pub(super) fn marquee(&mut self, text: &str, now: f64) -> String {
        if text != self.marquee_text {
            self.marquee_text = text.to_string();
            self.marquee_cursor = 0;
            self.marquee_last_step = now;
        }
        if !marquee_scrolls(&self.marquee_text) {
            return self.marquee_text.clone();
        }
        let steps = marquee_steps_since(now, self.marquee_last_step);
        self.marquee_cursor = self.marquee_cursor.wrapping_add(steps);
        self.marquee_last_step += steps as f64 * MARQUEE_STEP_SECS;
        marquee_window(&self.marquee_text, self.marquee_cursor)
    }

    /// The notice to show in the marquee, if a new one has arrived or
    /// one shown recently is still within its few seconds on screen.
    pub(super) fn current_notice(&mut self, notices: &[String], now: f64) -> Option<String> {
        let latest = notices.last().map(String::as_str);
        match (&self.shown_notice, latest) {
            (Some((shown, _)), Some(latest)) if shown == latest => {}
            (_, Some(latest)) => self.shown_notice = Some((latest.to_string(), now)),
            (_, None) => {}
        }
        let (text, started) = self.shown_notice.as_ref()?;
        (now - started < NOTICE_DURATION_SECS).then(|| text.clone())
    }

    /// The whole stack's height in skin pixels: the main window, plus
    /// the playlist window under it while it is open.
    pub fn stack_height(&self) -> u32 {
        stack_height(
            self.shade,
            self.playlist_open,
            self.playlist_shade,
            self.playlist_height,
        ) + if self.equalizer_open {
            if self.equalizer_shade {
                layout::EQ_SHADE_HEIGHT
            } else {
                layout::EQ_HEIGHT
            }
        } else {
            0
        }
    }
}
