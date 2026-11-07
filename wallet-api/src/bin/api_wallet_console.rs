use wallet_api::api_wallet_console::ApiWalletConsoleApp;

fn main() {
    let native = eframe::NativeOptions::default();
    let run = eframe::run_native(
        "API Wallet Console",
        native,
        Box::new(|cc| {
            install_chinese_font(&cc.egui_ctx);
            Ok(Box::new(ApiWalletConsoleApp::new()))
        }),
    );

    if let Err(err) = run {
        eprintln!("failed to start app: {err}");
    }
}

fn install_chinese_font(ctx: &eframe::egui::Context) {
    let font_path = "/System/Library/Fonts/Supplemental/Arial Unicode.ttf";
    let Ok(font_bytes) = std::fs::read(font_path) else {
        return;
    };

    let mut fonts = eframe::egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("zh_fallback".to_string(), eframe::egui::FontData::from_owned(font_bytes));

    for family in [eframe::egui::FontFamily::Proportional, eframe::egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("zh_fallback".to_string());
    }

    ctx.set_fonts(fonts);
}
