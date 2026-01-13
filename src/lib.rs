use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use wasm_bindgen::prelude::*;
use std::sync::Mutex;
use std::time::Duration;
use std::io::Cursor;
use lopdf::Document; // NEW IMPORT
use bevy::asset::AssetMetaCheck;

const AVAILABLE_FONTS: &[&str] = &[
  "Arimo-Regular.ttf",
    "EBGaramond-Regular.ttf",
    "Roboto-Regular.ttf",
    "Tinos-Regular.ttf"
];







// --- GLOBAL MAILBOX ---
static UPLOADED_FILE_QUEUE: Mutex<Option<Vec<u8>>> = Mutex::new(None);

#[wasm_bindgen]
pub fn pass_file_to_bevy(data: &[u8]) {
    let mut lock = UPLOADED_FILE_QUEUE.lock().unwrap();
    *lock = Some(data.to_vec());
}

// --- RESOURCES ---

#[derive(Resource)]
struct RsvpState {
    // Outer Vec = Pages, Inner Vec = Words in that page
    pages: Vec<Vec<String>>,
    
    current_page_index: usize,
    current_word_index: usize,
    
    wpm: f32,
    is_playing: bool,
    timer: Timer,
    
    font_size: f32,
    current_font_handle: Handle<Font>,
    current_font_name: String,
}

impl Default for RsvpState {
    fn default() -> Self {
        // Default demo text (Page 1)
        let page1 = vec!["Upload".into(), "a".into(), "PDF".into(), "to".into(), "begin.".into()];
        
        Self {
            pages: vec![page1],
            current_page_index: 0,
            current_word_index: 0,
            wpm: 300.0,
            is_playing: false,
            timer: Timer::from_seconds(60.0 / 300.0, TimerMode::Repeating),
            font_size: 100.0,
            current_font_handle: Handle::default(),
            current_font_name: "Default".to_string(),
        }
    }
}


#[derive(Component)]
struct ReaderText;

// --- SYSTEMS ---

