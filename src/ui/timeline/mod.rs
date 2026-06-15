use std::rc::Rc;
use gpui::*;
use gpui::prelude::FluentBuilder;
use crate::ui::theme;
use crate::core::project::{Sequence, TrackKind, Clip};

const PX_PER_SEC: f32 = 40.0;
type SeekCallback = Rc<dyn Fn(&f64, &mut Window, &mut App)>;

fn time_ruler(
    on_seek: SeekCallback,
) -> impl IntoElement {
    div()
        .h(px(24.0)).w_full().flex().items_center()
        .bg(theme::panel()).border_b_1().border_color(theme::border())
        .child(div().w(px(80.0)).h_full().border_r_1().border_color(theme::border()))
        .child(
            div().id("ruler-area").flex_grow().h_full().flex().relative()
                .children((0..60).map(|i| {
                    let label = if i % 5 == 0 { format!("{:02}:{:02}", i / 60, i % 60) } else { String::new() };
                    div().w(px(PX_PER_SEC)).h_full().flex().items_end()
                        .border_l_1().border_color(if i % 5 == 0 { theme::border() } else { rgb(0x181B20) })
                        .px_1()
                        .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(9.0)).child(label))
                }))
                .on_mouse_down(MouseButton::Left, {
                    let cb = Rc::clone(&on_seek);
                    move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        let secs = mouse_x_to_secs(event.position.x);
                        (cb)(&secs, window, cx);
                    }
                }),
        )
}

fn mouse_x_to_secs(mouse_x: Pixels) -> f64 {
    let x: f64 = mouse_x.into();
    ((x - 80.0) / PX_PER_SEC as f64).max(0.0)
}

fn track_label(name: &str, kind: &TrackKind) -> impl IntoElement {
    let icon = match kind {
        TrackKind::Video => "\u{1F3AC}",
        TrackKind::Audio => "\u{266A}",
        TrackKind::Text => "T",
    };
    div()
        .w(px(80.0)).h_full()
        .flex().flex_col().items_center().justify_center()
        .border_r_1().border_b_1().border_color(theme::border())
        .bg(theme::panel())
        .child(div().text_color(theme::text_secondary()).text_size(px(10.0)).child(icon.to_string()))
        .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(9.0)).child(name.to_string()))
}

fn clip_block(clip: &Clip, is_selected: bool) -> Div {
    let x = px(clip.position as f32 * PX_PER_SEC);
    let w = px((clip.duration as f32 * PX_PER_SEC).max(20.0));
    let (h, s, l) = clip.color;
    let bg = hsla(h, s, l, 1.0);
    let bg_hover = hsla(h, s, (l + 0.1).min(0.5), 1.0);
    let border_c: Rgba = if is_selected { theme::orange() } else { hsla(h, s, (l - 0.05).max(0.1), 1.0).into() };

    div()
        .absolute().left(x).top(px(2.0)).w(w).h(px(44.0))
        .bg(bg).rounded_sm().border_1().border_color(border_c)
        .cursor_pointer().hover(move |s: StyleRefinement| s.bg(bg_hover))
}

pub fn timeline(
    sequence: &Sequence,
    selected_clip_id: &Option<String>,
    playhead_position: f64,
    on_clip_select: impl Fn(&String, &mut Window, &mut App) + 'static,
    on_seek: impl Fn(&f64, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_clip_select = Rc::new(on_clip_select);
    let on_seek: SeekCallback = Rc::new(on_seek);

    div()
        .flex().flex_col().size_full().relative()
        .bg(theme::panel())
        .child(time_ruler(Rc::clone(&on_seek)))
        .on_mouse_move({
            let cb = Rc::clone(&on_seek);
            move |event: &MouseMoveEvent, window: &mut Window, cx: &mut App| {
                if event.dragging() {
                    let secs = mouse_x_to_secs(event.position.x);
                    (cb)(&secs, window, cx);
                }
            }
        })
        .children(sequence.tracks.iter().map(|track| {
            let on_cs = on_clip_select.clone();
            let clips: Vec<gpui::AnyElement> = track.clips.iter().enumerate().map(|(ci, clip)| {
                let is_sel = selected_clip_id.as_ref() == Some(&clip.id);
                let cid = clip.id.clone();
                let block = clip_block(clip, is_sel);
                let cb = on_cs.clone();
                div().id(("clip", ci))
                    .child(block.child(
                        div().px_2().py_1().flex().items_center().size_full()
                            .child(div().font_family("Lexend").text_color(rgb(0xFFFFFF)).text_size(px(10.0)).overflow_hidden().child(clip.name.clone())),
                    ))
                    .on_click(move |_, window, cx| (cb)(&cid, window, cx))
                    .into_any_element()
            }).collect();

            div()
                .flex().h(px(track.height)).w_full()
                .child(track_label(&track.name, &track.kind))
                .child(
                    div().flex_grow().h_full().relative()
                        .border_b_1().border_color(theme::border())
                        .bg(theme::background())
                        .children(clips)
                        .when(track.clips.is_empty(), |d: Div| d.child(
                            div().size_full().flex().items_center().px_3()
                                .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(10.0)).child("Glissez des medias ici")),
                        )),
                )
        }))
        .child(
            div().absolute().top(px(24.0)).left(px(80.0)).right(px(0.0)).bottom(px(0.0)).overflow_hidden()
                .child({
                    let x = px(playhead_position as f32 * PX_PER_SEC);
                    div().absolute().left(x).top(px(0.0)).w(px(2.0)).h_full().bg(theme::playhead())
                        .child(div().absolute().top(px(-6.0)).left(px(-5.0)).w(px(12.0)).h(px(8.0)).bg(theme::playhead()).rounded_t_md())
                }),
        )
}
