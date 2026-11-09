//! The embedded type. The app bundle carries only the binary and icon, so fonts live in the binary.

pub const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf"),
    include_bytes!("../assets/fonts/Caveat-Variable.ttf"),
];

pub const FAMILIES: (&str, &str) = ("Inter", "IBM Plex Mono");

/// The handwriting face used for panel titles.
pub const HAND: &str = "Caveat";

#[cfg(test)]
pub fn text_system() -> rune::text::TextSystem {
    rune::text::TextSystem::new(FONTS, FAMILIES)
}
