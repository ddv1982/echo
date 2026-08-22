use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use resvg::usvg;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Master {
    App,
    Tray,
}

impl Master {
    fn path(self) -> &'static str {
        match self {
            Self::App => "assets/icons/echo-app.svg",
            Self::Tray => "assets/icons/echo-tray.svg",
        }
    }
}

struct Raster {
    master: Master,
    size: u32,
    dest: &'static str,
}

const RASTERS: &[Raster] = &[
    Raster {
        master: Master::App,
        size: 32,
        dest: "src-tauri/icons/32x32.png",
    },
    Raster {
        master: Master::App,
        size: 128,
        dest: "src-tauri/icons/128x128.png",
    },
    Raster {
        master: Master::App,
        size: 256,
        dest: "src-tauri/icons/128x128@2x.png",
    },
    Raster {
        master: Master::App,
        size: 256,
        dest: "src-tauri/icons/256x256.png",
    },
    Raster {
        master: Master::App,
        size: 512,
        dest: "src-tauri/icons/512x512.png",
    },
    Raster {
        master: Master::App,
        size: 256,
        dest: "src-tauri/icons/icon.png",
    },
    Raster {
        master: Master::Tray,
        size: 22,
        dest: "src-tauri/icons/tray-22.png",
    },
    Raster {
        master: Master::Tray,
        size: 24,
        dest: "src-tauri/icons/tray-24.png",
    },
    Raster {
        master: Master::Tray,
        size: 32,
        dest: "src-tauri/icons/tray-32.png",
    },
    Raster {
        master: Master::Tray,
        size: 48,
        dest: "src-tauri/icons/tray-48.png",
    },
    Raster {
        master: Master::App,
        size: 32,
        dest: "frontend/public/favicon.png",
    },
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask crate lives at crates/xtask")
        .to_path_buf()
}

fn main() {
    if let Err(err) = generate(&workspace_root()) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn generate(root: &Path) -> Result<(), String> {
    for raster in RASTERS {
        let svg_path = root.join(raster.master.path());
        let svg = fs::read(&svg_path).map_err(|err| format!("read {}: {err}", svg_path.display()))?;
        let pixmap = render(&svg, raster.size)?;
        let dest = root.join(raster.dest);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create {}: {err}", parent.display()))?;
        }
        write_png(&dest, &pixmap)?;
    }
    Ok(())
}

fn render(svg: &[u8], size: u32) -> Result<tiny_skia::Pixmap, String> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default())
        .map_err(|err| format!("parse svg: {err}"))?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)
        .ok_or_else(|| format!("pixmap {size}x{size}"))?;
    let sx = size as f32 / tree.size().width();
    let sy = size as f32 / tree.size().height();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(sx, sy),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

