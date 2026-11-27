pub(super) fn html(markdown: &str) -> String {
    let mut options = comrak::Options::default();
    options.extension.cjk_friendly_emphasis = true;
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.render.hardbreaks = true;
    options.render.escape = true;
    comrak::markdown_to_html(markdown, &options)
}

#[cfg(test)]
mod tests {
    use super::html;

    #[test]
    fn renders_cjk_emphasis_and_keeps_distinct_field_lines() {
        assert_eq!(
            html("**语言：**英语\n**发音**：英 /ˈtəʊkən/"),
            "<p><strong>语言：</strong>英语<br />\n<strong>发音</strong>：英 /ˈtəʊkən/</p>\n"
        );
    }

    #[test]
    fn keeps_literal_markers_and_escapes_raw_html() {
        for (source, expected) in [
            ("`**语言：**英语`", "<code>**语言：**英语</code>"),
            (r"\*\*语言：\*\*英语", "**语言：**英语"),
            ("**发音： **英语", "**发音： **英语"),
            ("<img src=\"pic\">", "&lt;img"),
        ] {
            assert!(html(source).contains(expected));
        }
    }
}
