use std::rc::Rc;
use gpui::*;
use crate::ui::theme;
use crate::core::project::{MediaItem, Sequence};

fn find_media_at_playhead<'a>(sequence: &'a Sequence, media_items: &'a [MediaItem], pos: f64) -> Option<(&'a MediaItem, f64)> {
    for track in &sequence.tracks {
        for clip in &track.clips {
            if pos >= clip.position && pos <= clip.position + clip.duration
                && let Some(media) = media_items.iter().find(|m| m.id == clip.media_id)
            {
                return Some((media, clip.duration));
            }
        }
    }
    None
}

fn time_str(secs: f64) -> String {
    format!("{:02}:{:02}", (secs as u64) / 60, (secs as u64) % 60)
}

fn seek_bar_position_to_secs(mouse_x: Pixels, max_dur: f64, window: &Window) -> f64 {
    let win_w: f64 = window.bounds().size.width.into();
    let left_panel: f64 = 240.0;
    let right_panel: f64 = 240.0;
    let transport_pad: f64 = 24.0;
    let left_controls: f64 = 60.0;
    let right_controls: f64 = 160.0;
    let bar_start = left_panel + transport_pad + left_controls;
    let bar_end = win_w - right_panel - transport_pad - right_controls;
    let bar_w = (bar_end - bar_start).max(1.0);
    let x: f64 = mouse_x.into();
    let pct = ((x - bar_start) / bar_w).clamp(0.0, 1.0);
    pct * max_dur
}

fn transport_bar(
    sequence: &Sequence,
    _media_items: &[MediaItem],
    playing: bool,
    playhead_position: f64,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_stop: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_seek: impl Fn(&f64, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_toggle = Rc::new(on_toggle);
    let on_stop = Rc::new(on_stop);
    let on_seek = Rc::new(on_seek);
    let max_dur = sequence.tracks.iter()
        .flat_map(|t| t.clips.iter())
        .map(|c| c.position + c.duration)
        .fold(0.0, f64::max);

    div()
        .h(px(48.0)).px_4()
        .flex().items_center().gap_4()
        .bg(theme::panel())
        .border_t_1().border_color(theme::border())
        .child(
            div().font_family("Lexend").text_color(theme::text_primary()).text_size(px(13.0)).child(time_str(playhead_position)),
        )
        .child({
            let pct = if max_dur > 0.0 { (playhead_position / max_dur * 100.0) as f32 } else { 0.0 };
            let seek_cb = Rc::clone(&on_seek);
            let mdown_seek = Rc::clone(&on_seek);
            div().id("seek-bar").flex_grow().h(px(20.0)).flex().items_center().cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                    let secs = seek_bar_position_to_secs(event.position.x, max_dur, window);
                    (seek_cb)(&secs, window, cx);
                })
                .on_mouse_move(move |event: &MouseMoveEvent, window: &mut Window, cx: &mut App| {
                    if event.dragging() {
                        let secs = seek_bar_position_to_secs(event.position.x, max_dur, window);
                        (mdown_seek)(&secs, window, cx);
                    }
                })
                .child(
                    div().h(px(4.0)).w_full().bg(theme::surface()).rounded_full().relative()
                        .child(div().h_full().w(px(pct * 4.0)).bg(theme::orange()).rounded_full()),
                )
        })
        .child(
            div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(13.0)).child(time_str(max_dur)),
        )
        .child(
            div().flex().items_center().gap_2()
                .child({
                    let cb = Rc::clone(&on_toggle);
                    div().id("play-btn-transport").px_3().py_1p5().bg(theme::orange()).rounded_md().cursor_pointer()
                        .on_click(move |e, w, cx| (cb)(e, w, cx))
                        .child(div().font_family("Lexend").text_color(rgb(0xFFFFFF)).text_size(px(12.0)).font_weight(FontWeight::SEMIBOLD)
                            .child(if playing { "\u{23F8}  Pause" } else { "\u{25B6}  Play" }))
                })
                .child({
                    let cb = Rc::clone(&on_stop);
                    div().id("stop-btn").px_3().py_1p5().bg(theme::surface()).rounded_md().cursor_pointer()
                        .on_click(move |e, w, cx| (cb)(e, w, cx))
                        .child(div().font_family("Lexend").text_color(theme::text_secondary()).text_size(px(12.0)).font_weight(FontWeight::SEMIBOLD).child("\u{23F9}  Stop"))
                }),
        )
}

fn preview_area(
    sequence: &Sequence,
    media_items: &[MediaItem],
    playhead_position: f64,
    current_frame_path: &Option<std::path::PathBuf>,
    current_frame_media_id: &Option<String>,
) -> impl IntoElement {
    let (media, dur) = find_media_at_playhead(sequence, media_items, playhead_position)
        .map_or((None::<&MediaItem>, 0.0), |(m, d)| (Some(m), d));

    div()
        .flex_grow().flex().flex_col().overflow_hidden().bg(theme::background())
        .child(
            div().flex_grow().flex().items_center().justify_center().overflow_hidden().size_full()
                .child(if let Some(media) = media {
                    preview_image(media, dur, current_frame_path, current_frame_media_id)
                } else {
                    div().flex().flex_col().items_center().gap_3()
                        .child(div().font_family("Lexend").text_color(theme::text_secondary()).text_size(px(16.0)).font_weight(FontWeight::SEMIBOLD).child("Lecteur"))
                        .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(12.0)).child("Ajoutez des medias a la timeline pour commencer"))
                        .into_any_element()
                }),
        )
        .child(if let Some(media) = media {
            let type_lbl = match media.media_type {
                crate::core::project::MediaType::Video => "Video",
                crate::core::project::MediaType::Audio => "Audio",
                crate::core::project::MediaType::Image => "Image",
                _ => "Fichier",
            };
            div().h(px(36.0)).flex().items_center().justify_center().bg(theme::panel()).border_t_1().border_color(theme::border())
                .child(div().font_family("Lexend").text_color(theme::text_primary()).text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).child(media.name.clone()))
                .child(div().font_family("Lexend").text_color(theme::text_secondary()).text_size(px(11.0)).px_2().child(format!("{}  ·  {:02}:{:02}", type_lbl, dur as u64 / 60, dur as u64 % 60)))
                .into_any_element()
        } else {
            div().into_any_element()
        })
}

