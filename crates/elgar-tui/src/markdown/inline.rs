//! Inline markdown cleanup.
//!
//! This strips only simple paired emphasis markers and keeps code/path text
//! otherwise intact.

pub(super) fn render_plain_line(line: &str) -> String {
    render_inline(line.trim_end())
}

pub(super) fn render_inline(line: &str) -> String {
    strip_paired_marker(&strip_paired_marker(&line.replace("\\_", "_"), "**"), "__")
}

fn strip_paired_marker(line: &str, marker: &str) -> String {
    let mut rendered = String::new();
    let mut rest = line;
    loop {
        let Some(open) = rest.find(marker) else {
            rendered.push_str(rest);
            break;
        };
        let after_open = &rest[open + marker.len()..];
        let Some(close) = after_open.find(marker) else {
            rendered.push_str(rest);
            break;
        };
        rendered.push_str(&rest[..open]);
        rendered.push_str(&after_open[..close]);
        rest = &after_open[close + marker.len()..];
    }
    rendered
}
