use std::rc::Rc;
use gpui::*;
use crate::ui::theme;

pub fn home_screen(
    projects: &[crate::core::state::ProjectMeta],
    on_new_project: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_open_project: impl Fn(&String, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_new_project = Rc::new(on_new_project);
    let on_open_project = Rc::new(on_open_project);

    div()
        .flex().flex_col().size_full()
        .bg(theme::background())
        .child(
            div().h(px(56.0)).px_12().flex().items_center().justify_between()
                .border_b_1().border_color(theme::border())
                .child(
                    div().flex().items_center().gap_3()
                        .child(div().font_family("Lexend").text_color(theme::orange()).text_size(px(18.0)).font_weight(FontWeight::BOLD).child("VELOCIS"))
                        .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(11.0)).child("Éditeur vidéo")),
                )
                .child(
                    div().id("new-project-btn").px_4().py_1p5().bg(theme::orange()).rounded_md().cursor_pointer()
                        .on_click({
                            let cb = Rc::clone(&on_new_project);
                            move |e, w, cx| (cb)(e, w, cx)
                        })
                        .child(div().font_family("Lexend").text_color(rgb(0xFFFFFF)).text_size(px(12.0)).font_weight(FontWeight::SEMIBOLD).child("+ Nouveau projet")),
                ),
        )
        .child(
            div().flex_grow().overflow_hidden().px_12().py_12()
                .child(
                    div().flex().flex_col().gap_6()
                        .child(div().font_family("Lexend").text_color(theme::text_secondary()).text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).child("Projets récents"))
                        .child(
                            div().flex().flex_row().flex_wrap().gap_4()
                                .child(
                                    div().id("new-card")
                                        .flex().flex_col().w(px(200.0)).h(px(180.0))
                                        .bg(theme::panel()).rounded_lg()
                                        .border_2().border_dashed().border_color(theme::border())
                                        .items_center().justify_center().gap_2().cursor_pointer()
                                        .hover(|s: StyleRefinement| s.border_color(theme::orange()))
                                        .on_click({
                                            let cb = Rc::clone(&on_new_project);
                                            move |e, w, cx| (cb)(e, w, cx)
                                        })
                                        .child(div().text_color(theme::orange()).text_size(px(36.0)).font_weight(FontWeight::LIGHT).child("+"))
                                        .child(div().font_family("Lexend").text_color(theme::text_secondary()).text_size(px(12.0)).child("Nouveau projet")),
                                )
                                .children(projects.iter().enumerate().map(|(i, project)| {
                                    let pid = project.id.clone();
                                    let name = project.name.clone();
                                    let elapsed = project.last_opened
                                        .duration_since(std::time::UNIX_EPOCH).ok()
                                        .map(|d| {
                                            let mins = d.as_secs() / 60;
                                            if mins < 60 { format!("Il y a {} min", mins) }
                                            else if mins < 1440 { format!("Il y a {}h", mins / 60) }
                                            else { format!("Il y a {}j", mins / 1440) }
                                        }).unwrap_or_default();
                                    div().id(("project", i))
                                        .on_click({
                                            let cb = on_open_project.clone();
                                            move |_, w, cx| (cb)(&pid, w, cx)
                                        })
                                        .child(
                                            div().flex().flex_col().w(px(200.0))
                                                .bg(theme::panel()).rounded_lg().border_1().border_color(theme::border()).overflow_hidden()
                                                .cursor_pointer().hover(|s: StyleRefinement| s.border_color(theme::orange()))
                                                .child(
                                                    div().h(px(112.0)).bg(theme::surface()).flex().items_center().justify_center()
                                                        .child(div().text_color(theme::text_muted()).text_size(px(32.0)).child("\u{1F3AC}")),
                                                )
                                                .child(
                                                    div().flex().flex_col().p_3().gap_1()
                                                        .child(div().font_family("Lexend").text_color(theme::text_primary()).text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).child(name))
                                                        .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(10.0)).child(elapsed)),
                                                ),
                                        )
                                })),
                        ),
                ),
        )
        .child(
            div().h(px(32.0)).px_12().flex().items_center().bg(theme::panel()).border_t_1().border_color(theme::border())
                .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(10.0)).child("Ctrl+K \u{00B7} Palette de commandes")),
        )
}
