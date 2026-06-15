use gpui::*;
use gpui::prelude::FluentBuilder;
use crate::ui::{layout, home, command};
use crate::core::state::{AppView, ProjectMeta, Settings};
use crate::core::project::{MediaItem, MediaType, Sequence, Clip, TrackKind};
use crate::core::persistence;
use std::path::PathBuf;
use std::time::SystemTime;

pub struct VelocisApp {
    pub view: AppView,
    pub recent_projects: Vec<ProjectMeta>,
    #[expect(dead_code, reason = "reserved for settings panel")]
    pub settings: Settings,
    pub command_palette_open: bool,
    pub media_items: Vec<MediaItem>,
    pub sequence: Sequence,
    pub selected_clip_id: Option<String>,
    pub playhead_position: f64,
    pub show_new_project_dialog: bool,
    pub new_project_name: String,
    pub playing: bool,
    pub current_project_id: String,
    pub playback_task: Option<gpui::Task<()>>,
}

impl VelocisApp {
    pub fn new() -> Self {
        let projects = persistence::load_recent_projects();
        let view = if projects.is_empty() { AppView::Home } else { AppView::Editor };
        let pid = if projects.is_empty() { String::new() } else { projects[0].id.clone() };
        let mut app = Self {
            view,
            recent_projects: projects,
            settings: Settings::default(),
            command_palette_open: false,
            media_items: vec![],
            sequence: Sequence::new("Sequence 1"),
            selected_clip_id: None,
            playhead_position: 0.0,
            show_new_project_dialog: false,
            new_project_name: String::new(),
            playing: false,
            current_project_id: pid.clone(),
            playback_task: None,
        };
        if !pid.is_empty()
            && let Some((media, seq, playhead)) = persistence::load_project_data(&pid)
        {
            app.media_items = media;
            app.sequence = seq;
            app.playhead_position = playhead;
        }
        app
    }

    pub fn save_current_project(&self) {
        persistence::save_project_data(&self.current_project_id, &self.media_items, &self.sequence, self.playhead_position);
    }

    pub fn toggle_command_palette(&mut self) {
        self.command_palette_open = !self.command_palette_open;
    }

    pub fn start_new_project(&mut self) {
        self.show_new_project_dialog = true;
        self.new_project_name = String::new();
    }

    fn add_file(&mut self, path: PathBuf) {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("fichier").to_string();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let media_type = match ext.as_str() {
            "mp4" | "mov" | "avi" | "mkv" | "webm" => MediaType::Video,
            "mp3" | "wav" | "flac" | "aac" | "ogg" => MediaType::Audio,
            "png" | "jpg" | "jpeg" | "gif" | "webp" => MediaType::Image,
            _ => MediaType::Other,
        };
        let duration = match media_type {
            MediaType::Video => 10.0, MediaType::Audio => 30.0,
            MediaType::Image => 5.0, MediaType::Other => 3.0,
        };
        let mut item = MediaItem::new(&name, media_type);
        item.path = path;
        item.duration_seconds = duration;
        self.media_items.push(item);
    }

