use std::path::{Path, PathBuf};

use crate::constants::{
    PNG_ALPHA_THRESHOLD, WALL_PIECE_WIDTH, WALL_PIECE_HEIGHT, WALL_GRID_COLS, WALL_BITMASK_COUNT,
    FLOOR_PATTERN_COUNT, FLOOR_TILE_SIZE, CHAR_FRAME_W, CHAR_FRAME_H, CHAR_FRAMES_PER_ROW,
    CHAR_COUNT,
};

/// Decode an RGBA PNG buffer into a 2-D hex-string grid.
/// Empty string = transparent pixel; `#RRGGBB` = opaque pixel.
fn png_to_sprite(data: &[u8], width: u32, height: u32) -> Vec<Vec<String>> {
    let decoder = png::Decoder::new(std::io::Cursor::new(data));
    let mut reader = match decoder.read_info() {
        Ok(r) => r,
        Err(_) => {
            return (0..height)
                .map(|_| (0..width).map(|_| String::new()).collect())
                .collect()
        }
    };
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = match reader.next_frame(&mut buf) {
        Ok(i) => i,
        Err(_) => {
            return (0..height)
                .map(|_| (0..width).map(|_| String::new()).collect())
                .collect()
        }
    };
    let bytes = &buf[..info.buffer_size()];

    // Handle different color types by converting to RGBA
    let rgba = to_rgba(bytes, &info, width, height);

    (0..height as usize)
        .map(|y| {
            (0..width as usize)
                .map(|x| {
                    let i = (y * width as usize + x) * 4;
                    let a = rgba[i + 3];
                    if a < PNG_ALPHA_THRESHOLD {
                        String::new()
                    } else {
                        format!("#{:02X}{:02X}{:02X}", rgba[i], rgba[i + 1], rgba[i + 2])
                    }
                })
                .collect()
        })
        .collect()
}

/// Convert various PNG color types to an RGBA u8 vec.
fn to_rgba(bytes: &[u8], info: &png::OutputInfo, width: u32, height: u32) -> Vec<u8> {
    use png::ColorType;
    let pixel_count = (width * height) as usize;
    match info.color_type {
        ColorType::Rgba => bytes[..pixel_count * 4].to_vec(),
        ColorType::Rgb => {
            let mut out = Vec::with_capacity(pixel_count * 4);
            for i in 0..pixel_count {
                out.push(bytes[i * 3]);
                out.push(bytes[i * 3 + 1]);
                out.push(bytes[i * 3 + 2]);
                out.push(255);
            }
            out
        }
        ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(pixel_count * 4);
            for i in 0..pixel_count {
                let v = bytes[i * 2];
                out.push(v);
                out.push(v);
                out.push(v);
                out.push(bytes[i * 2 + 1]);
            }
            out
        }
        ColorType::Grayscale => {
            let mut out = Vec::with_capacity(pixel_count * 4);
            for i in 0..pixel_count {
                let v = bytes[i];
                out.push(v);
                out.push(v);
                out.push(v);
                out.push(255);
            }
            out
        }
        ColorType::Indexed => {
            // Palette look-up: fall back to transparent
            (0..pixel_count * 4).map(|_| 0).collect()
        }
    }
}

/// Return the assets root directory.
///
/// Priority:
///  1. `resource_dir/assets/` (production bundle)
///  2. `CARGO_MANIFEST_DIR/../../webview-ui/public` (development)
pub fn get_assets_root(app_handle: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    if let Ok(res_dir) = app_handle.path().resource_dir() {
        let candidate = res_dir.join("assets");
        if candidate
            .join("furniture")
            .join("furniture-catalog.json")
            .exists()
        {
            // Return the parent of assets/ so callers can do root/assets/...
            return res_dir;
        }
    }
    // Dev: two levels up from src-tauri/ → project root, then into webview-ui/public
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../../webview-ui/public")
}

// ── Furniture ─────────────────────────────────────────────────

