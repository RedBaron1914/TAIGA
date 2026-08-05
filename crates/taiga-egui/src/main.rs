use eframe::egui;
use taiga_egui::TaigaApp;

fn main() -> eframe::Result<()> {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    
    let data_dir = std::path::PathBuf::from(".taiga_data");
    
    eframe::run_native(
        "TAIGA",
        options,
        Box::new(|cc| Ok(Box::new(TaigaApp::new(cc, data_dir)))),
    )
}