fn setup(mut commands: Commands, asset_server: Res<AssetServer>, mut rsvp: ResMut<RsvpState>) {
    commands.spawn(Camera2d::default());

    // Load default font
    rsvp.current_font_name = AVAILABLE_FONTS[0].to_string();
    rsvp.current_font_handle = asset_server.load(format!("fonts/{}", rsvp.current_font_name));

    commands.spawn((
        Text::new("Ready"),
        TextFont {
            font: rsvp.current_font_handle.clone(),
            font_size: rsvp.font_size,
            ..default()
        },
        TextColor(Color::WHITE),
        TextLayout::new(JustifyText::Center, LineBreak::WordBoundary),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(40.0), 
            left: Val::Percent(10.0),
            right: Val::Percent(10.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        ReaderText,
    ));
}

fn file_listener_system(mut rsvp: ResMut<RsvpState>) {
    let mut lock = UPLOADED_FILE_QUEUE.lock().unwrap();
    
    if let Some(data) = lock.take() {
        info!("Processing PDF...");
        let cursor = Cursor::new(data);
        
        match Document::load_from(cursor) {
            Ok(doc) => {
                let mut new_pages = Vec::new();

                // Sort pages by key to ensure order (lopdf stores them in a map)
                let mut page_numbers: Vec<u32> = doc.get_pages().keys().cloned().collect();
                page_numbers.sort();

                for page_num in page_numbers {
                    if let Ok(text) = doc.extract_text(&[page_num]) {
                        // Clean up text
                        let words: Vec<String> = text
                            .split_whitespace()
                            .map(|s| s.to_string())
                            .collect();
                        
                        if !words.is_empty() {
                            new_pages.push(words);
                        }
                    }
                }

                if !new_pages.is_empty() {
                    rsvp.pages = new_pages;
                    rsvp.current_page_index = 0;
                    rsvp.current_word_index = 0;
                    rsvp.is_playing = true;
                    info!("PDF Parsed. Pages: {}", rsvp.pages.len());
                } else {
                    error!("PDF contained no text.");
                }
            },
            Err(e) => error!("Failed to load PDF: {:?}", e),
        }
    }
}

fn rsvp_tick_system(
    time: Res<Time>, 
    mut rsvp: ResMut<RsvpState>, 
    mut query: Query<&mut Text, With<ReaderText>>
) {
    if !rsvp.is_playing || rsvp.pages.is_empty() {
        return;
    }

    // 1. Update Timer based on WPM
    let seconds_per_word = 60.0 / rsvp.wpm;
    rsvp.timer.set_duration(Duration::from_secs_f32(seconds_per_word));
    rsvp.timer.tick(time.delta());

    if rsvp.timer.just_finished() {
        let current_page = &rsvp.pages[rsvp.current_page_index];

        // 2. Advance Word
        if rsvp.current_word_index < current_page.len() {
            // Update Screen
            for mut text in query.iter_mut() {
                text.0 = current_page[rsvp.current_word_index].clone();
            }
            rsvp.current_word_index += 1;
        } 
        // 3. End of Page?
        else {
            // Move to next page if available
            if rsvp.current_page_index + 1 < rsvp.pages.len() {
                rsvp.current_page_index += 1;
                rsvp.current_word_index = 0;
            } else {
                // End of Book
                rsvp.is_playing = false;
            }
        }
    }
}

fn ui_controls_system(
    mut contexts: EguiContexts, 
    mut rsvp: ResMut<RsvpState>,
    asset_server: Res<AssetServer>,
    mut text_query: Query<&mut TextFont, With<ReaderText>>
) {
    egui::Window::new("Reader Settings")
        .anchor(egui::Align2::RIGHT_TOP, [-10.0, 10.0])
        .show(contexts.ctx_mut(), |ui| {
            
            ui.heading("Controls");
            
            // Play / Pause
            if ui.button(if rsvp.is_playing { "Pause" } else { "Play" }).clicked() {
                rsvp.is_playing = !rsvp.is_playing;
            }

            ui.separator();

            // --- PAGE SELECTION ---
            ui.horizontal(|ui| {
                ui.label("Page:");
                // Slider from 1 to Total Pages (User sees 1-based, Code uses 0-based)
                // We use a temporary variable to bridge the gap
                let mut page_display = rsvp.current_page_index + 1;
                let max_pages = rsvp.pages.len();
                
                if ui.add(egui::Slider::new(&mut page_display, 1..=max_pages)).changed() {
                    // User moved the slider
                    rsvp.current_page_index = page_display - 1;
                    rsvp.current_word_index = 0; // Reset to start of that page
                    
                    // Update text immediately to show the first word of the new page
                    if let Some(page) = rsvp.pages.get(rsvp.current_page_index) {
                        if let Some(first_word) = page.first() {
                             // We can't easily update the Text component here without a Query for Text
                             // But the next tick will catch it instantly.
                        }
                    }
                }
                ui.label(format!("/ {}", max_pages));
            });
            
            // Progress Bar for current page
            let current_page_len = rsvp.pages.get(rsvp.current_page_index).map(|p| p.len()).unwrap_or(1);
            ui.add(egui::ProgressBar::new(rsvp.current_word_index as f32 / current_page_len as f32)
                .text("Page Progress"));

            ui.separator();

            // --- WPM ---
            ui.label(format!("Speed: {:.0} WPM", rsvp.wpm));
            ui.add(egui::Slider::new(&mut rsvp.wpm, 30.0..=900.0));

            ui.separator();

            // --- FONT SIZE ---
            ui.label("Text Size");
            if ui.add(egui::Slider::new(&mut rsvp.font_size, 20.0..=200.0)).changed() {
                for mut font in text_query.iter_mut() {
                    font.font_size = rsvp.font_size;
                }
            }

            ui.separator();

            // --- FONT SELECTION ---
            ui.label("Font Family");
            egui::ComboBox::from_id_salt("font_selector")
                .selected_text(&rsvp.current_font_name)
                .show_ui(ui, |ui| {
                    for font_name in AVAILABLE_FONTS {
                        if ui.selectable_value(&mut rsvp.current_font_name, font_name.to_string(), *font_name).clicked() {
                            let new_handle = asset_server.load(format!("fonts/{}", font_name));
                            rsvp.current_font_handle = new_handle.clone();
                            
                            for mut font in text_query.iter_mut() {
                                font.font = new_handle.clone();
                            }
                        }
                    }
                });
        });
}

use bevy::log::LogPlugin; // Add this import

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();

    App::new()
        .add_plugins(DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    canvas: Some("#bevy-canvas".into()),
                    fit_canvas_to_parent: true,
                    ..default()
                }),
                ..default()
            })
            // 1. Fix the Meta Check (Keep this from before)
            .set(AssetPlugin {
                meta_check: AssetMetaCheck::Never, 
                ..default()
            })
            // 2. NEW: Silence the PDF parser logs
            .set(LogPlugin {
                // Set the global default to INFO or WARN
                level: bevy::log::Level::INFO, 
                
                // Detailed filter: 
                // "wgpu=error" -> Silence graphics driver noise
                // "lopdf=error" -> ONLY show actual crashes from the PDF parser, hide warnings
                filter: "wgpu=error,lopdf=error,bevy_rsvp_reader=debug".to_string(),
                ..default()
            })
        )
        .add_plugins(EguiPlugin)
        .init_resource::<RsvpState>()
        .add_systems(Startup, setup)
        .add_systems(Update, (file_listener_system, ui_controls_system, rsvp_tick_system))
        .run();
}