pub fn load_furniture_assets(assets_root: &Path) -> Option<serde_json::Value> {
    let catalog_path = assets_root.join("assets/furniture/furniture-catalog.json");
    if !catalog_path.exists() {
        return None;
    }
    let catalog_text = std::fs::read_to_string(&catalog_path).ok()?;
    let catalog_json: serde_json::Value = serde_json::from_str(&catalog_text).ok()?;
    let assets = catalog_json
        .get("assets")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut sprites: serde_json::Map<String, serde_json::Value> =
        serde_json::Map::new();

    for asset in &assets {
        let id = match asset.get("id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let file = match asset.get("file").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let width = asset
            .get("width")
            .and_then(|v| v.as_u64())
            .unwrap_or(16) as u32;
        let height = asset
            .get("height")
            .and_then(|v| v.as_u64())
            .unwrap_or(16) as u32;

        let file_path = if file.starts_with("assets/") {
            assets_root.join(&file)
        } else {
            assets_root.join("assets").join(&file)
        };
        if !file_path.exists() {
            continue;
        }
        let data = match std::fs::read(&file_path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let sprite = png_to_sprite(&data, width, height);
        sprites.insert(id, serde_json::to_value(&sprite).unwrap_or_default());
    }

    Some(serde_json::json!({
        "catalog": assets,
        "sprites": sprites,
    }))
}

// ── Floor tiles ───────────────────────────────────────────────

pub fn load_floor_tiles(assets_root: &Path) -> Option<serde_json::Value> {
    let path = assets_root.join("assets/floors.png");
    if !path.exists() {
        return None;
    }
    let data = std::fs::read(&path).ok()?;
    let decoder = png::Decoder::new(std::io::Cursor::new(&data));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let bytes = &buf[..info.buffer_size()];
    let png_w = info.width;
    let rgba = to_rgba(bytes, &info, info.width, info.height);

    let mut sprites: Vec<Vec<Vec<String>>> = Vec::new();
    for t in 0..FLOOR_PATTERN_COUNT {
        let mut sprite: Vec<Vec<String>> = Vec::new();
        for y in 0..FLOOR_TILE_SIZE as usize {
            let mut row: Vec<String> = Vec::new();
            for x in 0..FLOOR_TILE_SIZE as usize {
                let px = (t * FLOOR_TILE_SIZE) as usize + x;
                let i = (y * png_w as usize + px) * 4;
                let a = rgba[i + 3];
                if a < PNG_ALPHA_THRESHOLD {
                    row.push(String::new());
                } else {
                    row.push(format!("#{:02X}{:02X}{:02X}", rgba[i], rgba[i + 1], rgba[i + 2]));
                }
            }
            sprite.push(row);
        }
        sprites.push(sprite);
    }
    Some(serde_json::json!({ "sprites": sprites }))
}

// ── Wall tiles ────────────────────────────────────────────────

pub fn load_wall_tiles(assets_root: &Path) -> Option<serde_json::Value> {
    let path = assets_root.join("assets/walls.png");
    if !path.exists() {
        return None;
    }
    let data = std::fs::read(&path).ok()?;
    let decoder = png::Decoder::new(std::io::Cursor::new(&data));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let bytes = &buf[..info.buffer_size()];
    let png_w = info.width;
    let rgba = to_rgba(bytes, &info, info.width, info.height);

    let mut sprites: Vec<Vec<Vec<String>>> = Vec::new();
    for mask in 0..WALL_BITMASK_COUNT {
        let ox = (mask % WALL_GRID_COLS) * WALL_PIECE_WIDTH;
        let oy = (mask / WALL_GRID_COLS) * WALL_PIECE_HEIGHT;
        let mut sprite: Vec<Vec<String>> = Vec::new();
        for r in 0..WALL_PIECE_HEIGHT as usize {
            let mut row: Vec<String> = Vec::new();
            for c in 0..WALL_PIECE_WIDTH as usize {
                let i = ((oy as usize + r) * png_w as usize + (ox as usize + c)) * 4;
                let a = rgba[i + 3];
                if a < PNG_ALPHA_THRESHOLD {
                    row.push(String::new());
                } else {
                    row.push(format!("#{:02X}{:02X}{:02X}", rgba[i], rgba[i + 1], rgba[i + 2]));
                }
            }
            sprite.push(row);
        }
        sprites.push(sprite);
    }
    Some(serde_json::json!({ "sprites": sprites }))
}

// ── Character sprites ─────────────────────────────────────────

pub fn load_character_sprites(assets_root: &Path) -> Option<serde_json::Value> {
    let char_dir = assets_root.join("assets/characters");
    let directions = ["down", "up", "right"];
    let mut characters: Vec<serde_json::Value> = Vec::new();

    for ci in 0..CHAR_COUNT {
        let file_path = char_dir.join(format!("char_{}.png", ci));
        if !file_path.exists() {
            return None;
        }
        let data = std::fs::read(&file_path).ok()?;
        let decoder = png::Decoder::new(std::io::Cursor::new(&data));
        let mut reader = decoder.read_info().ok()?;
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).ok()?;
        let bytes = &buf[..info.buffer_size()];
        let png_w = info.width;
        let rgba = to_rgba(bytes, &info, info.width, info.height);

        let mut char_data = serde_json::Map::new();
        for (dir_idx, dir) in directions.iter().enumerate() {
            let row_offset_y = (dir_idx as u32 * CHAR_FRAME_H) as usize;
            let mut frames: Vec<Vec<Vec<String>>> = Vec::new();
            for f in 0..CHAR_FRAMES_PER_ROW as usize {
                let frame_offset_x = f * CHAR_FRAME_W as usize;
                let mut frame: Vec<Vec<String>> = Vec::new();
                for y in 0..CHAR_FRAME_H as usize {
                    let mut row: Vec<String> = Vec::new();
                    for x in 0..CHAR_FRAME_W as usize {
                        let i = ((row_offset_y + y) * png_w as usize + (frame_offset_x + x)) * 4;
                        let a = rgba[i + 3];
                        if a < PNG_ALPHA_THRESHOLD {
                            row.push(String::new());
                        } else {
                            row.push(format!(
                                "#{:02X}{:02X}{:02X}",
                                rgba[i], rgba[i + 1], rgba[i + 2]
                            ));
                        }
                    }
                    frame.push(row);
                }
                frames.push(frame);
            }
            char_data.insert(dir.to_string(), serde_json::to_value(&frames).unwrap_or_default());
        }
        characters.push(serde_json::Value::Object(char_data));
    }
    Some(serde_json::json!({ "characters": characters }))
}

// ── Default layout ────────────────────────────────────────────

pub fn load_default_layout(assets_root: &Path) -> Option<serde_json::Value> {
    let path = assets_root.join("assets/default-layout.json");
    if !path.exists() {
        return None;
    }
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}
