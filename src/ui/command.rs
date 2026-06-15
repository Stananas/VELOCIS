use gpui::*;
use crate::ui::theme;

pub fn command_palette(
    visible: bool,
    close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    if !visible { return div().into_any_element(); }

    div().id("cmd-overlay").absolute().inset_0()
        .flex().items_start().justify_center().pt(px(80.0))
        .bg(hsla(0.0, 0.0, 0.0, 0.6))
        .child(
            div().w(px(480.0)).bg(theme::panel()).rounded_lg()
                .border_1().border_color(theme::border()).shadow_lg()
                .child(
                    div().h(px(40.0)).px_3().flex().items_center()
                        .bg(theme::surface()).border_b_1().border_color(theme::border()).rounded_t_lg()
                        .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(12.0)).child("Tapez une commande\u{2026}")),
                )
                .child(
                    div().flex().flex_col().p_1().gap_0p5()
                        .child(cmd_item("Palette de commandes", "Ctrl+K"))
                        .child(cmd_item("Accueil", "Ctrl+Shift+H"))
                        .child(cmd_item("Nouveau projet", "Ctrl+N"))
                        .child(cmd_item("Enregistrer", "Ctrl+S")),
                )
                .child(
                    div().px_3().py_2().flex().items_center().justify_end()
                        .border_t_1().border_color(theme::border())
                        .child(
                            div().id("close-cmd").px_3().py_1().cursor_pointer()
                                .on_click(close)
                                .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(11.0)).child("Esc \u{00B7} Fermer")),
                        ),
                ),
        )
        .into_any_element()
}

fn cmd_item(label: &str, shortcut: &str) -> impl IntoElement {
    div().h(px(32.0)).px_3().flex().items_center().justify_between().rounded_md()
        .hover(|s: StyleRefinement| s.bg(theme::surface()))
        .child(div().font_family("Lexend").text_color(theme::text_primary()).text_size(px(13.0)).child(label.to_string()))
        .child(
            div().px_2().py_0p5().bg(theme::surface()).rounded_sm()
                .child(div().font_family("Lexend").text_color(theme::text_muted()).text_size(px(10.0)).child(shortcut.to_string())),
        )
}
