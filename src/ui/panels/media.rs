use std::rc::Rc;
use gpui::*;
use crate::ui::theme;
use crate::core::project::MediaItem;

fn media_icon(media_type: &crate::core::project::MediaType) -> &str {
    match media_type {
        crate::core::project::MediaType::Video => "\u{25B6}",
        crate::core::project::MediaType::Audio => "\u{266A}",
        crate::core::project::MediaType::Image => "\u{1F5BC}",
        crate::core::project::MediaType::Other => "\u{1F4C1}",
    }
}

fn duration_label(secs: f64) -> String {
    if secs <= 0.0 { String::new() }
    else { format!("{:02}:{:02}", secs as u64 / 60, secs as u64 % 60) }
}

pub fn media_panel(
    items: &[MediaItem],
    on_import: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_media_click: impl Fn(&usize, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_media_click = Rc::new(on_media_click);

    div()
        .flex().flex_col().size_full()
        .child(
            div().h(px(32.0)).px_3().flex().items_center().justify_between()
                .bg(theme::surface()).border_b_1().border_color(theme::border())
                .child(div().font_family("Lexend").text_color(theme::text_secondary()).text_size(px(11.0)).font_weight(FontWeight::SEMIBOLD).child("MEDIAS"))
                .child(
                    div().id("import-btn").px_2().py_0p5().bg(theme::orange()).rounded_sm().cursor_pointer()
                        .on_click(on_import)
                        .child(div().font_family("Lexend").text_color(rgb(0xFFFFFF)).text_size(px(10.0)).font_weight(FontWeight::SEMIBOLD).child("+ Importer")),
                ),
        )
        .child(
            div().flex().flex_col().p_2().gap_1().overflow_hidden().size_full()
                .children({
                    let empty: Vec<gpui::AnyElement> = vec![
                        div().size_full().flex().items_center().justify_center()
                            .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(12.0)).child("Aucun media. + Importer"))
                            .into_any_element()
                    ];
                    if items.is_empty() {
                        empty
                    } else {
                        items.iter().enumerate().map(|(i, item)| {
                            let cb = Rc::clone(&on_media_click);
                            let idx = i;
                            div().id(("media", i))
                                .flex().gap_2().px_2().py_1p5().items_center().rounded_md()
                                .child(
                                    div().flex().items_center().justify_center().w(px(36.0)).h(px(36.0))
                                        .bg(theme::surface()).rounded_md()
                                        .child(div().text_color(theme::text_secondary()).text_size(px(16.0)).child(media_icon(&item.media_type).to_string())),
                                )
                                .child(
                                    div().flex().flex_col().gap_0p5().min_w_0().flex_grow()
                                        .child(div().font_family("Lexend").text_color(theme::text_primary()).text_size(px(12.0)).child(item.name.clone()))
                                        .child(
                                            div().flex().gap_2()
                                                .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(10.0)).child(match item.media_type {
                                                    crate::core::project::MediaType::Video => "Video",
                                                    crate::core::project::MediaType::Audio => "Audio",
                                                    crate::core::project::MediaType::Image => "Image",
                                                    crate::core::project::MediaType::Other => "Fichier",
                                                }))
                                                .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(10.0)).child(duration_label(item.duration_seconds))),
                                        ),
                                )
                                .child(
                                    div().id(("add-timeline", i)).px_2().rounded_sm().cursor_pointer()
                                        .hover(|s: StyleRefinement| s.bg(theme::orange()))
                                        .on_click(move |_, window, cx| (cb)(&idx, window, cx))
                                        .child(div().text_color(theme::text_secondary()).text_size(px(14.0)).font_weight(FontWeight::BOLD).child("+"))
                                )
                                .into_any_element()
                        }).collect()
                    }
                }),
        )
}