    fn nanoid() -> String {
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos().to_string()).unwrap_or_default()
    }

    pub fn confirm_new_project(&mut self, cx: &mut Context<Self>) {
        let name = if self.new_project_name.is_empty() { "Sans titre".to_string() } else { self.new_project_name.clone() };
        let id = format!("proj_{}", Self::nanoid());
        self.current_project_id = id.clone();
        self.recent_projects.insert(0, ProjectMeta {
            id, name, path: PathBuf::new(), last_opened: SystemTime::now(),
        });
        persistence::save_recent_projects(&self.recent_projects);
        self.media_items = vec![];
        self.sequence = Sequence::new("Sequence 1");
        self.selected_clip_id = None;
        self.playhead_position = 0.0;
        self.show_new_project_dialog = false;
        self.view = AppView::Editor;
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true, multiple: true, directories: false,
            prompt: Some("Choisissez des fichiers media".into()),
        });
        let weak = cx.weak_entity();
        cx.spawn(move |_weak, async_app: &mut gpui::AsyncApp| {
            let mut async_app = async_app.clone();
            async move {
                let Ok(Ok(Some(paths))) = receiver.await else { return };
                let Some(entity) = weak.upgrade() else { return };
                if let Err(e) = async_app.update_entity(&entity, |app, cx| {
                    for path in paths { app.add_file(path); }
                    app.save_current_project();
                    cx.notify();
                }) {
                    eprintln!("confirm_new_project update error: {e}");
                }
            }
        }).detach();
    }

    pub fn import_media(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true, multiple: true, directories: false,
            prompt: Some("Choisissez des fichiers media".into()),
        });
        let weak = cx.weak_entity();
        cx.spawn(move |_weak, async_app: &mut gpui::AsyncApp| {
            let mut async_app = async_app.clone();
            async move {
                let Ok(Ok(Some(paths))) = receiver.await else { return };
                let Some(entity) = weak.upgrade() else { return };
                if let Err(e) = async_app.update_entity(&entity, |app, cx| {
                    for path in paths { app.add_file(path); }
                    app.save_current_project();
                    cx.notify();
                }) {
                    eprintln!("import_media update error: {e}");
                }
            }
        }).detach();
    }

    pub fn open_project(&mut self, id: &str) {
        self.current_project_id = id.to_string();
        if let Some((media, seq, playhead)) = persistence::load_project_data(id) {
            self.media_items = media;
            self.sequence = seq;
            self.playhead_position = playhead;
        } else {
            self.media_items = vec![];
            self.sequence = Sequence::new("Sequence 1");
            self.playhead_position = 0.0;
        }
        self.selected_clip_id = None;
        self.playing = false;
        self.view = AppView::Editor;
    }

    pub fn current_project_name(&self) -> String {
        self.recent_projects.iter().find(|p| p.id == self.current_project_id).map(|p| p.name.clone()).unwrap_or_default()
    }

    pub fn go_home(&mut self) {
        self.save_current_project();
        self.command_palette_open = false;
        self.view = AppView::Home;
    }

    pub fn toggle_play(&mut self, cx: &mut Context<Self>) {
        self.playing = !self.playing;
        if self.playing {
            self.start_playback_tick(cx);
        }
    }

    pub fn stop_playback(&mut self) {
        self.playing = false;
        self.playhead_position = 0.0;
        self.playback_task = None;
    }

    fn start_playback_tick(&mut self, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        self.playback_task = Some(cx.spawn(move |_weak, async_app: &mut AsyncApp| {
            let mut async_app = async_app.clone();
            async move {
                loop {
                    Timer::after(std::time::Duration::from_millis(100)).await;
                    let Some(entity) = weak.upgrade() else { break };
                    if let Err(e) = async_app.update_entity(&entity, |app, cx| {
                        if !app.playing {
                            app.playback_task = None;
                            cx.notify();
                            return;
                        }
                        app.playhead_position += 0.1;
                        let max_pos = app.sequence.tracks.iter()
                            .flat_map(|t| t.clips.iter())
                            .map(|c| c.position + c.duration)
                            .fold(0.0, f64::max);
                        if app.playhead_position > max_pos + 1.0 && max_pos > 0.0 {
                            app.playhead_position = 0.0;
                            app.playing = false;
                            app.playback_task = None;
                        }
                        cx.notify();
                    }) {
                        eprintln!("playback tick error: {e}");
                        break;
                    }
                }
            }
        }));
    }

    pub fn seek_to(&mut self, pos_secs: f64) {
        let max_dur = self.sequence.tracks.iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.position + c.duration)
            .fold(0.0, f64::max)
            .max(0.0);
        let clamped = pos_secs.clamp(0.0, if max_dur > 0.0 { max_dur } else { 100.0 });
        self.playhead_position = clamped;
    }

    pub fn add_media_to_timeline(&mut self, media_idx: usize) {
        if media_idx >= self.media_items.len() { return; }
        let item = &self.media_items[media_idx];
        for track in &mut self.sequence.tracks {
            let fits = matches!((&track.kind, &item.media_type),
                (TrackKind::Video, MediaType::Video | MediaType::Image)
                | (TrackKind::Audio, MediaType::Audio)
                | (TrackKind::Text, _));
            if fits {
                let pos = track.clips.iter().map(|c| c.position + c.duration).fold(0.0, f64::max);
                let (h, s, l) = match item.media_type {
                    MediaType::Video => (0.08, 0.8, 0.35),
                    MediaType::Audio => (0.55, 0.6, 0.35),
                    MediaType::Image => (0.15, 0.5, 0.35),
                    MediaType::Other => (0.0, 0.0, 0.35),
                };
                track.clips.push(Clip {
                    id: Self::nanoid(),
                    media_id: item.id.clone(), name: item.name.clone(),
                    start_offset: 0.0, duration: item.duration_seconds.max(3.0), position: pos, color: (h, s, l),
                });
                self.save_current_project();
                return;
            }
        }
    }

    pub fn select_clip(&mut self, clip_id: &str) {
        self.selected_clip_id = Some(clip_id.to_string());
        for track in &self.sequence.tracks {
            for clip in &track.clips {
                if clip.id == clip_id {
                    self.playhead_position = clip.position;
                    return;
                }
            }
        }
    }
}

