use crate::configs::settings::RawSideView;

// TODO: implement search for raw side if \x or \o or \b
// TODO: make sync scroll/pan and selection

pub fn format_raw_bytes(bytes: &[u8], mode: RawSideView) -> String {
    match mode {
        RawSideView::Off => String::new(),
        RawSideView::Hex => bytes
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" "),
        RawSideView::Octal => bytes
            .iter()
            .map(|b| format!("{:03o}", b))
            .collect::<Vec<_>>()
            .join(" "),
        RawSideView::Binary => bytes
            .iter()
            .map(|b| format!("{:08b}", b))
            .collect::<Vec<_>>()
            .join(" "),
    }
}
