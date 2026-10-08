// Embedded web UI
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "ui/dist/"]
pub struct UiAssets;