fn write_png(path: &Path, pixmap: &tiny_skia::Pixmap) -> Result<(), String> {
    let file = File::create(path).map_err(|err| format!("create {}: {err}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), pixmap.width(), pixmap.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|err| format!("png header {}: {err}", path.display()))?;
    let data = straight_rgba(pixmap);
    writer
        .write_image_data(&data)
        .map_err(|err| format!("png data {}: {err}", path.display()))?;
    writer
        .finish()
        .map_err(|err| format!("png finish {}: {err}", path.display()))?;
    Ok(())
}

fn straight_rgba(pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixmap.data().len());
    for px in pixmap.pixels() {
        let color = px.demultiply();
        out.extend_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    struct Loaded {
        dest: &'static str,
        master: Master,
        width: u32,
        height: u32,
        color: png::ColorType,
        pixels: Vec<u8>,
    }

    fn load_all() -> Vec<Loaded> {
        let root = workspace_root();
        RASTERS
            .iter()
            .map(|raster| {
                let path = root.join(raster.dest);
                let file =
                    File::open(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
                let decoder = png::Decoder::new(BufReader::new(file));
                let mut reader = decoder
                    .read_info()
                    .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
                let (color, _) = reader.output_color_type();
                let mut pixels = vec![0; reader.output_buffer_size().expect("png too large")];
                let info = reader
                    .next_frame(&mut pixels)
                    .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
                Loaded {
                    dest: raster.dest,
                    master: raster.master,
                    width: info.width,
                    height: info.height,
                    color,
                    pixels: pixels[..info.buffer_size()].to_vec(),
                }
            })
            .collect()
    }

    fn size_from_filename(name: &str) -> u32 {
        if name == "icon.png" {
            return 256;
        }
        if name == "favicon.png" {
            return 32;
        }
        if let Some(rest) = name.strip_prefix("tray-") {
            return rest
                .strip_suffix(".png")
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| panic!("tray size in {name}"));
        }
        let stem = name.strip_suffix(".png").unwrap_or(name);
        if let Some((base, scale)) = stem.split_once('@') {
            let dim: u32 = base
                .split('x')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| panic!("size in {name}"));
            let factor: u32 = scale
                .strip_suffix('x')
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| panic!("scale in {name}"));
            return dim * factor;
        }
        stem.split('x')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("size in {name}"))
    }

    fn alpha_at(img: &Loaded, x: u32, y: u32) -> u8 {
        img.pixels[((y * img.width + x) * 4 + 3) as usize]
    }

    #[test]
    fn rasters_are_square_rgba_matching_filename() {
        for img in load_all() {
            let name = Path::new(img.dest)
                .file_name()
                .and_then(|n| n.to_str())
                .expect("utf-8 filename");
            assert_eq!(img.width, img.height, "{}", img.dest);
            assert_eq!(img.width, size_from_filename(name), "{}", img.dest);
            assert_eq!(img.color, png::ColorType::Rgba, "{}", img.dest);
        }
    }

    #[test]
    fn app_raster_corners_are_transparent() {
        for img in load_all() {
            if img.master != Master::App {
                continue;
            }
            let max = img.width - 1;
            for (x, y) in [(0, 0), (max, 0), (0, max), (max, max)] {
                assert_eq!(alpha_at(&img, x, y), 0, "{} corner {x},{y}", img.dest);
            }
        }
    }

    /// Minimum distance between the tray glyph's mean relative luminance and
    /// the panel it sits on. Panels cannot be probed for brightness, so the
    /// glyph is composited over the two extremes a panel can be: pure white
    /// and pure black. A near-white glyph on clear ground fails on white.
    const TRAY_MIN_LUMINANCE_CONTRAST: f64 = 0.30;

    fn channel_luminance(v: u8) -> f64 {
        let c = f64::from(v) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    fn relative_luminance(r: u8, g: u8, b: u8) -> f64 {
        0.2126 * channel_luminance(r) + 0.7152 * channel_luminance(g) + 0.0722 * channel_luminance(b)
    }

    #[test]
    fn tray_rasters_contrast_with_light_and_dark_panels() {
        for img in load_all() {
            if img.master != Master::Tray {
                continue;
            }
            let max = img.width - 1;
            for (x, y) in [(0, 0), (max, 0), (0, max), (max, max)] {
                assert_eq!(alpha_at(&img, x, y), 0, "{} corner {x},{y}", img.dest);
            }
            for bg in [0u8, 255] {
                let mut sum = 0.0;
                let mut count = 0usize;
                for px in img.pixels.chunks(4) {
                    if px[3] < 128 {
                        continue;
                    }
                    let a = f64::from(px[3]) / 255.0;
                    let over = |c: u8| {
                        (f64::from(c) * a + f64::from(bg) * (1.0 - a)).round() as u8
                    };
                    sum += relative_luminance(over(px[0]), over(px[1]), over(px[2]));
                    count += 1;
                }
                assert!(count > 0, "{} has no glyph pixels", img.dest);
                let glyph = sum / count as f64;
                let panel = relative_luminance(bg, bg, bg);
                assert!(
                    (glyph - panel).abs() >= TRAY_MIN_LUMINANCE_CONTRAST,
                    "{} glyph luminance {glyph:.3} vs panel {panel:.3} below {TRAY_MIN_LUMINANCE_CONTRAST}",
                    img.dest
                );
            }
        }
    }
}