fn preview_image(
    media: &MediaItem,
    _dur: f64,
    current_frame_path: &Option<std::path::PathBuf>,
    current_frame_media_id: &Option<String>,
) -> AnyElement {
    let has_path = !media.path.as_os_str().is_empty();
    match media.media_type {
        crate::core::project::MediaType::Image if has_path => {
            img(media.path.clone())
                .size_full().object_fit(ObjectFit::Contain)
                .rounded_lg().border_1().border_color(theme::border())
                .into_any_element()
        }
        crate::core::project::MediaType::Video => {
            if let (Some(frame_path), Some(frame_media_id)) = (current_frame_path, current_frame_media_id)
                && frame_media_id == &media.id
            {
                return img(frame_path.clone())
                    .size_full().object_fit(ObjectFit::Contain)
                    .rounded_lg().border_1().border_color(theme::border())
                    .into_any_element();
            }
            placeholder_box("\u{1F3AC}")
        }
        crate::core::project::MediaType::Image => {
            placeholder_box("\u{1F5BC}")
        }
        crate::core::project::MediaType::Audio => {
            placeholder_box("\u{266B}")
        }
        _ => {
            placeholder_box("\u{1F4C1}")
        }
    }
}

fn placeholder_box(icon: &str) -> AnyElement {
    div().size_full().bg(theme::surface()).rounded_lg()
        .border_1().border_color(theme::border())
        .flex().items_center().justify_center()
        .child(div().text_color(theme::text_muted()).text_size(px(64.0)).child(icon.to_string()))
        .into_any_element()
}

#[expect(clippy::too_many_arguments, reason = "GPUI callback pattern")]
pub fn editor_layout(
    project_name: &str,
    media_items: &[MediaItem],
    sequence: &Sequence,
    selected_clip_id: &Option<String>,
    playhead_position: f64,
    playing: bool,
    _player_position: f64,
    current_frame_path: &Option<std::path::PathBuf>,
    current_frame_media_id: &Option<String>,
    on_home: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_import: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_media_click: impl Fn(&usize, &mut Window, &mut App) + 'static,
    on_clip_select: impl Fn(&String, &mut Window, &mut App) + 'static,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_stop: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_seek: impl Fn(&f64, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_toggle = Rc::new(on_toggle);
    let on_stop = Rc::new(on_stop);
    let on_seek = Rc::new(on_seek);
    div()
        .flex().flex_col().size_full()
        .bg(theme::background())
        .child(
            div().h(px(40.0)).px_3().flex().items_center().justify_between()
                .bg(theme::panel()).border_b_1().border_color(theme::border())
                .child(
                    div().flex().items_center().gap_3()
                        .child(
                            div().id("back-home").flex().items_center().gap_2().cursor_pointer()
                                .on_click(on_home)
                                .child(div().text_color(theme::text_muted()).text_size(px(14.0)).child("\u{2190}"))
                                .child(div().font_family("Lexend").text_color(theme::orange()).text_size(px(13.0)).font_weight(FontWeight::BOLD).child("VELOCIS")),
                        )
                        .child(div().font_family("Lexend").text_color(theme::text_primary()).text_size(px(12.0)).child(project_name.to_string())),
                )
                .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(10.0)).child("Ctrl+K")),
        )
        .child(
            div().flex().flex_row().flex_grow().overflow_hidden()
                .child(
                    div().flex().flex_col().w(px(240.0)).h_full().bg(theme::panel()).border_r_1().border_color(theme::border())
                        .child(crate::ui::panels::media::media_panel(media_items, on_import, on_media_click)),
                )
                .child(
                    div().flex().flex_col().flex_grow().h_full()
                        .child(preview_area(sequence, media_items, playhead_position, current_frame_path, current_frame_media_id))
                        .child(transport_bar(sequence, media_items, playing, playhead_position, {
                            let cb = Rc::clone(&on_toggle);
                            move |e, w, cx| (cb)(e, w, cx)
                        }, {
                            let cb = Rc::clone(&on_stop);
                            move |e, w, cx| (cb)(e, w, cx)
                        }, {
                            let cb = Rc::clone(&on_seek);
                            move |p, w, cx| (cb)(p, w, cx)
                        }))
                )
                .child(
                    div().flex().flex_col().w(px(240.0)).h_full().bg(theme::panel()).border_l_1().border_color(theme::border())
                        .child(
                            div().h(px(32.0)).px_3().flex().items_center().bg(theme::surface()).border_b_1().border_color(theme::border())
                                .child(div().font_family("Lexend").text_color(theme::text_secondary()).text_size(px(11.0)).font_weight(FontWeight::SEMIBOLD).child("EFFETS")),
                        )
                        .child(
                            div().flex().flex_col().items_center().justify_center().size_full()
                                .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(12.0)).child("Selectionnez un clip")),
                        ),
                ),
        )
        .child(
            div().h(px(200.0)).w_full().bg(theme::panel()).border_t_1().border_color(theme::border())
                .child(crate::ui::timeline::timeline(sequence, selected_clip_id, playhead_position, on_clip_select, {
                    let cb = Rc::clone(&on_seek);
                    move |p, w, cx| (cb)(p, w, cx)
                })),
        )
}