impl Render for VelocisApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cmd_open = self.command_palette_open;
        let cmd_close = cx.listener(|this: &mut Self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
            this.command_palette_open = false; cx.notify();
        });

        let content: gpui::AnyElement = match &self.view {
            AppView::Home => {
                let on_new = cx.listener(|this: &mut Self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                    this.start_new_project(); cx.notify();
                });
                let on_open = cx.listener(|this: &mut Self, id: &String, _: &mut Window, cx: &mut Context<Self>| {
                    this.open_project(id); cx.notify();
                });
                home::home_screen(&self.recent_projects, on_new, on_open).into_any_element()
            }
            AppView::Editor => {
                let on_home = cx.listener(|this: &mut Self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                    this.go_home(); cx.notify();
                });
                let on_import = cx.listener(|this: &mut Self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                    this.import_media(cx); cx.notify();
                });
                let on_media_click = cx.listener(|this: &mut Self, idx: &usize, _: &mut Window, cx: &mut Context<Self>| {
                    this.add_media_to_timeline(*idx); cx.notify();
                });
                let on_clip_select = cx.listener(|this: &mut Self, id: &String, _: &mut Window, cx: &mut Context<Self>| {
                    this.select_clip(id); cx.notify();
                });
                let on_toggle = cx.listener(|this: &mut Self, _: &ClickEvent, _window: &mut Window, cx: &mut Context<Self>| {
                    this.toggle_play(cx); cx.notify();
                });
                let on_stop = cx.listener(|this: &mut Self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                    this.stop_playback(); cx.notify();
                });
                let on_seek = cx.listener(|this: &mut Self, pos: &f64, _: &mut Window, cx: &mut Context<Self>| {
                    this.seek_to(*pos); cx.notify();
                });
                let pname = self.current_project_name();
                layout::editor_layout(&pname, &self.media_items, &self.sequence, &self.selected_clip_id,
                    self.playhead_position, self.playing, 0.0,
                    on_home, on_import, on_media_click, on_clip_select, on_toggle, on_stop, on_seek).into_any_element()
            }
        };

        div().size_full().relative()
            .child(content)
            .child(command::command_palette(cmd_open, cmd_close))
            .child(new_project_dialog(self, _window, cx))
            .into_any_element()
    }
}

fn new_project_dialog(
    app: &mut VelocisApp,
    _window: &mut Window,
    cx: &mut Context<VelocisApp>,
) -> impl IntoElement {
    if !app.show_new_project_dialog { return div().into_any_element(); }
    let focus = cx.focus_handle();
    let name = app.new_project_name.clone();
    let on_key = cx.listener(|this: &mut VelocisApp, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<VelocisApp>| {
        match event.keystroke.key.as_ref() {
            "enter" | "return" => { this.confirm_new_project(cx); }
            "backspace" => { this.new_project_name.pop(); }
            "escape" => { this.show_new_project_dialog = false; }
            key if key.len() == 1 => { this.new_project_name.push_str(key); }
            _ => {}
        }
        cx.notify();
    });
    let on_confirm = cx.listener(|this: &mut VelocisApp, _: &ClickEvent, _: &mut Window, cx: &mut Context<VelocisApp>| {
        this.confirm_new_project(cx);
    });
    let on_cancel = cx.listener(|this: &mut VelocisApp, _: &ClickEvent, _: &mut Window, cx: &mut Context<VelocisApp>| {
        this.show_new_project_dialog = false; cx.notify();
    });
    div().id("np-overlay").absolute().inset_0()
        .flex().items_center().justify_center()
        .bg(hsla(0.0, 0.0, 0.0, 0.6))
        .on_click(cx.listener(|this: &mut VelocisApp, _: &ClickEvent, _: &mut Window, cx: &mut Context<VelocisApp>| {
            this.show_new_project_dialog = false; cx.notify();
        }))
        .child(div()
            .track_focus(&focus).on_key_down(on_key)
            .w(px(420.0)).bg(crate::ui::theme::panel()).rounded_lg()
            .border_1().border_color(crate::ui::theme::border()).shadow_lg()
            .child(div().h(px(48.0)).px_4().flex().items_center()
                .bg(crate::ui::theme::surface()).border_b_1().border_color(crate::ui::theme::border()).rounded_t_lg()
                .child(div().font_family("Lexend").text_color(crate::ui::theme::text_primary()).text_size(px(14.0)).font_weight(FontWeight::SEMIBOLD).child("Nouveau projet")))
            .child(div().flex().flex_col().p_4().gap_4()
                .child(div().flex().flex_col().gap_2()
                    .child(div().font_family("Lexend").text_color(crate::ui::theme::text_secondary()).text_size(px(11.0)).child("Nom du projet"))
                    .child(div().h(px(36.0)).px_3().flex().items_center()
                        .bg(crate::ui::theme::background()).rounded_md().border_1()
                        .when(name.is_empty(), |d: Div| d.border_color(crate::ui::theme::border()))
                        .when(!name.is_empty(), |d: Div| d.border_color(crate::ui::theme::orange()))
                        .child(div().font_family("Lexend").text_size(px(13.0))
                            .text_color(crate::ui::theme::text_muted()).when(!name.is_empty(), |d: Div| d.text_color(crate::ui::theme::text_primary()))
                            .child(if name.is_empty() { SharedString::from("Mon projet") } else { SharedString::from(name) }))))
                .child(div().flex().flex_row().justify_end().gap_2()
                    .child(div().id("cancel-btn").px_4().py_1p5().rounded_md().cursor_pointer()
                        .on_click(on_cancel)
                        .child(div().font_family("Lexend").text_color(crate::ui::theme::text_secondary()).text_size(px(12.0)).child("Annuler")))
                    .child(div().id("confirm-btn").px_4().py_1p5().bg(crate::ui::theme::orange()).rounded_md().cursor_pointer()
                        .on_click(on_confirm)
                        .child(div().font_family("Lexend").text_color(rgb(0xFFFFFF)).text_size(px(12.0)).font_weight(FontWeight::SEMIBOLD).child("Creer"))))))
        .into_any_element()
}
