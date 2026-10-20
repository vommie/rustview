//! Verification scaffolding for the File Formats work (removed when the work is finalized): runs every sample of a
//! directory through the real metadata, thumbnail and viewer code and prints what each stage made of it.
//!
//!   PROBE_DIR=<samples> PROBE_OUT=<dir for pictures> cargo test --test format_probe -- --nocapture

use rustview::media::{MediaDetector, MediaFormat, MediaType, MetadataExtractor};
use rustview::settings::PerformanceSettings;
use rustview::thumbnails::{ScalingAlgorithm, ThumbnailConfig, ThumbnailGenerator};
use rustview::viewer::HighResImageLoader;
use std::path::{Path, PathBuf};

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(dir).sort_by_file_name().into_iter().flatten() {
        if entry.file_type().is_file() {
            out.push(entry.path().to_path_buf());
        }
    }
    out
}

fn save_rgb(out: &Path, name: &str, w: u32, h: u32, rgb: Vec<u8>) {
    if let Some(img) = image::RgbImage::from_raw(w, h, rgb) {
        let _ = img.save(out.join(format!("{name}.png")));
    }
}

#[test]
fn probe_samples() {
    let Ok(dir) = std::env::var("PROBE_DIR") else { return };
    let out = std::env::var("PROBE_OUT").map(PathBuf::from).ok();
    if let Some(o) = &out {
        let _ = std::fs::create_dir_all(o.join("thumb"));
        let _ = std::fs::create_dir_all(o.join("full"));
    }
    let gen_ = ThumbnailGenerator::new();
    let cfg = ThumbnailConfig::from_performance_settings(&PerformanceSettings::default());
    let root = PathBuf::from(&dir);

    println!("{:<44} {:<9} {:<7} {:<18} {:<24} {:<16} sniff", "file", "format", "type", "metadata", "thumb", "viewer");
    for path in files(&root) {
        let rel = path.strip_prefix(&root).unwrap().to_string_lossy().to_string();
        let format = MediaFormat::from_path(&path);
        let header = MediaDetector::read_header(&path).unwrap_or_default();
        let sniff = MediaDetector::detect_from_bytes(&header).map(|d| d.0.display_name()).unwrap_or("-");
        if !format.is_supported() {
            println!("{rel:<44} {:<9} {:<7} (ignored: unsupported extension) sniff={sniff}", "-", "-");
            continue;
        }
        if !MediaDetector::confirms(format, &header) {
            println!("{rel:<44} {:<9} {:<7} (ignored: header does not match) sniff={sniff}", format.display_name(), "-");
            continue;
        }
        let meta = match MetadataExtractor::extract(&path) {
            Ok(m) => format!("{}x{} {}ms {:?}", m.width, m.height, m.duration_ms, m.media_type),
            Err(e) => format!("ERR {e}"),
        };
        let kind = MediaDetector::content_media_type(&header).or(format.media_type());
        let is_video = kind == Some(MediaType::Video);
        let stem = rel.replace('/', "__");

        let (thumb, viewer) = if is_video {
            let t = match gen_.generate_video_thumbnails(&path, &cfg) {
                Ok(r) => {
                    if let Some(o) = &out {
                        save_rgb(&o.join("thumb"), &stem, r.primary_frame.width, r.primary_frame.height, r.primary_frame.rgb_pixels.clone());
                    }
                    format!("{}x{} (orig {}x{})", r.primary_frame.width, r.primary_frame.height, r.orig_width, r.orig_height)
                }
                Err(e) => format!("ERR {e}"),
            };
            (t, "mpv".to_string())
        } else {
            let t = match gen_.generate_image_thumbnail(&path, cfg.max_bound, ScalingAlgorithm::CatmullRom) {
                Ok(r) => {
                    if let Some(o) = &out {
                        save_rgb(&o.join("thumb"), &stem, r.width, r.height, r.rgb_pixels.clone());
                    }
                    format!("{}x{} (orig {}x{})", r.width, r.height, r.orig_width, r.orig_height)
                }
                Err(e) => format!("ERR {e}"),
            };
            let v = match HighResImageLoader::load_full_resolution(&path) {
                Ok((w, h, buf)) => {
                    if let Some(o) = &out {
                        save_rgb(&o.join("full"), &stem, w, h, buf.as_bytes().to_vec());
                    }
                    format!("{w}x{h}")
                }
                Err(e) => format!("ERR {e}"),
            };
            (t, v)
        };
        println!("{rel:<44} {:<9} {:<7} {meta:<18} {thumb:<24} {viewer:<16} {sniff}", format.display_name(), if is_video { "video" } else { "image" });
    }
}

#[test]
fn print_format_table_as_markdown() {
    if std::env::var("PROBE_TABLE").is_err() {
        return;
    }
    use rustview::media::{FORMAT_SPECS, MediaType};
    for kind in [MediaType::Image, MediaType::Video] {
        println!("\n| Format | id | Extensions | Group | Decoder | Note |\n|---|---|---|---|---|---|");
        let mut specs: Vec<_> = FORMAT_SPECS.iter().filter(|s| s.media_type == Some(kind)).collect();
        specs.sort_by_key(|s| s.ui_sort_key());
        for s in specs {
            let ext: Vec<String> = s.extensions.iter().map(|e| format!(".{e}")).collect();
            println!("| {} | `{}` | {} | {} | {} | {} |", s.long_name, s.id, ext.join(" "), s.group.title(), s.decoder.label(), s.note);
        }
    }
}

#[test]
fn probe_animated_samples() {
    let Ok(dir) = std::env::var("PROBE_ANIM_DIR") else { return };
    use rustview::thumbnails::animated_frames;
    for path in files(Path::new(&dir)) {
        let rel = path.file_name().unwrap().to_string_lossy().to_string();
        let started = std::time::Instant::now();
        match animated_frames::sample(&path, 256, ScalingAlgorithm::CatmullRom, 5) {
            Ok(Some(frames)) => println!("{rel:<44} animated: {} frames {}x{} ({} ms)", frames.len(), frames[0].width, frames[0].height, started.elapsed().as_millis()),
            Ok(None) => println!("{rel:<44} static ({} ms)", started.elapsed().as_millis()),
            Err(e) => println!("{rel:<44} ERR {e}"),
        }
    }
}
