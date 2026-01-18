use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use wasm_bindgen::prelude::*;
use std::sync::Mutex;
use std::time::Duration;
use std::io::Cursor;
use lopdf::Document;
use bevy::asset::AssetMetaCheck;
use bevy::log::LogPlugin;

// Ensure these files exist in your "assets/fonts/" folder!
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
    words_per_frame: usize, 
    
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
            words_per_frame: 1, 
            is_playing: false,
            timer: Timer::from_seconds(60.0 / 300.0, TimerMode::Repeating),
            font_size: 100.0,
            // Initialize with default, but we MUST overwrite this in setup before use
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
    
    // 1. SAFETY: Load the font immediately into a local variable
    let font_name = AVAILABLE_FONTS.first().unwrap_or(&"Arimo-Regular.ttf"); 
    let font_handle = asset_server.load(format!("fonts/{}", font_name));
    
    // 2. Update Resource with this valid handle
    rsvp.current_font_name = font_name.to_string();
    rsvp.current_font_handle = font_handle.clone();

    // 3. Spawn Text with the VALID handle (never use Handle::default() for TextFont)
    commands.spawn((
        Text::new("Ready"),
        TextFont {
            font: font_handle, // Use the local strong handle
            font_size: rsvp.font_size,
            ..default()
        },
        TextColor(Color::WHITE),
        // CRITICAL FIX: Use NoWrap. WordBoundary causes panics if font metrics aren't ready during extraction.
        TextLayout::new(JustifyText::Center, LineBreak::NoWrap),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            justify_content: JustifyContent::Center, 
            align_items: AlignItems::Center,         
            ..default()
        },
        ReaderText,
    ));
}

fn ui_controls_system(
    mut contexts: EguiContexts, 
    mut rsvp: ResMut<RsvpState>, 
    asset_server: Res<AssetServer>, 
    mut text_query: Query<&mut TextFont, With<ReaderText>>
) {
    egui::Window::new("Reader Settings")
        .anchor(egui::Align2::RIGHT_TOP, [-10.0, 10.0]) 
        .default_width(200.0)
        .show(contexts.ctx_mut(), |ui| {
            
            ui.heading("Controls");
            
            if ui.button(if rsvp.is_playing { "Pause" } else { "Play" }).clicked() { 
                rsvp.is_playing = !rsvp.is_playing; 
            }
            
            ui.separator();
            
            ui.horizontal(|ui| {
                ui.label("Page:");
                let mut page_display = rsvp.current_page_index + 1;
                let total_pages = rsvp.pages.len().max(1);
                
                if ui.add(egui::Slider::new(&mut page_display, 1..=total_pages)).changed() {
                    rsvp.current_page_index = page_display - 1;
                    rsvp.current_word_index = 0;
                }
                ui.label(format!("/ {}", total_pages));
            });
            
            let current_len = rsvp.pages.get(rsvp.current_page_index).map(|p| p.len()).unwrap_or(1);
            let progress = (rsvp.current_word_index as f32 / current_len as f32).min(1.0);
            ui.add(egui::ProgressBar::new(progress).text("Page Progress"));

            ui.separator();

            ui.label(format!("Speed: {:.0} WPM", rsvp.wpm));
            ui.add(egui::Slider::new(&mut rsvp.wpm, 30.0..=900.0));

            ui.separator();
            
            ui.label(format!("Words Per Frame: {}", rsvp.words_per_frame));
            ui.add(egui::Slider::new(&mut rsvp.words_per_frame, 1..=7));
            
            ui.separator();

            ui.label("Text Size");
            if ui.add(egui::Slider::new(&mut rsvp.font_size, 20.0..=200.0)).changed() {
                for mut font in text_query.iter_mut() { font.font_size = rsvp.font_size; }
            }

            ui.separator();

            ui.label("Font Family");
            
            // Decoupled Font Selection Logic (Prevents UI lock/crash)
            let mut selected_font = rsvp.current_font_name.clone();
            let mut font_changed = false;

            egui::ComboBox::from_id_salt("font_sel")
                .selected_text(&selected_font)
                .show_ui(ui, |ui| {
                    for font_name in AVAILABLE_FONTS {
                        if ui.selectable_value(&mut selected_font, font_name.to_string(), *font_name).clicked() {
                            font_changed = true;
                        }
                    }
                });

            if font_changed {
                rsvp.current_font_name = selected_font;
                let new_handle = asset_server.load(format!("fonts/{}", rsvp.current_font_name));
                rsvp.current_font_handle = new_handle.clone();
                
                for mut font in text_query.iter_mut() { 
                    font.font = new_handle.clone(); 
                }
            }
        });
}

fn file_listener_system(mut rsvp: ResMut<RsvpState>) {
    let mut lock = UPLOADED_FILE_QUEUE.lock().unwrap();
    
    if let Some(data) = lock.take() {
        info!("Processing PDF...");
        let cursor = Cursor::new(data);
        
        match Document::load_from(cursor) {
            Ok(doc) => {
                let mut new_pages = Vec::new();

                let mut page_numbers: Vec<u32> = doc.get_pages().keys().cloned().collect();
                page_numbers.sort();

                for page_num in page_numbers {
                    if let Ok(text) = doc.extract_text(&[page_num]) {
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

    let seconds_per_chunk = (60.0 / rsvp.wpm) * (rsvp.words_per_frame as f32);
    
    rsvp.timer.set_duration(Duration::from_secs_f32(seconds_per_chunk));
    rsvp.timer.tick(time.delta());

    if rsvp.timer.just_finished() {
        let current_page = &rsvp.pages[rsvp.current_page_index];

        if rsvp.current_word_index < current_page.len() {
            let end_index = (rsvp.current_word_index + rsvp.words_per_frame).min(current_page.len());
            let chunk_text = current_page[rsvp.current_word_index..end_index].join(" ");
            
            for mut text in query.iter_mut() {
                text.0 = chunk_text.clone();
            }
            
            rsvp.current_word_index += rsvp.words_per_frame;
        } else {
            if rsvp.current_page_index + 1 < rsvp.pages.len() {
                rsvp.current_page_index += 1;
                rsvp.current_word_index = 0;
            } else {
                rsvp.is_playing = false;
            }
        }
    }
}

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
            .set(AssetPlugin {
                meta_check: AssetMetaCheck::Never, 
                ..default()
            })
            .set(LogPlugin {
                level: bevy::log::Level::INFO, 
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